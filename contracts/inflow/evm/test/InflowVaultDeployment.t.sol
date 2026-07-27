// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {InflowVaultBase, InflowVault, MockERC20} from "./InflowVaultBase.t.sol";

/// @notice Tests for withdrawForDeployment(), depositFromDeployment(),
/// submitDeployedAmount(), and availableForDeployment().
/// Corresponds to: deposit_from_deployment_test, submit_deployed_amount_test,
/// deposit_withdrawal_for_deployment_test (vault/testing.rs + control-center/testing.rs).
contract InflowVaultDeploymentTest is InflowVaultBase {
    // ── withdrawForDeployment ─────────────────────────────────────────────────

    function test_withdraw_for_deployment_transfers_and_tracks() public {
        _deposit(user, 100_000e6);

        uint256 adminBefore = asset.balanceOf(admin);
        vm.prank(admin);
        vault.withdrawForDeployment(100_000e6);

        assertEq(asset.balanceOf(admin), adminBefore + 100_000e6, "admin received assets");
        assertEq(asset.balanceOf(address(vault)), 0, "vault balance drained");
        assertEq(vault.deployedAmount(), 100_000e6, "deployedAmount incremented");
    }

    function test_withdraw_for_deployment_capped_at_available() public {
        _deposit(user, 60_000e6);

        // Request more than available — vault silently caps at 60k.
        vm.prank(user);
        vault.redeem(30_000e6, user, user); // queues (no free balance after next step)

        // Actually: no withdrawal has been queued yet. Let's drain first then request too much.
        // Re-set: fresh vault scenario.
        vault = _deployVault(0, address(0));
        asset = new MockERC20("USD Coin", "USDC", 6);
        vault = _deployVault(0, address(0));

        _deposit(user, 60_000e6);

        uint256 adminBefore = asset.balanceOf(admin);
        vm.prank(admin);
        vault.withdrawForDeployment(200_000e6); // request 200k but only 60k available

        assertEq(vault.deployedAmount(), 60_000e6, "only available amount deployed");
        assertEq(asset.balanceOf(admin), adminBefore + 60_000e6, "admin got 60k");
        assertEq(asset.balanceOf(address(vault)), 0);
    }

    function test_withdraw_for_deployment_zero_amount_reverts() public {
        vm.prank(admin);
        vm.expectRevert(InflowVault.ZeroAmount.selector);
        vault.withdrawForDeployment(0);
    }

    function test_withdraw_for_deployment_zero_available_reverts() public {
        // No deposits — nothing available.
        vm.prank(admin);
        vm.expectRevert(InflowVault.ZeroAmount.selector);
        vault.withdrawForDeployment(100_000e6);
    }

    function test_withdraw_for_deployment_zero_available_due_to_queue() public {
        _deposit(user, 100_000e6);

        // Queue all 100k for withdrawal first.
        vm.prank(admin);
        vault.withdrawForDeployment(100_000e6); // drain

        _deposit(bob, 100_000e6);

        // Bob redeems -> queued; queue reserve = 100k = full balance.
        vm.prank(bob);
        vault.redeem(100_000e6, bob, bob);

        assertEq(vault.availableForDeployment(), 0, "queue reserve blocks all funds");

        vm.prank(admin);
        vm.expectRevert(InflowVault.ZeroAmount.selector);
        vault.withdrawForDeployment(100_000e6);
    }

    function test_withdraw_for_deployment_unauthorized() public {
        _deposit(user, 100_000e6);
        vm.prank(stranger);
        vm.expectRevert(InflowVault.Unauthorized.selector);
        vault.withdrawForDeployment(100_000e6);
    }

    function test_withdraw_for_deployment_emits_event() public {
        _deposit(user, 60_000e6);

        vm.expectEmit(true, false, false, true, address(vault));
        emit InflowVault.WithdrawForDeployment(admin, 60_000e6, 60_000e6);

        vm.prank(admin);
        vault.withdrawForDeployment(60_000e6);
    }

    function test_withdraw_for_deployment_emits_capped_amount_in_event() public {
        _deposit(user, 60_000e6);

        // requested = 200k, withdrawn = 60k (capped).
        vm.expectEmit(true, false, false, true, address(vault));
        emit InflowVault.WithdrawForDeployment(admin, 200_000e6, 60_000e6);

        vm.prank(admin);
        vault.withdrawForDeployment(200_000e6);
    }

    // ── depositFromDeployment ─────────────────────────────────────────────────

    function test_deposit_from_deployment_receives_and_tracks() public {
        _deposit(user, 100_000e6);
        vm.prank(admin);
        vault.withdrawForDeployment(100_000e6); // deployedAmount = 100k

        // Return 110k (principal + yield).
        asset.mint(admin, 110_000e6);
        vm.prank(admin);
        asset.approve(address(vault), 110_000e6);

        vm.prank(admin);
        vault.depositFromDeployment(110_000e6);

        assertEq(asset.balanceOf(address(vault)), 110_000e6, "vault received 110k");
        assertEq(vault.deployedAmount(), 0, "deployedAmount decreased (clamped to 0)");
    }

    function test_deposit_from_deployment_exact_amount() public {
        _deposit(user, 100_000e6);
        vm.prank(admin);
        vault.withdrawForDeployment(100_000e6);

        asset.mint(admin, 100_000e6);
        vm.prank(admin);
        asset.approve(address(vault), 100_000e6);

        vm.prank(admin);
        vault.depositFromDeployment(100_000e6);

        assertEq(vault.deployedAmount(), 0);
        assertEq(asset.balanceOf(address(vault)), 100_000e6);
    }

    function test_deposit_from_deployment_decrements_to_zero_not_underflow() public {
        _deposit(user, 50_000e6);
        vm.prank(admin);
        vault.withdrawForDeployment(50_000e6); // deployedAmount = 50k

        // Return 200k (more than was deployed).
        asset.mint(admin, 200_000e6);
        vm.prank(admin);
        asset.approve(address(vault), 200_000e6);

        vm.prank(admin);
        vault.depositFromDeployment(200_000e6);

        assertEq(vault.deployedAmount(), 0, "clamped to 0, no underflow");
    }

    function test_deposit_from_deployment_zero_amount_reverts() public {
        vm.prank(admin);
        vm.expectRevert(InflowVault.ZeroAmount.selector);
        vault.depositFromDeployment(0);
    }

    function test_deposit_from_deployment_unauthorized() public {
        vm.prank(stranger);
        vm.expectRevert(InflowVault.Unauthorized.selector);
        vault.depositFromDeployment(100_000e6);
    }

    function test_deposit_from_deployment_emits_event() public {
        _deposit(user, 100_000e6);
        vm.prank(admin);
        vault.withdrawForDeployment(100_000e6);

        asset.mint(admin, 100_000e6);
        vm.prank(admin);
        asset.approve(address(vault), 100_000e6);

        vm.expectEmit(true, false, false, true, address(vault));
        emit InflowVault.DepositFromDeployment(admin, 100_000e6);

        vm.prank(admin);
        vault.depositFromDeployment(100_000e6);
    }

    // ── submitDeployedAmount ──────────────────────────────────────────────────

    function test_submit_deployed_amount_updates_value() public {
        _deposit(user, 100_000e6);
        vm.prank(admin);
        vault.withdrawForDeployment(100_000e6);

        vm.prank(admin);
        vault.submitDeployedAmount(150_000e6);

        assertEq(vault.deployedAmount(), 150_000e6);
    }

    function test_submit_deployed_amount_unauthorized_for_main_whitelist_only() public {
        // `stranger` is on neither whitelist.
        vm.prank(stranger);
        vm.expectRevert(InflowVault.Unauthorized.selector);
        vault.submitDeployedAmount(100_000e6);
    }

    function test_submit_deployed_amount_requires_deployed_amount_whitelist() public {
        // Add `user` to the primary whitelist, but NOT to the deployed amount whitelist.
        vm.prank(admin);
        vault.addToWhitelist(user);

        vm.prank(user);
        vm.expectRevert(InflowVault.Unauthorized.selector);
        vault.submitDeployedAmount(100_000e6);
    }

    /// submitDeployedAmount() sets deployedAmount first, then calls accrueFees().
    /// Fee accrual therefore observes the NEW share price (after the deployedAmount update).
    function test_submit_deployed_amount_accrues_fees_at_new_price() public {
        vault = _deployVaultWithFees(WAD / 5); // 20% fee

        _deposit(user, 100_000e6);
        vm.prank(admin);
        vault.withdrawForDeployment(100_000e6);
        // deployedAmount = 100k, totalAssets = 100k, supply = 100k, price = 1.0, HWM = 1.0

        // Submit 120k: deployedAmount becomes 120k first, then accrueFees() runs.
        // accrueFees() sees price = 1.2 (120k / 100k) > HWM = 1.0 -> mints fee shares.
        vm.prank(admin);
        vault.submitDeployedAmount(120_000e6);

        assertGt(vault.highWaterMarkPrice(), WAD, "HWM advanced to the new price");
        assertGt(vault.balanceOf(feeRecipient), 0, "fee shares minted at the new price");
        assertEq(vault.deployedAmount(), 120_000e6, "deployedAmount stored correctly");
    }

    // ── availableForDeployment ────────────────────────────────────────────────

    function test_available_for_deployment_no_queue() public {
        _deposit(user, 100_000e6);
        assertEq(vault.availableForDeployment(), 100_000e6);
    }

    function test_available_for_deployment_with_queue_reserve() public {
        _deposit(user, 100_000e6);
        _deposit(alice, 100_000e6);

        // Drain 100k via deployment.
        vm.prank(admin);
        vault.withdrawForDeployment(100_000e6);

        // Alice redeems -> queued; reserve = 100k; vault balance = 100k.
        vm.prank(alice);
        vault.redeem(100_000e6, alice, alice);

        // available = vault balance (100k) - reserve (100k) = 0.
        assertEq(vault.availableForDeployment(), 0, "queue reserve blocks deployment");
    }

    function test_available_for_deployment_partial_reserve() public {
        _deposit(user, 200_000e6);

        // Drain 100k.
        vm.prank(admin);
        vault.withdrawForDeployment(100_000e6);

        // Queue 50k withdrawal — reserve = 50k; vault balance = 100k.
        vm.prank(user);
        vault.redeem(50_000e6, user, user);

        // available = 100k - 50k = 50k.
        assertEq(vault.availableForDeployment(), 50_000e6);
    }

    function test_available_for_deployment_zero_when_reserve_exceeds_balance() public {
        _deposit(user, 100_000e6);
        vm.prank(admin);
        vault.withdrawForDeployment(100_000e6); // vault balance = 0, deployedAmount = 100k

        // User redeems — queued with reserve = 100k.
        vm.prank(user);
        vault.redeem(100_000e6, user, user);

        assertEq(vault.availableForDeployment(), 0, "clamped to 0");
    }
}
