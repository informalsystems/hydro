// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {InflowVaultBase} from "./InflowVaultBase.t.sol";
import {InflowWithdrawalQueueLib} from "../contracts/InflowWithdrawalQueueLib.sol";

/// @notice Tests for fulfillPendingWithdrawals() and claimUnbondedWithdrawals().
/// Corresponds to: fulfill_pending_withdrawals_test, claim_unbonded_withdrawals_test
/// (vault/testing.rs).
contract InflowVaultWithdrawalQueueTest is InflowVaultBase {
    // ── helpers ───────────────────────────────────────────────────────────────

    /// Queue `count` withdrawals of `amountEach` for `who` (vault must have no free balance).
    function _queueWithdrawals(address who, uint256 amountEach, uint256 count) internal returns (uint256[] memory ids) {
        ids = new uint256[](count);
        vm.startPrank(who);
        for (uint256 i = 0; i < count; i++) {
            ids[i] = vault.nextWithdrawalId();
            vault.redeem(amountEach, who, who);
        }
        vm.stopPrank();
    }

    /// Fund the vault with `amount` tokens (simulates deployment return).
    function _fundVault(uint256 amount) internal {
        asset.mint(admin, amount);
        vm.prank(admin);
        asset.approve(address(vault), amount);
        vm.prank(admin);
        vault.depositFromDeployment(amount);
    }

    // ── fulfillPendingWithdrawals ──────────────────────────────────────────────

    function test_fulfill_marks_entries_funded_fifo() public {
        _deposit(user, 300_000e6);
        vm.prank(admin);
        vault.withdrawForDeployment(300_000e6);

        vm.startPrank(user);
        vault.redeem(100_000e6, user, user); // id 0
        vault.redeem(100_000e6, user, user); // id 1
        vault.redeem(100_000e6, user, user); // id 2
        vm.stopPrank();

        // Return 200k — enough for ids 0 and 1 but not 2.
        _fundVault(200_000e6);
        vault.fulfillPendingWithdrawals(10);

        assertTrue(vault.withdrawalRequest(0).isFunded, "id 0 funded");
        assertTrue(vault.withdrawalRequest(1).isFunded, "id 1 funded");
        assertFalse(vault.withdrawalRequest(2).isFunded, "id 2 not funded (no balance)");
        assertEq(vault.lastFundedWithdrawalId(), 1);
        assertTrue(vault.anyWithdrawalFunded());
    }

    function test_fulfill_stops_at_first_unaffordable() public {
        _deposit(user, 300_000e6);
        vm.prank(admin);
        vault.withdrawForDeployment(300_000e6);

        vm.startPrank(user);
        vault.redeem(200_000e6, user, user); // id 0 — large, needs 200k
        vault.redeem(10_000e6, user, user); // id 1 — small
        vm.stopPrank();

        // Only 50k available — can't cover id 0 -> FIFO stops.
        _fundVault(50_000e6);
        vault.fulfillPendingWithdrawals(10);

        assertFalse(vault.withdrawalRequest(0).isFunded, "id 0 not funded");
        assertFalse(vault.withdrawalRequest(1).isFunded, "id 1 not funded (blocked by id 0)");
        assertFalse(vault.anyWithdrawalFunded());
    }

    function test_fulfill_respects_limit() public {
        _deposit(user, 300_000e6);
        vm.prank(admin);
        vault.withdrawForDeployment(300_000e6);

        vm.startPrank(user);
        vault.redeem(100_000e6, user, user); // 0
        vault.redeem(100_000e6, user, user); // 1
        vault.redeem(100_000e6, user, user); // 2
        vm.stopPrank();

        _fundVault(300_000e6);
        vault.fulfillPendingWithdrawals(2); // process at most 2

        assertTrue(vault.withdrawalRequest(0).isFunded);
        assertTrue(vault.withdrawalRequest(1).isFunded);
        assertFalse(vault.withdrawalRequest(2).isFunded, "limit=2 stops after 2");
    }

    function test_fulfill_no_op_when_queue_empty() public {
        vault.fulfillPendingWithdrawals(10); // should not revert
        assertFalse(vault.anyWithdrawalFunded());
    }

    function test_fulfill_skips_cancelled_entries() public {
        _deposit(user, 200_000e6);
        vm.prank(admin);
        vault.withdrawForDeployment(200_000e6);

        vm.startPrank(user);
        vault.redeem(100_000e6, user, user); // id 0
        vault.redeem(100_000e6, user, user); // id 1
        vm.stopPrank();

        // Cancel id 0.
        uint256[] memory cancelIds = new uint256[](1);
        cancelIds[0] = 0;
        vm.prank(user);
        vault.cancelWithdrawal(cancelIds);

        // Only 100k returned — enough for id 1 if id 0 is skipped.
        _fundVault(100_000e6);
        vault.fulfillPendingWithdrawals(10);

        assertTrue(vault.withdrawalRequest(1).isFunded, "id 1 funded");
        assertEq(vault.lastFundedWithdrawalId(), 1);
    }

    function test_fulfill_updates_non_funded_amount() public {
        _deposit(user, 200_000e6);
        vm.prank(admin);
        vault.withdrawForDeployment(200_000e6);

        vm.startPrank(user);
        vault.redeem(100_000e6, user, user); // 0
        vault.redeem(100_000e6, user, user); // 1
        vm.stopPrank();

        InflowWithdrawalQueueLib.WithdrawalQueueInfo memory before = vault.withdrawalQueueInfo();
        assertEq(before.nonFundedWithdrawalAmount, 200_000e6);

        _fundVault(100_000e6);
        vault.fulfillPendingWithdrawals(1);

        InflowWithdrawalQueueLib.WithdrawalQueueInfo memory after_ = vault.withdrawalQueueInfo();
        assertEq(after_.nonFundedWithdrawalAmount, 100_000e6, "100k funded, 100k still unfunded");
    }

    function test_fulfill_updates_last_funded_id() public {
        _deposit(user, 200_000e6);
        vm.prank(admin);
        vault.withdrawForDeployment(200_000e6);

        vm.startPrank(user);
        vault.redeem(100_000e6, user, user); // 0
        vault.redeem(100_000e6, user, user); // 1
        vm.stopPrank();

        _fundVault(200_000e6);
        vault.fulfillPendingWithdrawals(10);

        assertEq(vault.lastFundedWithdrawalId(), 1);
    }

    // ── claimUnbondedWithdrawals ───────────────────────────────────────────────

    function test_claim_transfers_assets_to_receiver() public {
        _deposit(user, 100_000e6);
        vm.prank(admin);
        vault.withdrawForDeployment(100_000e6);

        vm.prank(user);
        vault.redeem(100_000e6, alice, user); // receiver = alice

        _fundVault(100_000e6);
        vault.fulfillPendingWithdrawals(10);

        uint256 aliceBefore = asset.balanceOf(alice);
        uint256[] memory ids = new uint256[](1);
        ids[0] = 0;
        vault.claimUnbondedWithdrawals(ids);

        assertEq(asset.balanceOf(alice), aliceBefore + 100_000e6, "alice received assets");
    }

    function test_claim_multiple_ids() public {
        _deposit(user, 300_000e6);
        vm.prank(admin);
        vault.withdrawForDeployment(300_000e6);

        vm.startPrank(user);
        vault.redeem(100_000e6, user, user); // 0
        vault.redeem(100_000e6, user, user); // 1
        vault.redeem(100_000e6, user, user); // 2
        vm.stopPrank();

        _fundVault(300_000e6);
        vault.fulfillPendingWithdrawals(10);

        uint256 before = asset.balanceOf(user);
        uint256[] memory ids = new uint256[](3);
        ids[0] = 0;
        ids[1] = 1;
        ids[2] = 2;
        vault.claimUnbondedWithdrawals(ids);

        assertEq(asset.balanceOf(user), before + 300_000e6, "all three amounts received");
    }

    function test_claim_deduplicates_ids() public {
        _deposit(user, 100_000e6);
        vm.prank(admin);
        vault.withdrawForDeployment(100_000e6);
        vm.prank(user);
        vault.redeem(100_000e6, user, user); // id 0

        _fundVault(100_000e6);
        vault.fulfillPendingWithdrawals(10);

        uint256 before = asset.balanceOf(user);
        uint256[] memory ids = new uint256[](3);
        ids[0] = 0; // same id three times
        ids[1] = 0;
        ids[2] = 0;
        vault.claimUnbondedWithdrawals(ids);

        // Only one transfer — 100k received, not 300k.
        assertEq(asset.balanceOf(user), before + 100_000e6, "only claimed once");
    }

    function test_claim_skips_unfunded_ids() public {
        _deposit(user, 200_000e6);
        vm.prank(admin);
        vault.withdrawForDeployment(200_000e6);

        vm.startPrank(user);
        vault.redeem(100_000e6, user, user); // 0
        vault.redeem(100_000e6, user, user); // 1 — not funded
        vm.stopPrank();

        // Only fund id 0.
        _fundVault(100_000e6);
        vault.fulfillPendingWithdrawals(1);

        uint256 before = asset.balanceOf(user);
        uint256[] memory ids = new uint256[](2);
        ids[0] = 0;
        ids[1] = 1;
        vault.claimUnbondedWithdrawals(ids);

        assertEq(asset.balanceOf(user), before + 100_000e6, "only funded id claimed");
    }

    function test_claim_skips_ids_above_last_funded() public {
        _deposit(user, 200_000e6);
        vm.prank(admin);
        vault.withdrawForDeployment(200_000e6);

        vm.startPrank(user);
        vault.redeem(100_000e6, user, user); // 0
        vault.redeem(100_000e6, user, user); // 1
        vm.stopPrank();

        // Fund only id 0; lastFundedId = 0.
        _fundVault(100_000e6);
        vault.fulfillPendingWithdrawals(1);

        uint256 before = asset.balanceOf(user);
        uint256[] memory ids = new uint256[](1);
        ids[0] = 1; // above lastFundedId
        vault.claimUnbondedWithdrawals(ids);

        assertEq(asset.balanceOf(user), before, "nothing paid out");
    }

    function test_claim_reverts_nothing_funded_yet() public {
        _deposit(user, 100_000e6);
        vm.prank(admin);
        vault.withdrawForDeployment(100_000e6);
        vm.prank(user);
        vault.redeem(100_000e6, user, user);

        uint256[] memory ids = new uint256[](1);
        ids[0] = 0;
        vm.expectRevert(InflowWithdrawalQueueLib.NothingFundedYet.selector);
        vault.claimUnbondedWithdrawals(ids);
    }

    function test_claim_removes_user_id_from_list() public {
        _deposit(user, 200_000e6);
        vm.prank(admin);
        vault.withdrawForDeployment(200_000e6);

        vm.startPrank(user);
        vault.redeem(100_000e6, user, user); // 0
        vault.redeem(100_000e6, user, user); // 1
        vm.stopPrank();

        _fundVault(100_000e6);
        vault.fulfillPendingWithdrawals(1);

        uint256[] memory ids = new uint256[](1);
        ids[0] = 0;
        vault.claimUnbondedWithdrawals(ids);

        uint256[] memory remaining = vault.getUserWithdrawalIds(user);
        assertEq(remaining.length, 1, "one id remains");
        assertEq(remaining[0], 1);
    }

    function test_claim_updates_queue_totals() public {
        _deposit(user, 100_000e6);
        vm.prank(admin);
        vault.withdrawForDeployment(100_000e6);
        vm.prank(user);
        vault.redeem(100_000e6, user, user); // 0

        _fundVault(100_000e6);
        vault.fulfillPendingWithdrawals(10);

        InflowWithdrawalQueueLib.WithdrawalQueueInfo memory before = vault.withdrawalQueueInfo();
        assertEq(before.totalWithdrawalAmount, 100_000e6);
        assertEq(before.totalSharesBurned, 100_000e6);

        uint256[] memory ids = new uint256[](1);
        ids[0] = 0;
        vault.claimUnbondedWithdrawals(ids);

        InflowWithdrawalQueueLib.WithdrawalQueueInfo memory after_ = vault.withdrawalQueueInfo();
        assertEq(after_.totalWithdrawalAmount, 0);
        assertEq(after_.totalSharesBurned, 0);
    }

    // ── end-to-end ────────────────────────────────────────────────────────────

    function test_fulfill_then_claim_end_to_end() public {
        // Alice and Bob both deposit 100k.
        _deposit(alice, 100_000e6);
        _deposit(bob, 100_000e6);

        // Admin deploys all 200k.
        vm.prank(admin);
        vault.withdrawForDeployment(200_000e6);

        // Both redeem -> queued.
        vm.prank(alice);
        vault.redeem(100_000e6, alice, alice); // id 0

        vm.prank(bob);
        vault.redeem(100_000e6, bob, bob); // id 1

        assertEq(vault.totalAssets(), 0, "all assets pending");

        // Deployment returns 200k.
        _fundVault(200_000e6);

        // Fund the queue.
        vault.fulfillPendingWithdrawals(10);

        assertTrue(vault.withdrawalRequest(0).isFunded);
        assertTrue(vault.withdrawalRequest(1).isFunded);

        // Alice claims.
        uint256[] memory aliceIds = new uint256[](1);
        aliceIds[0] = 0;
        vault.claimUnbondedWithdrawals(aliceIds);
        assertEq(asset.balanceOf(alice), 100_000e6);

        // Bob claims.
        uint256[] memory bobIds = new uint256[](1);
        bobIds[0] = 1;
        vault.claimUnbondedWithdrawals(bobIds);
        assertEq(asset.balanceOf(bob), 100_000e6);

        // Queue fully cleared.
        assertEq(vault.withdrawalQueueInfo().totalWithdrawalAmount, 0);
        assertEq(vault.getUserWithdrawalIds(alice).length, 0);
        assertEq(vault.getUserWithdrawalIds(bob).length, 0);
    }
}
