// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {InflowVaultBase, InflowVault, MockAdapterWithAsset} from "./InflowVaultBase.t.sol";
import {IERC4626} from "@openzeppelin/contracts/interfaces/IERC4626.sol";
import {InflowWithdrawalQueueLib} from "../contracts/InflowWithdrawalQueueLib.sol";

/// @notice Tests for redeem() / withdraw() — immediate fulfilment and queued paths.
/// Corresponds to: withdrawal_test, withdraw_pays_on_behalf_recipient,
/// withdraw_queue_uses_on_behalf_withdrawer (vault/testing.rs).
contract InflowVaultWithdrawalTest is InflowVaultBase {

    // ── immediate fulfilment ──────────────────────────────────────────────────

    function test_redeem_immediate_when_funds_available() public {
        _deposit(user, 100_000e6);

        uint256 balanceBefore = asset.balanceOf(user);
        vm.prank(user);
        uint256 assetsOut = vault.redeem(100_000e6, user, user);

        assertEq(assetsOut,             100_000e6, "assets returned immediately");
        assertEq(vault.balanceOf(user), 0,         "shares burned");
        assertEq(asset.balanceOf(user), balanceBefore + 100_000e6);
        assertEq(vault.nextWithdrawalId(), 0, "no queue entry created");
    }

    function test_redeem_emits_withdraw_event_on_immediate_path() public {
        _deposit(user, 50_000e6);

        vm.expectEmit(true, true, true, true, address(vault));
        emit IERC4626.Withdraw(user, user, user, 50_000e6, 50_000e6);

        vm.prank(user);
        vault.redeem(50_000e6, user, user);
    }

    // ── queued withdrawal ─────────────────────────────────────────────────────

    function test_redeem_queues_when_insufficient_free_balance() public {
        _deposit(user, 100_000e6);
        vm.prank(admin);
        vault.withdrawForDeployment(100_000e6); // vault balance = 0

        uint256 withdrawalId = vault.nextWithdrawalId(); // == 0

        vm.expectEmit(true, true, true, true, address(vault));
        emit InflowVault.WithdrawalQueued(withdrawalId, user, user, 100_000e6, 100_000e6);

        uint256 balanceBefore = asset.balanceOf(user);
        vm.prank(user);
        vault.redeem(100_000e6, user, user);

        assertEq(vault.nextWithdrawalId(), 1,            "queue entry created");
        assertEq(vault.balanceOf(user),    0,            "shares burned immediately");
        assertEq(asset.balanceOf(user),    balanceBefore, "no assets transferred when queued");
    }

    function test_redeem_queues_returns_zero_assets() public {
        _deposit(user, 100_000e6);
        vm.prank(admin);
        vault.withdrawForDeployment(100_000e6);

        uint256 balanceBefore = asset.balanceOf(user);
        vm.prank(user);
        vault.redeem(100_000e6, user, user);
        // When queued, no assets are transferred immediately.
        assertEq(asset.balanceOf(user), balanceBefore, "no assets received when queued");
        assertEq(vault.nextWithdrawalId(), 1, "withdrawal was queued");
    }

    // ── queue entry fields ────────────────────────────────────────────────────

    function test_queue_entry_fields() public {
        _deposit(user, 100_000e6);
        vm.prank(admin);
        vault.withdrawForDeployment(100_000e6);

        uint256 id = vault.nextWithdrawalId();

        vm.prank(user);
        vault.redeem(100_000e6, alice, user); // receiver = alice, owner = user

        InflowWithdrawalQueueLib.WithdrawalEntry memory e = vault.withdrawalRequest(id);
        assertEq(e.id,              id);
        assertEq(e.owner,           user,        "owner is msg.sender");
        assertEq(e.receiver,        alice,       "receiver is alice");
        assertEq(e.sharesBurned,    100_000e6);
        assertEq(e.amountToReceive, 100_000e6);
        assertFalse(e.isFunded);
        assertGt(e.initiatedAt,     0,           "timestamp set");
    }

    function test_user_withdrawal_ids_tracked() public {
        _deposit(user, 200_000e6);
        vm.prank(admin);
        vault.withdrawForDeployment(200_000e6);

        vm.startPrank(user);
        vault.redeem(50_000e6, user, user);
        vault.redeem(50_000e6, user, user);
        vm.stopPrank();

        uint256[] memory ids = vault.getUserWithdrawalIds(user);
        assertEq(ids.length, 2);
        assertEq(ids[0], 0);
        assertEq(ids[1], 1);
    }

    // ── receiver / owner distinction ──────────────────────────────────────────

    function test_redeem_queued_with_different_receiver() public {
        _deposit(user, 100_000e6);
        vm.prank(admin);
        vault.withdrawForDeployment(100_000e6);

        uint256 id = vault.nextWithdrawalId();
        vm.prank(user);
        vault.redeem(100_000e6, alice, user); // assets will go to alice

        InflowWithdrawalQueueLib.WithdrawalEntry memory e = vault.withdrawalRequest(id);
        assertEq(e.receiver, alice);
        assertEq(e.owner,    user);
    }

    function test_redeem_immediate_with_different_receiver() public {
        _deposit(user, 100_000e6);

        uint256 aliceBefore = asset.balanceOf(alice);
        vm.prank(user);
        vault.redeem(100_000e6, alice, user);
        assertEq(asset.balanceOf(alice), aliceBefore + 100_000e6, "alice received assets");
        assertEq(vault.balanceOf(user),  0,                       "user shares burned");
    }

    // ── operator allowance ────────────────────────────────────────────────────

    function test_redeem_by_operator_with_allowance() public {
        _deposit(user, 100_000e6);

        // User grants alice an allowance for vault shares.
        vm.prank(user);
        vault.approve(alice, 100_000e6);

        vm.prank(alice);
        uint256 assetsOut = vault.redeem(100_000e6, alice, user);

        assertEq(assetsOut,              100_000e6);
        assertEq(vault.balanceOf(user),  0);
        assertEq(asset.balanceOf(alice), 100_000e6);
        assertEq(vault.allowance(user, alice), 0, "allowance fully spent");
    }

    // ── withdraw() by asset amount ────────────────────────────────────────────

    function test_withdraw_immediate_by_asset_amount() public {
        _deposit(user, 100_000e6);

        vm.prank(user);
        uint256 sharesUsed = vault.withdraw(100_000e6, user, user);
        assertEq(sharesUsed, 100_000e6, "1:1 price, shares == assets");
        assertEq(asset.balanceOf(user), 100_000e6);
    }

    function test_withdraw_queues_when_no_free_balance() public {
        _deposit(user, 100_000e6);
        vm.prank(admin);
        vault.withdrawForDeployment(100_000e6);

        uint256 id = vault.nextWithdrawalId();
        vm.prank(user);
        uint256 sharesUsed = vault.withdraw(100_000e6, user, user);

        // Returns shares burned (non-zero), assets transferred = 0.
        assertGt(sharesUsed, 0, "shares burned even when queued");
        InflowWithdrawalQueueLib.WithdrawalEntry memory e = vault.withdrawalRequest(id);
        assertFalse(e.isFunded);
    }

    // ── zero amount ───────────────────────────────────────────────────────────

    function test_redeem_zero_shares_reverts() public {
        vm.prank(user);
        vm.expectRevert(InflowVault.ZeroAmount.selector);
        vault.redeem(0, user, user);
    }

    // ── queue size limit ──────────────────────────────────────────────────────

    function test_max_withdrawals_per_user_reverts() public {
        // Need enough shares: deposit MAX_WITHDRAWALS + 1 times worth of amount
        uint256 perWithdrawal = 1_000e6;
        _deposit(user, perWithdrawal * (MAX_WITHDRAWALS + 1));
        vm.prank(admin);
        vault.withdrawForDeployment(perWithdrawal * (MAX_WITHDRAWALS + 1));

        vm.startPrank(user);
        for (uint256 i = 0; i < MAX_WITHDRAWALS; i++) {
            vault.redeem(perWithdrawal, user, user);
        }

        vm.expectRevert(InflowWithdrawalQueueLib.MaxWithdrawalsReached.selector);
        vault.redeem(perWithdrawal, user, user);
        vm.stopPrank();
    }

    function test_multiple_queued_withdrawals_below_limit() public {
        uint256 perWithdrawal = 1_000e6;
        _deposit(user, perWithdrawal * MAX_WITHDRAWALS);
        vm.prank(admin);
        vault.withdrawForDeployment(perWithdrawal * MAX_WITHDRAWALS);

        vm.startPrank(user);
        for (uint256 i = 0; i < MAX_WITHDRAWALS; i++) {
            vault.redeem(perWithdrawal, user, user);
        }
        vm.stopPrank();

        assertEq(vault.getUserWithdrawalIds(user).length, MAX_WITHDRAWALS);
    }

    // ── totalAssets accounts for pending withdrawal reserve ───────────────────

    function test_total_assets_subtracts_pending_withdrawal_reserve() public {
        _deposit(alice, 100_000e6);
        _deposit(bob,   100_000e6);

        vm.prank(admin);
        vault.withdrawForDeployment(200_000e6); // deployedAmount = 200k, vault balance = 0

        // Alice redeems -> queued. Total pending reserve = 100k.
        vm.prank(alice);
        vault.redeem(100_000e6, alice, alice);

        // totalAssets = deployedAmount - pendingReserve = 200k - 100k = 100k (Bob's share).
        assertEq(vault.totalAssets(), 100_000e6, "pending reserve subtracted from totalAssets");
    }

    // ── availableForDeployment respects queue reserve ─────────────────────────

    function test_funded_reserve_blocks_deployment() public {
        // Deploy all user funds so vault balance = 0.
        _deposit(user, 100_000e6);
        vm.prank(admin);
        vault.withdrawForDeployment(100_000e6); // vaultBalance = 0, deployedAmount = 100k

        // Return 50k from deployment (no yield — 1:1 price preserved).
        asset.mint(admin, 50_000e6);
        vm.prank(admin);
        asset.approve(address(vault), 50_000e6);
        vm.prank(admin);
        vault.depositFromDeployment(50_000e6); // vaultBalance = 50k, deployedAmount = 50k

        // User redeems 100k shares. previewRedeem = 100k (still 1:1).
        // Vault has 50k < 100k -> queued. Reserve = 100k.
        vm.prank(user);
        vault.redeem(100_000e6, user, user);

        // available = max(0, 50k balance - 100k reserve) = 0.
        assertEq(vault.availableForDeployment(), 0, "queue reserve blocks all available funds");
    }

    // ── available-for-deployment respects queue reserve (no yield) ───────────

    function test_redeem_partial_from_adapter_then_immediate() public {
        MockAdapterWithAsset adapter = new MockAdapterWithAsset(address(asset));
        // Vault has 20k, adapter has 80k capacity.
        _deposit(user, 100_000e6);
        adapter.setDepositCap(address(vault), 80_000e6);
        adapter.setWithdrawCap(address(vault), 80_000e6);
        _registerAdapter("adapter1", adapter, true, false);

        // Trigger adapter allocation: deposit so 80k goes to adapter.
        _deposit(alice, 80_000e6); // 80k -> adapter (auto), 20k stays in vault

        // alice redeems 80k -> adapter covers it entirely -> immediate.
        vm.prank(alice);
        uint256 out = vault.redeem(80_000e6, alice, alice);
        assertEq(out, 80_000e6, "immediate: adapter + vault covered withdrawal");
        assertEq(vault.nextWithdrawalId(), 0, "nothing queued");
    }

    function test_redeem_queues_when_adapters_still_insufficient() public {
        MockAdapterWithAsset adapter = new MockAdapterWithAsset(address(asset));
        adapter.setWithdrawCap(address(vault), 30_000e6); // only 30k available

        _deposit(user, 100_000e6);
        // Drain vault.
        vm.prank(admin);
        vault.withdrawForDeployment(100_000e6);
        _registerAdapter("adapter1", adapter, true, false);

        // User tries to redeem 100k but adapter only has 30k -> queue.
        vm.prank(user);
        vault.redeem(100_000e6, user, user);

        assertEq(vault.nextWithdrawalId(), 1, "one entry queued");
    }
}
