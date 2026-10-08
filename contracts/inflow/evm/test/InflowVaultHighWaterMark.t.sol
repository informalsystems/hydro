// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {InflowVaultBase, InflowVault} from "./InflowVaultBase.t.sol";
import {InflowVaultV1} from "./mocks/InflowVaultV1.sol";
import {UpgradeInflowVaultResetHwm} from "../script/UpgradeInflowVaultResetHwm.s.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {ERC1967Proxy} from "@openzeppelin/contracts/proxy/ERC1967/ERC1967Proxy.sol";
import {Initializable} from "@openzeppelin/contracts/proxy/utils/Initializable.sol";
import {Math} from "@openzeppelin/contracts/utils/math/Math.sol";
import {Vm} from "forge-std/Vm.sol";

/// @notice Tests for the post-mint high-water mark recorded by accrueFees() and for the
/// one-shot resetHighWaterMark() available to vaults initialized at version 1.
/// The post-mint accrual tests correspond to test_high_water_mark_is_post_mint_share_price
/// (control-center/testing_fees.rs); the reset has no CosmWasm counterpart.
contract InflowVaultHighWaterMarkTest is InflowVaultBase {
    using Math for uint256;

    uint256 internal constant FEE_RATE_10 = WAD / 10; // 10 %

    /// @dev ERC-7201 slot of OpenZeppelin's Initializable storage; the low 8 bytes hold the version.
    bytes32 internal constant INITIALIZABLE_STORAGE =
        0xf0c57e16840df040f15088dc2f81fe391c3923bec73e23a9662efc9c229c6a00;
    bytes32 internal constant IMPL_SLOT = bytes32(uint256(keccak256("eip1967.proxy.implementation")) - 1);

    // State of the Cycles USDC risk-2 vault on Arc mainnet at its only fee accrual (block 21008843).
    uint256 internal constant CYCLES_SUPPLY_AT_ACCRUAL = 800_902;
    uint256 internal constant CYCLES_ASSETS_AT_ACCRUAL = 881_006;
    uint256 internal constant CYCLES_FEE_SHARES = 7_281;
    uint256 internal constant CYCLES_FEE_ASSETS = 8_010;
    uint256 internal constant CYCLES_HWM = 1_100_017_230_572_529_473;

    // ── helpers ───────────────────────────────────────────────────────────────

    function _sharePrice(InflowVault v) internal view returns (uint256) {
        return v.totalAssets().mulDiv(WAD, v.totalSupply(), Math.Rounding.Floor);
    }

    function _initializedVersion(address proxy) internal view returns (uint64) {
        return uint64(uint256(vm.load(proxy, INITIALIZABLE_STORAGE)));
    }

    function _implementation(address proxy) internal view returns (address) {
        return address(uint160(uint256(vm.load(proxy, IMPL_SLOT))));
    }

    /// @dev True when a HighWaterMarkReset event was recorded since the last vm.recordLogs().
    function _emittedHighWaterMarkReset() internal returns (bool) {
        Vm.Log[] memory logs = vm.getRecordedLogs();
        for (uint256 i = 0; i < logs.length; i++) {
            if (logs[i].topics[0] == InflowVault.HighWaterMarkReset.selector) return true;
        }
        return false;
    }

    /// @dev Deploy a proxy on the version-1 implementation with a 10% fee.
    function _deployV1Vault() internal returns (InflowVaultV1) {
        address[] memory wl = new address[](1);
        wl[0] = admin;

        InflowVaultV1 impl = new InflowVaultV1();
        bytes memory init = abi.encodeCall(
            InflowVaultV1.initialize,
            (
                IERC20(address(asset)),
                "Hydro Inflow Vault",
                "hvUSDC",
                DEPOSIT_CAP,
                MAX_WITHDRAWALS,
                wl,
                wl,
                FEE_RATE_10,
                feeRecipient
            )
        );
        return InflowVaultV1(address(new ERC1967Proxy(address(impl), init)));
    }

    function _depositV1(InflowVaultV1 v1, address who, uint256 amount) internal {
        asset.mint(who, amount);
        vm.prank(who);
        asset.approve(address(v1), amount);
        vm.prank(who);
        v1.deposit(amount, who);
    }

    /// @dev Version-1 vault brought to the state of the Cycles vault right after its only
    /// accrual: the HWM holds the pre-mint price and the share price sits below it.
    function _deployV1VaultAfterCyclesAccrual() internal returns (InflowVaultV1 v1) {
        v1 = _deployV1Vault();
        _depositV1(v1, user, CYCLES_SUPPLY_AT_ACCRUAL);
        asset.mint(address(v1), CYCLES_ASSETS_AT_ACCRUAL - CYCLES_SUPPLY_AT_ACCRUAL);

        vm.expectEmit(true, false, false, true, address(v1));
        emit InflowVaultV1.FeesAccrued(feeRecipient, CYCLES_FEE_SHARES, CYCLES_HWM, CYCLES_FEE_ASSETS);
        v1.accrueFees();

        assertEq(v1.highWaterMarkPrice(), CYCLES_HWM, "v1 stores the pre-mint price");
        assertEq(v1.totalSupply(), CYCLES_SUPPLY_AT_ACCRUAL + CYCLES_FEE_SHARES);
    }

    /// @dev Upgrade a version-1 proxy to the current implementation without any migration call.
    function _upgradeWithoutReset(InflowVaultV1 v1) internal returns (InflowVault v) {
        InflowVault newImpl = new InflowVault();
        vm.prank(admin);
        v1.upgradeToAndCall(address(newImpl), "");
        v = InflowVault(address(v1));
    }

    // ── accrueFees: HWM is the post-mint price ───────────────────────────────

    /// After an accrual the HWM equals the share price, and a following small gain is charged.
    function test_accrue_fees_hwm_is_post_mint_share_price() public {
        vault = _deployVaultWithFees(FEE_RATE_10);
        _deposit(user, 100_000e6);
        asset.mint(address(vault), 10_000e6); // 10% yield, price = 1.1

        uint256 preMintPrice = _sharePrice(vault);
        vault.accrueFees();

        uint256 feeShares = vault.balanceOf(feeRecipient);
        assertGt(feeShares, 0, "fee shares minted");
        assertEq(vault.highWaterMarkPrice(), _sharePrice(vault), "HWM == post-mint share price");
        assertEq(
            vault.highWaterMarkPrice(),
            uint256(110_000e6).mulDiv(WAD, 100_000e6 + feeShares, Math.Rounding.Floor),
            "HWM == assets / (supply + fee shares)"
        );
        assertLt(vault.highWaterMarkPrice(), preMintPrice, "HWM below the pre-mint price");

        // No gain since the accrual: nothing to charge.
        vault.accrueFees();
        assertEq(vault.balanceOf(feeRecipient), feeShares, "no fee without a gain");

        // 0.1% gain: the price is still below the pre-mint price of the first accrual,
        // yet the gain is charged because the HWM is the post-mint price.
        asset.mint(address(vault), 110e6);
        uint256 priceBeforeSecondAccrual = _sharePrice(vault);
        assertLt(priceBeforeSecondAccrual, preMintPrice, "still below the first pre-mint price");

        uint256 supply = vault.totalSupply();
        uint256 totalYield = (priceBeforeSecondAccrual - vault.highWaterMarkPrice()).mulDiv(supply, WAD);
        uint256 feeAssets = totalYield.mulDiv(FEE_RATE_10, WAD);
        uint256 expectedShares = feeAssets.mulDiv(WAD, priceBeforeSecondAccrual);
        assertApproxEqAbs(feeAssets, 11e6, 1, "10% of the 110 USDC gain");

        vm.expectEmit(true, false, false, true, address(vault));
        emit InflowVault.FeesAccrued(feeRecipient, expectedShares, priceBeforeSecondAccrual, feeAssets);
        vault.accrueFees();

        assertEq(vault.balanceOf(feeRecipient), feeShares + expectedShares, "small gain charged immediately");
        assertEq(vault.highWaterMarkPrice(), _sharePrice(vault), "HWM == share price after second accrual");
    }

    /// The HWM never decreases through accrueFees(), including at a 100% fee rate where the
    /// whole gain is minted away and the post-mint price lands back on the previous HWM.
    function test_accrue_fees_hwm_never_decreases_at_full_fee_rate() public {
        vault = _deployVaultWithFees(WAD);
        _deposit(user, 100_000e6);

        uint256 hwm = vault.highWaterMarkPrice();
        for (uint256 i = 0; i < 5; i++) {
            asset.mint(address(vault), 1_234_567 * (i + 1));
            vault.accrueFees();
            assertGe(vault.highWaterMarkPrice(), hwm, "HWM never decreases");
            assertEq(vault.highWaterMarkPrice(), _sharePrice(vault), "HWM == share price");
            hwm = vault.highWaterMarkPrice();
        }
    }

    /// Fuzz: whenever an accrual mints fee shares, the HWM ends equal to the share price and
    /// does not move below its previous value.
    function testFuzz_accrue_fees_hwm_equals_share_price(uint256 depositAmount, uint256 gain, uint256 feeRate_) public {
        depositAmount = bound(depositAmount, 1, 500_000e6);
        gain = bound(gain, 0, 500_000e6);
        feeRate_ = bound(feeRate_, 1, WAD);

        vault = _deployVaultWithFees(feeRate_);
        _deposit(user, depositAmount);
        asset.mint(address(vault), gain);

        uint256 hwmBefore = vault.highWaterMarkPrice();
        vault.accrueFees();

        if (vault.balanceOf(feeRecipient) > 0) {
            assertEq(vault.highWaterMarkPrice(), _sharePrice(vault), "HWM == share price");
            assertGe(vault.highWaterMarkPrice(), hwmBefore, "HWM never decreases");
        } else {
            assertEq(vault.highWaterMarkPrice(), hwmBefore, "HWM untouched without a mint");
        }
    }

    // ── Cycles case ───────────────────────────────────────────────────────────

    /// Replay of the Cycles vault history: 0.4 assets / 0.4 shares, 0.04 donated, first accrual.
    function test_cycles_donation_replay_hwm_equals_share_price() public {
        vault = _deployVaultWithFees(FEE_RATE_10);
        _deposit(user, 400_000); // 0.4 USDC -> 0.4 shares
        asset.mint(address(vault), 40_000); // 0.04 USDC sent straight to the vault

        assertEq(_sharePrice(vault), 1.1e18, "donation takes the price to 1.1");

        // feeAssets = 40_000 * 10% = 4_000; sharesToMint = 4_000 / 1.1 = 3_636
        vm.expectEmit(true, false, false, true, address(vault));
        emit InflowVault.FeesAccrued(feeRecipient, 3_636, 1.1e18, 4_000);
        vault.accrueFees();

        assertEq(vault.totalSupply(), 403_636);
        assertEq(vault.highWaterMarkPrice(), _sharePrice(vault), "HWM == share price");
        assertEq(vault.highWaterMarkPrice(), uint256(440_000).mulDiv(WAD, 403_636), "HWM ~ 1.0901");

        // The next gain is charged: 1_000 of yield -> 100 fee assets -> 91 fee shares.
        asset.mint(address(vault), 1_000);
        vault.accrueFees();
        assertEq(vault.balanceOf(feeRecipient), 3_636 + 91, "next gain accrues fees");
        assertEq(vault.highWaterMarkPrice(), _sharePrice(vault), "HWM == share price");
    }

    /// Same accrual with the exact on-chain figures of block 21008843.
    function test_cycles_accrual_onchain_figures_hwm_equals_share_price() public {
        vault = _deployVaultWithFees(FEE_RATE_10);
        _deposit(user, CYCLES_SUPPLY_AT_ACCRUAL);
        asset.mint(address(vault), CYCLES_ASSETS_AT_ACCRUAL - CYCLES_SUPPLY_AT_ACCRUAL);

        vm.expectEmit(true, false, false, true, address(vault));
        emit InflowVault.FeesAccrued(feeRecipient, CYCLES_FEE_SHARES, CYCLES_HWM, CYCLES_FEE_ASSETS);
        vault.accrueFees();

        assertEq(vault.highWaterMarkPrice(), _sharePrice(vault), "HWM == share price");
        assertEq(vault.highWaterMarkPrice(), 1_090_107_067_334_997_147, "HWM == 881006 / 808183");
    }

    // ── resetHighWaterMark ────────────────────────────────────────────────────

    function test_reset_hwm_lowers_to_current_price_and_emits() public {
        InflowVaultV1 v1 = _deployV1VaultAfterCyclesAccrual();
        InflowVault v = _upgradeWithoutReset(v1);

        uint256 price = _sharePrice(v);
        assertLt(price, CYCLES_HWM, "price below the stored HWM");
        assertEq(_initializedVersion(address(v)), 1);

        vm.expectEmit(false, false, false, true, address(v));
        emit InflowVault.HighWaterMarkReset(CYCLES_HWM, price);
        vm.expectEmit(false, false, false, true, address(v));
        emit Initializable.Initialized(2);

        vm.prank(admin);
        v.resetHighWaterMark();

        assertEq(v.highWaterMarkPrice(), price, "HWM lowered to the current price");
        assertEq(v.highWaterMarkPrice(), 1_090_107_067_334_997_147);
        assertEq(_initializedVersion(address(v)), 2);
    }

    function test_reset_hwm_second_call_reverts() public {
        InflowVault v = _upgradeWithoutReset(_deployV1VaultAfterCyclesAccrual());

        vm.prank(admin);
        v.resetHighWaterMark();

        // A loss puts the price below the HWM again; the reset is still unavailable.
        vm.prank(admin);
        v.withdrawForDeployment(100_000);
        vm.prank(admin);
        v.submitDeployedAmount(50_000);
        assertLt(_sharePrice(v), v.highWaterMarkPrice());

        uint256 hwm = v.highWaterMarkPrice();
        vm.prank(admin);
        vm.expectRevert(Initializable.InvalidInitialization.selector);
        v.resetHighWaterMark();
        assertEq(v.highWaterMarkPrice(), hwm, "HWM unchanged");
    }

    function test_reset_hwm_is_noop_when_price_equals_hwm() public {
        InflowVaultV1 v1 = _deployV1Vault();
        _depositV1(v1, user, 100_000e6);
        InflowVault v = _upgradeWithoutReset(v1);

        vm.recordLogs();
        vm.prank(admin);
        v.resetHighWaterMark();

        assertEq(v.highWaterMarkPrice(), WAD, "HWM unchanged");
        assertFalse(_emittedHighWaterMarkReset(), "no HighWaterMarkReset on a no-op");
        assertEq(_initializedVersion(address(v)), 2, "one-shot consumed by the no-op");

        vm.prank(admin);
        vm.expectRevert(Initializable.InvalidInitialization.selector);
        v.resetHighWaterMark();
    }

    function test_reset_hwm_is_noop_when_price_above_hwm() public {
        InflowVaultV1 v1 = _deployV1Vault();
        _depositV1(v1, user, 100_000e6);
        asset.mint(address(v1), 5_000e6); // price = 1.05, HWM = 1.0, fees not accrued yet
        InflowVault v = _upgradeWithoutReset(v1);

        vm.recordLogs();
        vm.prank(admin);
        v.resetHighWaterMark();

        assertEq(v.highWaterMarkPrice(), WAD, "HWM not raised to the share price");
        assertFalse(_emittedHighWaterMarkReset(), "no HighWaterMarkReset on a no-op");
        assertEq(_initializedVersion(address(v)), 2, "one-shot consumed by the no-op");

        // The pending gain is still charged in full against the untouched HWM.
        v.accrueFees();
        assertGt(v.balanceOf(feeRecipient), 0, "pending gain still charged");
    }

    function test_reset_hwm_is_noop_without_shares_at_initial_hwm() public {
        InflowVault v = _upgradeWithoutReset(_deployV1Vault());
        assertEq(v.totalSupply(), 0);

        vm.recordLogs();
        vm.prank(admin);
        v.resetHighWaterMark();

        assertEq(v.highWaterMarkPrice(), WAD, "HWM unchanged");
        assertFalse(_emittedHighWaterMarkReset(), "no HighWaterMarkReset on a no-op");
        assertEq(_initializedVersion(address(v)), 2, "one-shot consumed by the no-op");
    }

    /// A vault emptied after an accrual keeps its HWM; the reset brings it back to WAD, the
    /// mark a new vault starts from.
    function test_reset_hwm_without_shares_lowers_to_wad() public {
        InflowVaultV1 v1 = _deployV1VaultAfterCyclesAccrual();

        uint256 userShares = v1.balanceOf(user);
        vm.prank(user);
        v1.redeem(userShares, user, user);
        vm.prank(feeRecipient);
        v1.redeem(CYCLES_FEE_SHARES, feeRecipient, feeRecipient);
        assertEq(v1.totalSupply(), 0, "vault emptied");
        assertEq(v1.highWaterMarkPrice(), CYCLES_HWM, "HWM survives the withdrawals");

        InflowVault v = _upgradeWithoutReset(v1);

        vm.expectEmit(false, false, false, true, address(v));
        emit InflowVault.HighWaterMarkReset(CYCLES_HWM, WAD);
        vm.prank(admin);
        v.resetHighWaterMark();

        assertEq(v.highWaterMarkPrice(), WAD, "HWM lowered to WAD");
        assertEq(_initializedVersion(address(v)), 2);
    }

    function test_reset_hwm_unauthorized_reverts() public {
        InflowVault v = _upgradeWithoutReset(_deployV1VaultAfterCyclesAccrual());

        vm.prank(stranger);
        vm.expectRevert(InflowVault.Unauthorized.selector);
        v.resetHighWaterMark();

        // Deployed-amount whitelist membership alone is not enough.
        address submitter = makeAddr("submitter");
        vm.prank(admin);
        v.addToDeployedAmountWhitelist(submitter);
        vm.prank(submitter);
        vm.expectRevert(InflowVault.Unauthorized.selector);
        v.resetHighWaterMark();

        assertEq(v.highWaterMarkPrice(), CYCLES_HWM, "HWM unchanged");
        assertEq(_initializedVersion(address(v)), 1);
    }

    // ── upgrade of a version-1 vault ──────────────────────────────────────────

    /// Upgrade from the version-1 implementation through upgradeToAndCall(resetHighWaterMark),
    /// from a state mirroring the Cycles vault: HWM 1.100017230572529473, price ~1.0929.
    function test_upgrade_from_v1_with_reset_preserves_state_and_resumes_fees() public {
        InflowVaultV1 v1 = _deployV1VaultAfterCyclesAccrual();

        // Real deposits come in at the diluted price, part of the funds are deployed, and the
        // reported position earns ~0.26%: price ~1.0929, still below the HWM.
        _depositV1(v1, alice, 150_000e6);
        vm.prank(admin);
        v1.withdrawForDeployment(100_000e6);
        vm.prank(admin);
        v1.submitDeployedAmount(100_390e6);
        v1.accrueFees();

        uint256 price = v1.totalAssets().mulDiv(WAD, v1.totalSupply(), Math.Rounding.Floor);
        assertApproxEqAbs(price, 1.0929e18, 0.0001e18, "price near 1.0929");
        assertEq(v1.highWaterMarkPrice(), CYCLES_HWM, "v1 HWM stuck above the price");
        assertEq(v1.balanceOf(feeRecipient), CYCLES_FEE_SHARES, "v1 charges nothing on the gain");

        uint256 totalAssetsBefore = v1.totalAssets();
        uint256 totalSupplyBefore = v1.totalSupply();
        uint256 aliceSharesBefore = v1.balanceOf(alice);
        uint256 userSharesBefore = v1.balanceOf(user);
        uint256 deployedBefore = v1.deployedAmount();

        InflowVault newImpl = new InflowVault();
        bytes memory resetCall = abi.encodeCall(InflowVault.resetHighWaterMark, ());

        // A non-whitelisted caller cannot perform the upgrade.
        vm.prank(stranger);
        vm.expectRevert(InflowVault.Unauthorized.selector);
        v1.upgradeToAndCall(address(newImpl), resetCall);

        vm.expectEmit(false, false, false, true, address(v1));
        emit InflowVault.HighWaterMarkReset(CYCLES_HWM, price);
        vm.prank(admin);
        v1.upgradeToAndCall(address(newImpl), resetCall);

        InflowVault v = InflowVault(address(v1));

        assertEq(_implementation(address(v)), address(newImpl), "implementation switched");
        assertEq(_initializedVersion(address(v)), 2);
        assertEq(v.highWaterMarkPrice(), price, "HWM reset to the current price");

        assertEq(v.totalAssets(), totalAssetsBefore, "totalAssets preserved");
        assertEq(v.totalSupply(), totalSupplyBefore, "totalSupply preserved");
        assertEq(v.balanceOf(alice), aliceSharesBefore, "alice shares preserved");
        assertEq(v.balanceOf(user), userSharesBefore, "user shares preserved");
        assertEq(v.balanceOf(feeRecipient), CYCLES_FEE_SHARES, "fee shares preserved");
        assertEq(v.deployedAmount(), deployedBefore, "deployedAmount preserved");
        assertEq(v.depositCap(), DEPOSIT_CAP, "depositCap preserved");
        assertEq(v.maxWithdrawalsPerUser(), MAX_WITHDRAWALS, "maxWithdrawalsPerUser preserved");
        assertEq(v.feeRate(), FEE_RATE_10, "feeRate preserved");
        assertEq(v.feeRecipient(), feeRecipient, "feeRecipient preserved");
        assertEq(v.asset(), address(asset), "asset preserved");
        assertEq(v.name(), "Hydro Inflow Vault", "name preserved");
        assertTrue(v.whitelist(admin), "whitelist preserved");
        assertTrue(v.deployedAmountWhitelist(admin), "deployed amount whitelist preserved");

        // The next gain, however small, accrues fees: +10 USDC on the deployed position.
        vm.expectEmit(true, false, false, false, address(v));
        emit InflowVault.FeesAccrued(feeRecipient, 0, 0, 0);
        vm.prank(admin);
        v.submitDeployedAmount(100_400e6);

        assertGt(v.balanceOf(feeRecipient), CYCLES_FEE_SHARES, "fees resume on the next gain");
        // The fee shares are worth 10% of the 10 USDC gain, less their own share of the dilution.
        assertApproxEqAbs(
            v.convertToAssets(v.balanceOf(feeRecipient) - CYCLES_FEE_SHARES), 1e6, 20, "10% of the 10 USDC gain"
        );
        assertEq(v.highWaterMarkPrice(), _sharePrice(v), "HWM == post-mint share price");

        // Neither one-shot entry point is left open after the upgrade.
        vm.prank(admin);
        vm.expectRevert(Initializable.InvalidInitialization.selector);
        v.resetHighWaterMark();
        _expectInitializeReverts(v);
    }

    /// initialize() cannot be replayed on a version-1 vault, whether or not the reset ran.
    function test_initialize_reverts_on_upgraded_v1_vault() public {
        InflowVault v = _upgradeWithoutReset(_deployV1VaultAfterCyclesAccrual());
        _expectInitializeReverts(v);
        assertEq(_initializedVersion(address(v)), 1);
        assertFalse(v.whitelist(stranger));

        vm.prank(admin);
        v.resetHighWaterMark();
        _expectInitializeReverts(v);
    }

    function _expectInitializeReverts(InflowVault v) internal {
        address[] memory wl = new address[](1);
        wl[0] = stranger;

        vm.prank(stranger);
        vm.expectRevert(Initializable.InvalidInitialization.selector);
        v.initialize(IERC20(address(asset)), "Vault", "V", DEPOSIT_CAP, MAX_WITHDRAWALS, wl, wl, 0, address(0));
    }

    // ── upgrade script ────────────────────────────────────────────────────────

    /// The calldata printed by the upgrade script, sent by the whitelisted admin, performs
    /// the upgrade and the reset.
    function test_upgrade_script_calldata_upgrades_and_resets() public {
        InflowVaultV1 v1 = _deployV1VaultAfterCyclesAccrual();
        InflowVault newImpl = new InflowVault();
        UpgradeInflowVaultResetHwm script = new UpgradeInflowVaultResetHwm();

        uint256 snapshot = vm.snapshotState();
        bytes memory upgradeCalldata = script.simulateUpgrade(address(v1), admin, address(newImpl));
        vm.revertToState(snapshot);

        assertEq(
            upgradeCalldata,
            abi.encodeWithSignature(
                "upgradeToAndCall(address,bytes)", address(newImpl), abi.encodeWithSignature("resetHighWaterMark()")
            ),
            "calldata is upgradeToAndCall(newImpl, resetHighWaterMark())"
        );
        assertEq(v1.highWaterMarkPrice(), CYCLES_HWM, "simulation rolled back");

        vm.prank(admin);
        (bool success,) = address(v1).call(upgradeCalldata);
        assertTrue(success, "upgrade call succeeds");

        InflowVault v = InflowVault(address(v1));
        assertEq(_implementation(address(v)), address(newImpl), "implementation switched");
        assertEq(v.highWaterMarkPrice(), _sharePrice(v), "HWM reset to the current price");
    }

    /// Without an implementation address and without DEPLOY_IMPLEMENTATION, the script
    /// deploys the implementation in the simulation only and validates the upgrade against it.
    function test_upgrade_script_dry_run_without_implementation() public {
        InflowVaultV1 v1 = _deployV1VaultAfterCyclesAccrual();
        UpgradeInflowVaultResetHwm script = new UpgradeInflowVaultResetHwm();

        bytes memory upgradeCalldata = script.prepare(address(v1), admin, address(0), false);

        InflowVault v = InflowVault(address(v1));
        assertEq(
            upgradeCalldata,
            abi.encodeCall(
                v.upgradeToAndCall, (_implementation(address(v)), abi.encodeCall(InflowVault.resetHighWaterMark, ()))
            )
        );
        assertEq(v.highWaterMarkPrice(), _sharePrice(v), "simulated upgrade reset the HWM");
    }

    function test_upgrade_script_rejects_non_whitelisted_safe() public {
        InflowVaultV1 v1 = _deployV1VaultAfterCyclesAccrual();
        InflowVault newImpl = new InflowVault();
        UpgradeInflowVaultResetHwm script = new UpgradeInflowVaultResetHwm();

        vm.expectRevert("ADMIN_SAFE is not whitelisted on the proxy");
        script.simulateUpgrade(address(v1), stranger, address(newImpl));
    }

    /// With the share price at the HWM, the script reports a no-op reset and the upgrade
    /// still goes through, consuming the one-shot.
    function test_upgrade_script_noop_reset_when_price_not_below_hwm() public {
        InflowVaultV1 v1 = _deployV1Vault();
        _depositV1(v1, user, 100_000e6);
        InflowVault newImpl = new InflowVault();
        UpgradeInflowVaultResetHwm script = new UpgradeInflowVaultResetHwm();

        script.simulateUpgrade(address(v1), admin, address(newImpl));

        InflowVault v = InflowVault(address(v1));
        assertEq(_implementation(address(v)), address(newImpl), "implementation switched");
        assertEq(v.highWaterMarkPrice(), WAD, "HWM unchanged");
        assertEq(_initializedVersion(address(v)), 2, "one-shot consumed");
    }

    /// A vault without shares does not make the script divide by zero.
    function test_upgrade_script_handles_vault_without_shares() public {
        InflowVaultV1 v1 = _deployV1Vault();
        InflowVault newImpl = new InflowVault();
        UpgradeInflowVaultResetHwm script = new UpgradeInflowVaultResetHwm();

        script.simulateUpgrade(address(v1), admin, address(newImpl));

        InflowVault v = InflowVault(address(v1));
        assertEq(_implementation(address(v)), address(newImpl), "implementation switched");
        assertEq(v.highWaterMarkPrice(), WAD, "HWM unchanged");
        assertEq(_initializedVersion(address(v)), 2, "one-shot consumed");
    }

    // ── freshly deployed vault ────────────────────────────────────────────────

    /// A vault deployed from the current implementation has no usable reset, even when its
    /// share price is below the HWM.
    function test_fresh_vault_has_no_usable_reset() public {
        vault = _deployVaultWithFees(FEE_RATE_10);
        assertEq(_initializedVersion(address(vault)), 2, "initialize() consumes the reset version");

        _deposit(user, 100_000e6);
        asset.mint(address(vault), 10_000e6);
        vault.accrueFees();

        // Loss: price drops below the HWM.
        vm.prank(admin);
        vault.withdrawForDeployment(50_000e6);
        vm.prank(admin);
        vault.submitDeployedAmount(40_000e6);
        uint256 hwm = vault.highWaterMarkPrice();
        assertLt(_sharePrice(vault), hwm, "price below the HWM");

        vm.prank(admin);
        vm.expectRevert(Initializable.InvalidInitialization.selector);
        vault.resetHighWaterMark();

        vm.prank(stranger);
        vm.expectRevert(InflowVault.Unauthorized.selector);
        vault.resetHighWaterMark();

        // Upgrading a fresh vault to a new copy of the implementation does not reopen it.
        InflowVault newImpl = new InflowVault();
        vm.prank(admin);
        vm.expectRevert(Initializable.InvalidInitialization.selector);
        vault.upgradeToAndCall(address(newImpl), abi.encodeCall(InflowVault.resetHighWaterMark, ()));

        assertEq(vault.highWaterMarkPrice(), hwm, "HWM unchanged");
    }

    /// The implementation contract itself accepts neither one-shot entry point.
    function test_implementation_contract_has_no_usable_reset() public {
        InflowVault impl = new InflowVault();
        assertEq(_initializedVersion(address(impl)), type(uint64).max, "initializers disabled");

        vm.prank(admin);
        vm.expectRevert(InflowVault.Unauthorized.selector);
        impl.resetHighWaterMark();

        _expectInitializeReverts(impl);
    }
}
