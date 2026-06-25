// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {InflowVaultBase, InflowVault} from "./InflowVaultBase.t.sol";
import {Math} from "@openzeppelin/contracts/utils/math/Math.sol";

/// @notice Tests for accrueFees(), updateFeeConfig(), and high-water-mark logic.
/// Corresponds to all testing_fees.rs tests in control-center (minus multi-vault ones).
contract InflowVaultFeesTest is InflowVaultBase {
    using Math for uint256;

    uint256 internal constant FEE_RATE_20 = WAD / 5; // 20 %
    uint256 internal constant FEE_RATE_10 = WAD / 10; // 10 %

    // ── accrueFees — disabled ─────────────────────────────────────────────────

    function test_accrue_fees_disabled_fee_rate_zero() public {
        _deposit(user, 100_000e6);
        // feeRate == 0: should return without minting.
        vault.accrueFees();
        assertEq(vault.totalSupply(), 100_000e6, "no fee shares minted");
    }

    function test_accrue_fees_no_shares_reverts() public {
        vault = _deployVaultWithFees(FEE_RATE_20);
        // No deposits -> supply == 0, feeRate > 0 -> revert.
        vm.expectRevert(InflowVault.NoSharesIssued.selector);
        vault.accrueFees();
    }

    function test_accrue_fees_no_shares_disabled_no_revert() public {
        // supply == 0, feeRate == 0 -> returns silently.
        vault.accrueFees(); // must not revert
    }

    // ── accrueFees — below HWM ────────────────────────────────────────────────

    function test_accrue_fees_below_hwm_no_mint() public {
        vault = _deployVaultWithFees(FEE_RATE_20);
        _deposit(user, 100_000e6);

        // Share price == 1.0 == HWM -> no fees.
        vault.accrueFees();
        assertEq(vault.balanceOf(feeRecipient), 0, "no shares minted below HWM");
        assertEq(vault.highWaterMarkPrice(), WAD, "HWM unchanged");
    }

    // ── accrueFees — basic yield ──────────────────────────────────────────────

    /// 10% yield, 20% fee rate -> correct shares minted; HWM updated; event emitted.
    /// Corresponds to test_accrue_fees_basic_yield (control-center/testing_fees.rs:577).
    /// Uses direct token mint to vault to avoid submitDeployedAmount calling accrueFees internally.
    function test_accrue_fees_basic_yield() public {
        vault = _deployVaultWithFees(FEE_RATE_20);

        _deposit(user, 100_000e6);

        // Simulate 10% yield by minting tokens directly to vault (no submitDeployedAmount).
        asset.mint(address(vault), 10_000e6);
        // totalAssets = 110k (vault holds), totalSupply = 100k, price = 1.1

        uint256 supply = vault.totalSupply();
        uint256 assets = vault.totalAssets();
        uint256 currentPrice = assets.mulDiv(WAD, supply, Math.Rounding.Floor);
        uint256 yieldPerShare = currentPrice - WAD; // HWM was 1.0
        uint256 totalYield = yieldPerShare.mulDiv(supply, WAD, Math.Rounding.Floor);
        uint256 feeAssets = totalYield.mulDiv(FEE_RATE_20, WAD, Math.Rounding.Floor);
        uint256 expectedShares = feeAssets.mulDiv(WAD, currentPrice, Math.Rounding.Floor);

        vm.expectEmit(true, false, false, true, address(vault));
        emit InflowVault.FeesAccrued(feeRecipient, expectedShares, currentPrice, feeAssets);

        vault.accrueFees();

        assertEq(vault.balanceOf(feeRecipient), expectedShares, "correct fee shares minted");
        assertEq(vault.highWaterMarkPrice(), currentPrice, "HWM updated to current price");
    }

    // ── accrueFees — permissionless ───────────────────────────────────────────

    function test_accrue_fees_permissionless() public {
        vault = _deployVaultWithFees(FEE_RATE_20);
        _deposit(user, 100_000e6);

        // Non-whitelisted stranger can call accrueFees.
        vm.prank(stranger);
        vault.accrueFees(); // must not revert
    }

    // ── accrueFees — zero rate advances HWM ──────────────────────────────────

    /// When feeRate == 0 but price > HWM, HWM must still advance.
    /// Corresponds to test_accrue_fees_zero_fee_rate_is_disabled
    /// (control-center/testing_fees.rs:698).
    function test_accrue_fees_zero_rate_advances_hwm() public {
        _deposit(user, 100_000e6);
        vm.prank(admin);
        vault.withdrawForDeployment(100_000e6);
        vm.prank(admin);
        vault.submitDeployedAmount(120_000e6); // price = 1.2

        vault.accrueFees(); // feeRate == 0

        assertEq(vault.balanceOf(feeRecipient), 0, "no fee shares");
        assertGt(vault.highWaterMarkPrice(), WAD, "HWM advanced");
        assertEq(vault.highWaterMarkPrice(), vault.totalAssets().mulDiv(WAD, vault.totalSupply(), Math.Rounding.Floor));
    }

    // ── dust yield ────────────────────────────────────────────────────────────

    /// When sharesToMint rounds to 0 (dust), HWM must NOT advance.
    /// Corresponds to test_dust_yield_does_not_update_high_water_mark
    /// (control-center/testing_fees.rs:1044).
    /// Uses a small pool (100 USDC) so that 1 wei of yield produces feeAssets = 0.
    function test_accrue_fees_dust_yield_no_hwm_update() public {
        vault = _deployVaultWithFees(FEE_RATE_20);
        // Small pool: 100 USDC (6 decimals). supply = 100e6.
        // 1 wei yield -> totalYield = 1, feeAssets = 1 * 0.2e18 / 1e18 = 0 (dust).
        _deposit(user, 100e6);

        asset.mint(address(vault), 1); // 1 wei yield directly into vault

        uint256 hwmBefore = vault.highWaterMarkPrice();
        vault.accrueFees();

        assertEq(vault.balanceOf(feeRecipient), 0, "dust: no shares minted");
        assertEq(vault.highWaterMarkPrice(), hwmBefore, "dust: HWM must not advance");
    }

    /// Three small yields accumulate; the third crosses the threshold and mints shares.
    /// Uses direct token mints to vault to isolate accrueFees logic.
    function test_accrue_fees_dust_accumulates_then_mints() public {
        vault = _deployVaultWithFees(FEE_RATE_20);
        // Small pool: 100 USDC. supply = 100e6.
        _deposit(user, 100e6);

        uint256 hwmBefore = vault.highWaterMarkPrice();

        // Step 1: 1 wei yield — feeAssets = 0, dust.
        asset.mint(address(vault), 1);
        vault.accrueFees();
        assertEq(vault.balanceOf(feeRecipient), 0, "step 1: dust, no mint");
        assertEq(vault.highWaterMarkPrice(), hwmBefore, "HWM unchanged after dust");

        // Step 2: 1 more wei — still dust (HWM still at 1.0 since not updated).
        asset.mint(address(vault), 1);
        vault.accrueFees();
        assertEq(vault.balanceOf(feeRecipient), 0, "step 2: dust, no mint");

        // Step 3: 10 USDC yield on 100 USDC pool = 10% -> mints shares.
        asset.mint(address(vault), 10e6);
        vault.accrueFees();

        assertGt(vault.balanceOf(feeRecipient), 0, "step 3: yield crosses threshold, mints");
        assertGt(vault.highWaterMarkPrice(), hwmBefore, "HWM updated after mint");
    }

    // ── HWM consecutive accruals ──────────────────────────────────────────────

    /// Multiple yield steps; HWM advances each time.
    /// Corresponds to test_high_water_mark_consecutive_accruals (testing_fees.rs:840).
    function test_hwm_consecutive_accruals() public {
        vault = _deployVaultWithFees(FEE_RATE_10);
        _deposit(user, 100_000e6);
        vm.prank(admin);
        vault.withdrawForDeployment(100_000e6);

        // Step 1: 5% yield.
        vm.prank(admin);
        vault.submitDeployedAmount(105_000e6);
        vault.accrueFees();
        uint256 hwm1 = vault.highWaterMarkPrice();
        assertGt(hwm1, WAD, "HWM > 1.0 after step 1");

        // Step 2: further yield.
        vm.prank(admin);
        vault.submitDeployedAmount(115_000e6);
        vault.accrueFees();
        uint256 hwm2 = vault.highWaterMarkPrice();
        assertGt(hwm2, hwm1, "HWM advanced after step 2");

        // Step 3: more yield.
        vm.prank(admin);
        vault.submitDeployedAmount(130_000e6);
        vault.accrueFees();
        uint256 hwm3 = vault.highWaterMarkPrice();
        assertGt(hwm3, hwm2, "HWM advanced after step 3");
    }

    // ── HWM — recovery from loss ──────────────────────────────────────────────

    /// Loss -> partial recovery -> new high: fees only charged above old HWM.
    /// Corresponds to test_high_water_mark_recovery_from_loss (testing_fees.rs:928).
    function test_hwm_recovery_from_loss() public {
        vault = _deployVaultWithFees(FEE_RATE_20);
        _deposit(user, 100_000e6);
        vm.prank(admin);
        vault.withdrawForDeployment(100_000e6);

        // Yield to 1.2.
        vm.prank(admin);
        vault.submitDeployedAmount(120_000e6);
        vault.accrueFees();
        uint256 hwmAfterGain = vault.highWaterMarkPrice();
        assertGt(hwmAfterGain, WAD);

        uint256 feeSharesAfterGain = vault.balanceOf(feeRecipient);

        // Loss to 0.9.
        vm.prank(admin);
        vault.submitDeployedAmount(90_000e6);
        vault.accrueFees();
        assertEq(vault.highWaterMarkPrice(), hwmAfterGain, "HWM stays at prior high during loss");
        assertEq(vault.balanceOf(feeRecipient), feeSharesAfterGain, "no new fees during loss");

        // Partial recovery to 1.1 (still below 1.2 HWM).
        vm.prank(admin);
        vault.submitDeployedAmount(110_000e6);
        vault.accrueFees();
        assertEq(vault.highWaterMarkPrice(), hwmAfterGain, "HWM stays during recovery below prior high");
        assertEq(vault.balanceOf(feeRecipient), feeSharesAfterGain, "no new fees below prior HWM");

        // New high at 1.3.
        vm.prank(admin);
        vault.submitDeployedAmount(130_000e6);
        vault.accrueFees();
        assertGt(vault.highWaterMarkPrice(), hwmAfterGain, "HWM advanced to new high");
        assertGt(vault.balanceOf(feeRecipient), feeSharesAfterGain, "fees charged on gain above old HWM");
    }

    // ── updateFeeConfig ───────────────────────────────────────────────────────

    function test_update_fee_config_partial_rate_only() public {
        vault = _deployVaultWithFees(FEE_RATE_10);
        address originalRecipient = vault.feeRecipient();

        vm.prank(admin);
        vault.updateFeeConfig(FEE_RATE_20, address(0)); // address(0) means keep current recipient

        assertEq(vault.feeRate(), FEE_RATE_20, "rate updated");
        assertEq(vault.feeRecipient(), originalRecipient, "recipient unchanged");
    }

    function test_update_fee_config_change_recipient() public {
        vault = _deployVaultWithFees(FEE_RATE_10);
        address newRecipient = makeAddr("newRecipient");

        vm.prank(admin);
        vault.updateFeeConfig(FEE_RATE_10, newRecipient);

        assertEq(vault.feeRecipient(), newRecipient);
        assertEq(vault.feeRate(), FEE_RATE_10, "rate unchanged");
    }

    function test_update_fee_config_disable_by_zero_rate() public {
        vault = _deployVaultWithFees(FEE_RATE_20);

        vm.prank(admin);
        vault.updateFeeConfig(0, address(0));

        assertEq(vault.feeRate(), 0, "fees disabled");
    }

    function test_update_fee_config_unauthorized() public {
        vault = _deployVaultWithFees(FEE_RATE_20);

        vm.prank(stranger);
        vm.expectRevert(InflowVault.Unauthorized.selector);
        vault.updateFeeConfig(FEE_RATE_10, address(0));
    }

    function test_update_fee_config_invalid_rate_reverts() public {
        vm.prank(admin);
        vm.expectRevert(InflowVault.InvalidFeeRate.selector);
        vault.updateFeeConfig(WAD + 1, feeRecipient);
    }

    function test_update_fee_config_nonzero_rate_without_any_recipient_reverts() public {
        // vault deployed with feeRate=0 and no recipient.
        vm.prank(admin);
        vm.expectRevert(InflowVault.FeeRecipientNotSet.selector);
        vault.updateFeeConfig(FEE_RATE_10, address(0));
    }

    function test_update_fee_config_emits_event() public {
        vault = _deployVaultWithFees(FEE_RATE_10);

        vm.expectEmit(false, false, false, true, address(vault));
        emit InflowVault.FeeConfigUpdated(FEE_RATE_20, feeRecipient);

        vm.prank(admin);
        vault.updateFeeConfig(FEE_RATE_20, address(0));
    }

    // ── re-enable fees advances HWM ───────────────────────────────────────────

    /// When re-enabling fees after a period with feeRate=0, HWM is set to
    /// max(existing HWM, current price) to avoid backdating charges.
    /// Corresponds to test_reenable_fees_resets_high_water_mark (testing_fees.rs:1181).
    function test_update_fee_config_reenable_advances_hwm() public {
        vault = _deployVaultWithFees(FEE_RATE_20);
        _deposit(user, 100_000e6);
        vm.prank(admin);
        vault.withdrawForDeployment(100_000e6);

        // Accrue at 10% yield (HWM -> 1.1).
        vm.prank(admin);
        vault.submitDeployedAmount(110_000e6);
        vault.accrueFees();
        uint256 hwmAfterFirstAccrual = vault.highWaterMarkPrice();

        // Disable fees.
        vm.prank(admin);
        vault.updateFeeConfig(0, address(0));

        // Simulate 40% yield while disabled (price goes high).
        vm.prank(admin);
        vault.submitDeployedAmount(150_000e6);

        uint256 priceWhileDisabled = vault.totalAssets().mulDiv(WAD, vault.totalSupply(), Math.Rounding.Floor);

        // Re-enable fees — HWM should jump to current price (1.5) not old 1.1.
        vm.prank(admin);
        vault.updateFeeConfig(FEE_RATE_20, address(0));

        assertEq(vault.highWaterMarkPrice(), priceWhileDisabled, "HWM reset to current price on re-enable");
        assertGt(vault.highWaterMarkPrice(), hwmAfterFirstAccrual, "HWM must exceed old value");
    }

    // ── submitDeployedAmount — fee interaction ────────────────────────────────

    function test_submit_deployed_amount_with_fees_disabled() public {
        _deposit(user, 100_000e6);
        vm.prank(admin);
        vault.withdrawForDeployment(100_000e6);

        // feeRate = 0 -> no revert, no fees.
        vm.prank(admin);
        vault.submitDeployedAmount(120_000e6);

        assertEq(vault.deployedAmount(), 120_000e6);
        assertEq(vault.balanceOf(feeRecipient), 0, "no fees when disabled");
    }
}
