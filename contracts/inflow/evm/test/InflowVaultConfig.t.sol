// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {InflowVaultBase, InflowVault} from "./InflowVaultBase.t.sol";
import {InflowWithdrawalQueueLib} from "../contracts/InflowWithdrawalQueueLib.sol";

/// @notice Tests for updateDepositCap() and updateMaxWithdrawalsPerUser().
/// Corresponds to: withdrawal_with_config_update_test (vault/testing.rs).
contract InflowVaultConfigTest is InflowVaultBase {
    // ── updateDepositCap ──────────────────────────────────────────────────────

    function test_update_deposit_cap_success() public {
        uint256 newCap = 500_000e6;

        vm.expectEmit(false, false, false, true, address(vault));
        emit InflowVault.DepositCapUpdated(newCap);

        vm.prank(admin);
        vault.updateDepositCap(newCap);

        assertEq(vault.depositCap(), newCap);
    }

    function test_update_deposit_cap_unauthorized() public {
        vm.prank(stranger);
        vm.expectRevert(InflowVault.Unauthorized.selector);
        vault.updateDepositCap(500_000e6);
    }

    function test_update_deposit_cap_blocks_deposit_above_new_cap() public {
        // Deposit 100k first.
        _deposit(alice, 100_000e6);

        // Lower cap to 150k.
        vm.prank(admin);
        vault.updateDepositCap(150_000e6);

        // Deposit exactly up to the new cap.
        _deposit(bob, 50_000e6); // 100k + 50k = 150k = cap -> succeeds

        // One wei more should revert.
        asset.mint(user, 1);
        vm.prank(user);
        asset.approve(address(vault), 1);
        vm.prank(user);
        vm.expectRevert();
        vault.deposit(1, user);
    }

    function test_update_deposit_cap_to_zero_blocks_all_deposits() public {
        vm.prank(admin);
        vault.updateDepositCap(0);

        asset.mint(user, 1);
        vm.prank(user);
        asset.approve(address(vault), 1);

        vm.prank(user);
        vm.expectRevert();
        vault.deposit(1, user);
    }

    // ── updateMaxWithdrawalsPerUser ───────────────────────────────────────────

    function test_update_max_withdrawals_per_user_success() public {
        uint256 newMax = 5;

        vm.expectEmit(false, false, false, true, address(vault));
        emit InflowVault.MaxWithdrawalsPerUserUpdated(newMax);

        vm.prank(admin);
        vault.updateMaxWithdrawalsPerUser(newMax);

        assertEq(vault.maxWithdrawalsPerUser(), newMax);
    }

    function test_update_max_withdrawals_per_user_unauthorized() public {
        vm.prank(stranger);
        vm.expectRevert(InflowVault.Unauthorized.selector);
        vault.updateMaxWithdrawalsPerUser(5);
    }

    function test_update_max_withdrawals_reduces_limit_for_new_requests() public {
        uint256 perWithdrawal = 10_000e6;
        // Deposit enough for 5 withdrawals.
        _deposit(user, perWithdrawal * 5);
        vm.prank(admin);
        vault.withdrawForDeployment(perWithdrawal * 5);

        // Queue 3 withdrawals.
        vm.startPrank(user);
        vault.redeem(perWithdrawal, user, user); // 0
        vault.redeem(perWithdrawal, user, user); // 1
        vault.redeem(perWithdrawal, user, user); // 2
        vm.stopPrank();

        // Lower max to 3 — existing 3 are fine; a 4th should now fail.
        vm.prank(admin);
        vault.updateMaxWithdrawalsPerUser(3);

        vm.prank(user);
        vm.expectRevert(InflowWithdrawalQueueLib.MaxWithdrawalsReached.selector);
        vault.redeem(perWithdrawal, user, user);
    }

    function test_update_max_withdrawals_increases_limit() public {
        uint256 perWithdrawal = 10_000e6;
        // Reduce limit to 2.
        vm.prank(admin);
        vault.updateMaxWithdrawalsPerUser(2);

        _deposit(user, perWithdrawal * 3);
        vm.prank(admin);
        vault.withdrawForDeployment(perWithdrawal * 3);

        vm.startPrank(user);
        vault.redeem(perWithdrawal, user, user); // 0
        vault.redeem(perWithdrawal, user, user); // 1

        vm.expectRevert(InflowWithdrawalQueueLib.MaxWithdrawalsReached.selector);
        vault.redeem(perWithdrawal, user, user); // 2 — blocked
        vm.stopPrank();

        // Raise the limit to 5.
        vm.prank(admin);
        vault.updateMaxWithdrawalsPerUser(5);

        // Now the 3rd withdrawal should succeed.
        vm.prank(user);
        vault.redeem(perWithdrawal, user, user); // 2 — allowed now
        assertEq(vault.getUserWithdrawalIds(user).length, 3);
    }
}
