// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {InflowVaultBase, InflowVault, MockAdapterWithAsset, MockERC20} from "./InflowVaultBase.t.sol";
import {RevertingAvailabilityAdapter, ShortfallAdapter} from "./mocks/Mocks.sol";
import {InflowAdapterLib} from "../contracts/InflowAdapterLib.sol";

/// @notice Tests for adapter registration, allocation, and manual operations.
/// Corresponds to all 39 tests in vault/testing_adapters.rs.
contract InflowVaultAdaptersTest is InflowVaultBase {
    // ── helpers ───────────────────────────────────────────────────────────────

    function _newAdapter() internal returns (MockAdapterWithAsset) {
        return new MockAdapterWithAsset(address(asset));
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Registration
    // ═══════════════════════════════════════════════════════════════════════

    function test_register_adapter_success() public {
        MockAdapterWithAsset a = _newAdapter();

        vm.expectEmit(false, true, false, true, address(vault));
        emit InflowVault.AdapterRegistered("myAdapter", address(a), true, false);

        vm.prank(admin);
        vault.registerAdapter("myAdapter", address(a), true, false);

        InflowAdapterLib.AdapterInfo memory info = vault.getAdapterByName("myAdapter");
        assertEq(info.addr, address(a));
        assertTrue(info.automated);
        assertFalse(info.tracked);
        assertEq(info.name, "myAdapter");
    }

    function test_register_adapter_unauthorized() public {
        MockAdapterWithAsset a = _newAdapter();
        vm.prank(stranger);
        vm.expectRevert(InflowVault.Unauthorized.selector);
        vault.registerAdapter("a", address(a), true, false);
    }

    function test_register_adapter_duplicate_name_reverts() public {
        MockAdapterWithAsset a = _newAdapter();
        vm.prank(admin);
        vault.registerAdapter("dup", address(a), true, false);

        MockAdapterWithAsset b = _newAdapter();
        vm.prank(admin);
        vm.expectRevert(abi.encodeWithSelector(InflowAdapterLib.AdapterAlreadyExists.selector, "dup"));
        vault.registerAdapter("dup", address(b), false, false);
    }

    function test_register_adapter_zero_address_reverts() public {
        vm.prank(admin);
        vm.expectRevert(InflowVault.ZeroAddress.selector);
        vault.registerAdapter("zero", address(0), true, false);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Unregistration
    // ═══════════════════════════════════════════════════════════════════════

    function test_unregister_adapter_success() public {
        MockAdapterWithAsset a = _newAdapter();
        vm.prank(admin);
        vault.registerAdapter("toRemove", address(a), true, false);

        vm.expectEmit(false, true, false, true, address(vault));
        emit InflowVault.AdapterUnregistered("toRemove", address(a));

        vm.prank(admin);
        vault.unregisterAdapter("toRemove");

        InflowAdapterLib.AdapterInfo[] memory adapters = vault.getAdapters();
        assertEq(adapters.length, 0, "adapter list empty after removal");
    }

    function test_unregister_adapter_not_found_reverts() public {
        vm.prank(admin);
        vm.expectRevert(abi.encodeWithSelector(InflowAdapterLib.AdapterNotFound.selector, "ghost"));
        vault.unregisterAdapter("ghost");
    }

    function test_unregister_adapter_unauthorized() public {
        MockAdapterWithAsset a = _newAdapter();
        vm.prank(admin);
        vault.registerAdapter("a", address(a), true, false);

        vm.prank(stranger);
        vm.expectRevert(InflowVault.Unauthorized.selector);
        vault.unregisterAdapter("a");
    }

    function test_unregister_funded_untracked_adapter_reverts() public {
        MockAdapterWithAsset a = _newAdapter();
        a.setDepositCap(address(vault), 50_000e6);
        _registerAdapter("a", a, false, false); // untracked

        _deposit(user, 50_000e6);
        vm.prank(admin);
        vault.depositToAdapter("a", 50_000e6); // adapter holds 50k

        vm.prank(admin);
        vm.expectRevert(abi.encodeWithSelector(InflowAdapterLib.AdapterPositionNotEmpty.selector, "a"));
        vault.unregisterAdapter("a");

        // After withdrawing, totalAssets() still matches recoverable assets.
        vm.prank(admin);
        vault.withdrawFromAdapter("a", 50_000e6);
        assertEq(vault.totalAssets(), 50_000e6, "totalAssets matches recoverable assets");

        // Now unregistration succeeds.
        vm.prank(admin);
        vault.unregisterAdapter("a");
        assertEq(vault.getAdapters().length, 0);
    }

    function test_unregister_funded_tracked_adapter_reverts() public {
        MockAdapterWithAsset a = _newAdapter();
        a.setDepositCap(address(vault), 50_000e6);
        _registerAdapter("a", a, false, true); // tracked

        _deposit(user, 50_000e6);
        vm.prank(admin);
        vault.depositToAdapter("a", 50_000e6); // deployedAmount += 50k

        vm.prank(admin);
        vm.expectRevert(abi.encodeWithSelector(InflowAdapterLib.AdapterPositionNotEmpty.selector, "a"));
        vault.unregisterAdapter("a");

        // After withdrawing, deployedAmount is decremented and totalAssets() still matches.
        vm.prank(admin);
        vault.withdrawFromAdapter("a", 50_000e6);
        assertEq(vault.deployedAmount(), 0, "deployedAmount zeroed after withdraw");
        assertEq(vault.totalAssets(), 50_000e6, "totalAssets matches recoverable assets");

        // Now unregistration succeeds.
        vm.prank(admin);
        vault.unregisterAdapter("a");
        assertEq(vault.getAdapters().length, 0);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Allocation mode
    // ═══════════════════════════════════════════════════════════════════════

    function test_set_adapter_allocation_mode_to_manual() public {
        MockAdapterWithAsset a = _newAdapter();
        vm.prank(admin);
        vault.registerAdapter("a", address(a), true, false);

        vm.expectEmit(false, false, false, true, address(vault));
        emit InflowVault.AdapterAllocationModeUpdated("a", false);

        vm.prank(admin);
        vault.setAdapterAllocationMode("a", false);

        assertFalse(vault.getAdapterByName("a").automated);
    }

    function test_set_adapter_allocation_mode_not_found_reverts() public {
        vm.prank(admin);
        vm.expectRevert(abi.encodeWithSelector(InflowAdapterLib.AdapterNotFound.selector, "ghost"));
        vault.setAdapterAllocationMode("ghost", true);
    }

    function test_set_adapter_allocation_mode_unauthorized() public {
        MockAdapterWithAsset a = _newAdapter();
        vm.prank(admin);
        vault.registerAdapter("a", address(a), true, false);

        vm.prank(stranger);
        vm.expectRevert(InflowVault.Unauthorized.selector);
        vault.setAdapterAllocationMode("a", false);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Deployment tracking
    // ═══════════════════════════════════════════════════════════════════════

    function test_set_adapter_deployment_tracking() public {
        MockAdapterWithAsset a = _newAdapter();
        vm.prank(admin);
        vault.registerAdapter("a", address(a), false, false);

        vm.expectEmit(false, false, false, true, address(vault));
        emit InflowVault.AdapterDeploymentTrackingUpdated("a", true);

        vm.prank(admin);
        vault.setAdapterDeploymentTracking("a", true);

        assertTrue(vault.getAdapterByName("a").tracked);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Queries
    // ═══════════════════════════════════════════════════════════════════════

    function test_get_adapters_empty() public view {
        assertEq(vault.getAdapters().length, 0);
    }

    function test_get_adapters_with_entries() public {
        MockAdapterWithAsset a1 = _newAdapter();
        MockAdapterWithAsset a2 = _newAdapter();
        vm.prank(admin);
        vault.registerAdapter("first", address(a1), true, false);
        vm.prank(admin);
        vault.registerAdapter("second", address(a2), false, true);

        InflowAdapterLib.AdapterInfo[] memory infos = vault.getAdapters();
        assertEq(infos.length, 2);
        assertEq(infos[0].name, "first");
        assertEq(infos[1].name, "second");
    }

    function test_get_adapter_by_name_success() public {
        MockAdapterWithAsset a = _newAdapter();
        vm.prank(admin);
        vault.registerAdapter("named", address(a), true, true);

        InflowAdapterLib.AdapterInfo memory info = vault.getAdapterByName("named");
        assertEq(info.addr, address(a));
        assertTrue(info.automated);
        assertTrue(info.tracked);
    }

    function test_get_adapter_by_name_not_found_reverts() public {
        vm.expectRevert(abi.encodeWithSelector(InflowAdapterLib.AdapterNotFound.selector, "missing"));
        vault.getAdapterByName("missing");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Auto-allocation on deposit
    // ═══════════════════════════════════════════════════════════════════════

    function test_deposit_auto_allocates_to_automated_tracked_adapter() public {
        MockAdapterWithAsset a = _newAdapter();
        a.setDepositCap(address(vault), 100_000e6);
        _registerAdapter("a", a, true, true); // tracked

        uint256 deployedBefore = vault.deployedAmount();
        _deposit(user, 100_000e6);

        assertEq(asset.balanceOf(address(a)), 100_000e6, "adapter received funds");
        assertEq(vault.deployedAmount(), deployedBefore + 100_000e6, "tracked: deployedAmount incremented");
    }

    function test_deposit_auto_allocates_to_automated_untracked_adapter() public {
        MockAdapterWithAsset a = _newAdapter();
        a.setDepositCap(address(vault), 100_000e6);
        _registerAdapter("a", a, true, false); // untracked

        _deposit(user, 100_000e6);

        assertEq(asset.balanceOf(address(a)), 100_000e6);
        assertEq(vault.deployedAmount(), 0, "untracked: deployedAmount unchanged");
    }

    function test_deposit_skips_manual_adapter() public {
        MockAdapterWithAsset a = _newAdapter();
        a.setDepositCap(address(vault), 100_000e6);
        _registerAdapter("a", a, false, false); // manual

        _deposit(user, 100_000e6);

        assertEq(asset.balanceOf(address(a)), 0, "manual adapter not used on deposit");
        assertEq(asset.balanceOf(address(vault)), 100_000e6, "funds stay in vault");
    }

    function test_deposit_no_adapters_funds_stay_in_vault() public {
        _deposit(user, 100_000e6);
        assertEq(asset.balanceOf(address(vault)), 100_000e6);
    }

    function test_deposit_splits_across_multiple_adapters() public {
        MockAdapterWithAsset a1 = _newAdapter();
        MockAdapterWithAsset a2 = _newAdapter();
        a1.setDepositCap(address(vault), 40_000e6);
        a2.setDepositCap(address(vault), 40_000e6);
        _registerAdapter("a1", a1, true, false);
        _registerAdapter("a2", a2, true, false);

        _deposit(user, 100_000e6);

        assertEq(asset.balanceOf(address(a1)), 40_000e6, "a1 filled to capacity");
        assertEq(asset.balanceOf(address(a2)), 40_000e6, "a2 filled to capacity");
        assertEq(asset.balanceOf(address(vault)), 20_000e6, "20k stays in vault");
    }

    function test_deposit_skips_reverting_adapter_and_uses_healthy_one() public {
        RevertingAvailabilityAdapter bad = new RevertingAvailabilityAdapter(address(asset));
        MockAdapterWithAsset good = _newAdapter();
        good.setDepositCap(address(vault), 100_000e6);

        // bad is registered first; it must not block the vault.
        vm.prank(admin);
        vault.registerAdapter("bad", address(bad), true, false);
        _registerAdapter("good", good, true, false);

        _deposit(user, 100_000e6);

        assertEq(asset.balanceOf(address(bad)), 0, "reverting adapter skipped");
        assertEq(asset.balanceOf(address(good)), 100_000e6, "healthy adapter received funds");
    }

    function test_withdraw_skips_reverting_adapter_and_queues_remainder() public {
        RevertingAvailabilityAdapter bad = new RevertingAvailabilityAdapter(address(asset));
        MockAdapterWithAsset good = _newAdapter();
        good.setWithdrawCap(address(vault), 0); // healthy adapter has nothing available either

        vm.prank(admin);
        vault.registerAdapter("bad", address(bad), true, false);
        _registerAdapter("good", good, true, false);

        _deposit(user, 100_000e6);
        vm.prank(admin);
        vault.withdrawForDeployment(100_000e6); // drain vault so free balance = 0

        // Neither adapter can cover the redemption; should queue rather than revert.
        vm.prank(user);
        vault.redeem(100_000e6, user, user);

        assertEq(vault.nextWithdrawalId(), 1, "withdrawal queued instead of reverting");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Auto-withdrawal on redeem
    // ═══════════════════════════════════════════════════════════════════════

    function test_redeem_auto_withdraws_from_automated_adapter() public {
        MockAdapterWithAsset a = _newAdapter();
        a.setDepositCap(address(vault), 100_000e6);
        a.setWithdrawCap(address(vault), 100_000e6);
        _registerAdapter("a", a, true, true);

        _deposit(user, 100_000e6); // 100k -> adapter

        uint256 userBefore = asset.balanceOf(user);
        vm.prank(user);
        uint256 out = vault.redeem(100_000e6, user, user); // adapter covers it -> immediate

        assertEq(out, 100_000e6, "immediate fulfilment");
        assertEq(asset.balanceOf(user), userBefore + 100_000e6);
        assertEq(vault.nextWithdrawalId(), 0, "nothing queued");
    }

    function test_redeem_queues_when_adapters_insufficient() public {
        MockAdapterWithAsset a = _newAdapter();
        a.setWithdrawCap(address(vault), 20_000e6); // only 20k available
        _registerAdapter("a", a, true, false);

        _deposit(user, 100_000e6);
        vm.prank(admin);
        vault.withdrawForDeployment(100_000e6); // drain vault

        vm.prank(user);
        vault.redeem(100_000e6, user, user); // adapter can only provide 20k < 100k -> queue

        assertEq(vault.nextWithdrawalId(), 1, "queued");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Manual withdrawFromAdapter
    // ═══════════════════════════════════════════════════════════════════════

    function test_withdraw_from_adapter_manual() public {
        MockAdapterWithAsset a = _newAdapter();
        a.setDepositCap(address(vault), 50_000e6);
        _registerAdapter("a", a, false, false); // manual, untracked

        // Manually send funds to adapter via depositToAdapter.
        _deposit(user, 50_000e6);
        vm.prank(admin);
        vault.depositToAdapter("a", 50_000e6);

        uint256 vaultBefore = asset.balanceOf(address(vault));
        vm.prank(admin);
        vault.withdrawFromAdapter("a", 50_000e6);

        assertEq(asset.balanceOf(address(vault)), vaultBefore + 50_000e6, "funds returned");
    }

    function test_withdraw_from_adapter_tracked_decrements_deployed_amount() public {
        MockAdapterWithAsset a = _newAdapter();
        a.setDepositCap(address(vault), 50_000e6);
        _registerAdapter("a", a, false, true); // tracked

        _deposit(user, 50_000e6);
        vm.prank(admin);
        vault.depositToAdapter("a", 50_000e6); // deployedAmount += 50k

        uint256 deployedBefore = vault.deployedAmount();
        vm.prank(admin);
        vault.withdrawFromAdapter("a", 50_000e6);

        assertEq(vault.deployedAmount(), deployedBefore - 50_000e6, "tracked: decremented");
    }

    function test_withdraw_from_adapter_untracked_no_deployed_amount_change() public {
        MockAdapterWithAsset a = _newAdapter();
        a.setDepositCap(address(vault), 50_000e6);
        _registerAdapter("a", a, false, false); // untracked

        _deposit(user, 50_000e6);
        vm.prank(admin);
        vault.depositToAdapter("a", 50_000e6);

        uint256 deployedBefore = vault.deployedAmount();
        vm.prank(admin);
        vault.withdrawFromAdapter("a", 50_000e6);

        assertEq(vault.deployedAmount(), deployedBefore, "untracked: deployedAmount unchanged");
    }

    function test_withdraw_from_adapter_unauthorized() public {
        MockAdapterWithAsset a = _newAdapter();
        vm.prank(admin);
        vault.registerAdapter("a", address(a), false, false);

        vm.prank(stranger);
        vm.expectRevert(InflowVault.Unauthorized.selector);
        vault.withdrawFromAdapter("a", 1_000e6);
    }

    function test_withdraw_from_adapter_not_found_reverts() public {
        vm.prank(admin);
        vm.expectRevert(abi.encodeWithSelector(InflowAdapterLib.AdapterNotFound.selector, "ghost"));
        vault.withdrawFromAdapter("ghost", 1_000e6);
    }

    function test_withdraw_from_adapter_shortfall_reverts() public {
        uint256 shortfall = 1;
        ShortfallAdapter a = new ShortfallAdapter(address(asset), shortfall);
        vm.prank(admin);
        vault.registerAdapter("a", address(a), false, true);

        _deposit(user, 50_000e6);
        asset.mint(address(a), 50_000e6); // seed adapter directly

        vm.prank(admin);
        vm.expectRevert(
            abi.encodeWithSelector(
                InflowAdapterLib.AdapterWithdrawShortfall.selector, "a", 50_000e6, 50_000e6 - shortfall
            )
        );
        vault.withdrawFromAdapter("a", 50_000e6);
    }

    function test_move_adapter_funds_shortfall_reverts() public {
        uint256 shortfall = 1;
        ShortfallAdapter a1 = new ShortfallAdapter(address(asset), shortfall);
        MockAdapterWithAsset a2 = _newAdapter();
        vm.prank(admin);
        vault.registerAdapter("a1", address(a1), false, false);
        _registerAdapter("a2", a2, false, false);

        asset.mint(address(a1), 50_000e6); // seed adapter directly

        vm.prank(admin);
        vm.expectRevert(
            abi.encodeWithSelector(
                InflowAdapterLib.AdapterWithdrawShortfall.selector, "a1", 50_000e6, 50_000e6 - shortfall
            )
        );
        vault.moveAdapterFunds("a1", "a2", 50_000e6);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Manual depositToAdapter
    // ═══════════════════════════════════════════════════════════════════════

    function test_deposit_to_adapter_success() public {
        MockAdapterWithAsset a = _newAdapter();
        a.setDepositCap(address(vault), 50_000e6);
        _registerAdapter("a", a, false, true); // tracked

        _deposit(user, 50_000e6);

        uint256 deployedBefore = vault.deployedAmount();
        vm.prank(admin);
        vault.depositToAdapter("a", 50_000e6);

        assertEq(asset.balanceOf(address(a)), 50_000e6, "adapter received funds");
        assertEq(vault.deployedAmount(), deployedBefore + 50_000e6, "tracked: deployedAmount incremented");
    }

    function test_deposit_to_adapter_untracked_no_deployed_amount_change() public {
        MockAdapterWithAsset a = _newAdapter();
        _registerAdapter("a", a, false, false); // untracked

        _deposit(user, 50_000e6);

        uint256 deployedBefore = vault.deployedAmount();
        vm.prank(admin);
        vault.depositToAdapter("a", 50_000e6);

        assertEq(vault.deployedAmount(), deployedBefore, "untracked: no change");
    }

    function test_deposit_to_adapter_unauthorized() public {
        MockAdapterWithAsset a = _newAdapter();
        vm.prank(admin);
        vault.registerAdapter("a", address(a), false, false);

        vm.prank(stranger);
        vm.expectRevert(InflowVault.Unauthorized.selector);
        vault.depositToAdapter("a", 1_000e6);
    }

    function test_deposit_to_adapter_not_found_reverts() public {
        _deposit(user, 50_000e6);
        vm.prank(admin);
        vm.expectRevert(abi.encodeWithSelector(InflowAdapterLib.AdapterNotFound.selector, "ghost"));
        vault.depositToAdapter("ghost", 50_000e6);
    }

    function test_deposit_to_adapter_works_regardless_of_allocation_mode() public {
        MockAdapterWithAsset a = _newAdapter();
        a.setDepositCap(address(vault), 50_000e6);
        _registerAdapter("a", a, false, false); // manual (not automated)

        _deposit(user, 50_000e6);
        vm.prank(admin);
        vault.depositToAdapter("a", 50_000e6); // explicit call works even in manual mode

        assertEq(asset.balanceOf(address(a)), 50_000e6);
    }

    function test_deposit_to_adapter_reverts_when_amount_exceeds_vault_balance() public {
        MockAdapterWithAsset a = _newAdapter();
        _registerAdapter("a", a, false, true);

        _deposit(user, 30_000e6); // vault holds 30k

        vm.prank(admin);
        vm.expectRevert(abi.encodeWithSelector(InflowVault.InsufficientAvailableBalance.selector, 30_000e6, 50_000e6));
        vault.depositToAdapter("a", 50_000e6); // 50k > 30k available
    }

    function test_deposit_to_adapter_reverts_when_withdrawal_reserves_reduce_available_amount() public {
        MockAdapterWithAsset a = _newAdapter();
        _registerAdapter("a", a, false, true);

        // Deposit 50k so vault holds 50k.
        _deposit(user, 50_000e6);

        // Queue a withdrawal for 40k: totalWithdrawalAmount becomes 40k,
        // so availableForDeployment = 50k - 40k = 10k.
        vm.prank(user);
        vault.redeem(40_000e6, user, user); // queued, not immediately funded

        // Requesting 20k exceeds the 10k available even though raw balance is 50k.
        vm.prank(admin);
        vm.expectRevert(abi.encodeWithSelector(InflowVault.InsufficientAvailableBalance.selector, 10_000e6, 20_000e6));
        vault.depositToAdapter("a", 20_000e6);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // moveAdapterFunds
    // ═══════════════════════════════════════════════════════════════════════

    function test_move_adapter_funds_tracked_to_tracked() public {
        MockAdapterWithAsset a1 = _newAdapter();
        MockAdapterWithAsset a2 = _newAdapter();
        a1.setDepositCap(address(vault), 50_000e6);
        _registerAdapter("a1", a1, false, true);
        _registerAdapter("a2", a2, false, true);

        _deposit(user, 50_000e6);
        vm.prank(admin);
        vault.depositToAdapter("a1", 50_000e6);

        uint256 deployedBefore = vault.deployedAmount();
        vm.prank(admin);
        vault.moveAdapterFunds("a1", "a2", 50_000e6);

        assertEq(vault.deployedAmount(), deployedBefore, "tracked-to-tracked: deployedAmount unchanged");
        assertEq(asset.balanceOf(address(a1)), 0, "a1 drained");
        assertEq(asset.balanceOf(address(a2)), 50_000e6, "a2 received");
    }

    function test_move_adapter_funds_tracked_to_untracked() public {
        MockAdapterWithAsset a1 = _newAdapter();
        MockAdapterWithAsset a2 = _newAdapter();
        a1.setDepositCap(address(vault), 50_000e6);
        _registerAdapter("a1", a1, false, true);
        _registerAdapter("a2", a2, false, false); // untracked

        _deposit(user, 50_000e6);
        vm.prank(admin);
        vault.depositToAdapter("a1", 50_000e6);

        uint256 deployedBefore = vault.deployedAmount();
        vm.prank(admin);
        vault.moveAdapterFunds("a1", "a2", 50_000e6);

        assertEq(vault.deployedAmount(), deployedBefore - 50_000e6, "tracked-to-untracked: decremented");
    }

    function test_move_adapter_funds_untracked_to_tracked() public {
        MockAdapterWithAsset a1 = _newAdapter();
        MockAdapterWithAsset a2 = _newAdapter();
        a1.setDepositCap(address(vault), 50_000e6);
        _registerAdapter("a1", a1, false, false); // untracked
        _registerAdapter("a2", a2, false, true); // tracked

        _deposit(user, 50_000e6);
        vm.prank(admin);
        vault.depositToAdapter("a1", 50_000e6);

        uint256 deployedBefore = vault.deployedAmount();
        vm.prank(admin);
        vault.moveAdapterFunds("a1", "a2", 50_000e6);

        assertEq(vault.deployedAmount(), deployedBefore + 50_000e6, "untracked-to-tracked: incremented");
    }

    function test_move_adapter_funds_unauthorized() public {
        MockAdapterWithAsset a1 = _newAdapter();
        MockAdapterWithAsset a2 = _newAdapter();
        vm.prank(admin);
        vault.registerAdapter("a1", address(a1), false, false);
        vm.prank(admin);
        vault.registerAdapter("a2", address(a2), false, false);

        vm.prank(stranger);
        vm.expectRevert(InflowVault.Unauthorized.selector);
        vault.moveAdapterFunds("a1", "a2", 1_000e6);
    }

    function test_move_adapter_funds_from_not_found_reverts() public {
        MockAdapterWithAsset a2 = _newAdapter();
        vm.prank(admin);
        vault.registerAdapter("a2", address(a2), false, false);

        vm.prank(admin);
        vm.expectRevert(abi.encodeWithSelector(InflowAdapterLib.AdapterNotFound.selector, "ghost"));
        vault.moveAdapterFunds("ghost", "a2", 1_000e6);
    }

    function test_move_adapter_funds_to_not_found_reverts() public {
        MockAdapterWithAsset a1 = _newAdapter();
        vm.prank(admin);
        vault.registerAdapter("a1", address(a1), false, false);

        vm.prank(admin);
        vm.expectRevert(abi.encodeWithSelector(InflowAdapterLib.AdapterNotFound.selector, "ghost"));
        vault.moveAdapterFunds("a1", "ghost", 1_000e6);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // totalAssets integration
    // ═══════════════════════════════════════════════════════════════════════

    function test_untracked_adapter_counted_in_total_assets() public {
        MockAdapterWithAsset a = _newAdapter();
        _registerAdapter("a", a, false, false); // untracked

        _deposit(user, 100_000e6);
        vm.prank(admin);
        vault.depositToAdapter("a", 100_000e6); // funds in untracked adapter

        // totalAssets should include untracked position via queryUntrackedPositions.
        assertEq(vault.totalAssets(), 100_000e6, "untracked position included in totalAssets");
    }

    function test_tracked_adapter_not_double_counted_in_total_assets() public {
        MockAdapterWithAsset a = _newAdapter();
        _registerAdapter("a", a, false, true); // tracked

        _deposit(user, 100_000e6);
        vm.prank(admin);
        vault.depositToAdapter("a", 100_000e6); // deployedAmount += 100k

        // Tracked adapters: deployedAmount already counts them.
        // queryUntrackedPositions skips tracked adapters -> no double count.
        assertEq(vault.totalAssets(), 100_000e6, "tracked position counted exactly once");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Full round-trip (deposit → adapter → redeem)
    // ═══════════════════════════════════════════════════════════════════════

    function test_full_roundtrip_depositAndRedeem() public {
        MockAdapterWithAsset a = _newAdapter();
        a.setDepositCap(address(vault), 100_000e6);
        a.setWithdrawCap(address(vault), 100_000e6);
        _registerAdapter("basic", a, true, false); // automated, untracked

        _mintAndApprove(user, 100_000e6);
        vm.prank(user);
        uint256 sharesReceived = vault.deposit(100_000e6, user);

        assertEq(sharesReceived, 100_000e6, "1:1 shares minted");
        assertEq(asset.balanceOf(address(vault)), 0, "vault holds 0 - tokens routed to adapter");
        assertEq(asset.balanceOf(address(a)), 100_000e6, "adapter holds 100k");
        assertEq(vault.totalAssets(), 100_000e6, "totalAssets = 100k");

        vm.prank(user);
        uint256 assetsReturned = vault.redeem(sharesReceived, user, user);

        assertEq(assetsReturned, 100_000e6, "user receives 100k back");
        assertEq(asset.balanceOf(user), 100_000e6, "user balance restored");
        assertEq(asset.balanceOf(address(a)), 0, "adapter drained");
        assertEq(vault.totalSupply(), 0, "no shares outstanding");
        assertEq(vault.totalAssets(), 0);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // moveAdapterFundsToken
    // ═══════════════════════════════════════════════════════════════════════

    function test_moveAdapterFundsToken_nonDepositToken() public {
        MockERC20 usdt = new MockERC20("Tether USD", "USDT", 6);
        uint256 transferAmount = 50_000e6;

        MockAdapterWithAsset a1 = _newAdapter();
        MockAdapterWithAsset a2 = _newAdapter();
        _registerAdapter("adapterA", a1, false, false);
        _registerAdapter("adapterB", a2, false, false);

        usdt.mint(address(a1), transferAmount); // seed adapter A with USDT directly

        uint256 deployedBefore = vault.deployedAmount();

        vm.prank(admin);
        vault.moveAdapterFundsToken("adapterA", "adapterB", transferAmount, address(usdt));

        assertEq(usdt.balanceOf(address(a1)), 0, "adapterA: USDT drained");
        assertEq(usdt.balanceOf(address(a2)), transferAmount, "adapterB: USDT received");
        assertEq(vault.deployedAmount(), deployedBefore, "deployedAmount unchanged");
    }

    function test_moveAdapterFundsToken_trackingMismatch_reverts() public {
        MockAdapterWithAsset a1 = _newAdapter();
        MockAdapterWithAsset a2 = _newAdapter();
        _registerAdapter("trackedA", a1, false, true); // tracked
        _registerAdapter("untrackedB", a2, false, false); // untracked

        MockERC20 usdt = new MockERC20("Tether USD", "USDT", 6);

        vm.expectRevert(
            abi.encodeWithSelector(InflowAdapterLib.AdapterTrackingMismatch.selector, "trackedA", "untrackedB")
        );
        vm.prank(admin);
        vault.moveAdapterFundsToken("trackedA", "untrackedB", 1e6, address(usdt));
    }

    function test_moveAdapterFundsToken_depositAsset_reverts() public {
        MockAdapterWithAsset a1 = _newAdapter();
        MockAdapterWithAsset a2 = _newAdapter();
        _registerAdapter("adapterA", a1, false, false);
        _registerAdapter("adapterB", a2, false, false);

        vm.expectRevert(InflowVault.TokenIsDepositAsset.selector);
        vm.prank(admin);
        vault.moveAdapterFundsToken("adapterA", "adapterB", 1e6, address(asset));
    }
}
