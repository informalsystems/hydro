// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {InflowVaultBase, InflowVault} from "./InflowVaultBase.t.sol";

/// @notice Tests for whitelist management and two-whitelist access enforcement.
/// Corresponds to: whitelist_add_remove_test in vault/testing.rs and
/// control-center/testing.rs; submit_deployed_amount_test in
/// control-center/testing.rs.
contract InflowVaultAccessControlTest is InflowVaultBase {
    // ═══════════════════════════════════════════════════════════════════════
    // Primary whitelist
    // ═══════════════════════════════════════════════════════════════════════

    function test_add_to_whitelist_success() public {
        vm.expectEmit(true, false, false, false, address(vault));
        emit InflowVault.WhitelistAdded(user);

        vm.prank(admin);
        vault.addToWhitelist(user);

        assertTrue(vault.whitelist(user), "user should be whitelisted");
    }

    function test_add_to_whitelist_unauthorized() public {
        vm.prank(stranger);
        vm.expectRevert(InflowVault.Unauthorized.selector);
        vault.addToWhitelist(user);
    }

    function test_add_to_whitelist_zero_address_reverts() public {
        vm.prank(admin);
        vm.expectRevert(InflowVault.ZeroAddress.selector);
        vault.addToWhitelist(address(0));
    }

    function test_add_to_whitelist_duplicate_reverts() public {
        vm.prank(admin);
        vm.expectRevert(InflowVault.AlreadyWhitelisted.selector);
        vault.addToWhitelist(admin); // admin is already in the whitelist
    }

    function test_remove_from_whitelist_success() public {
        // First add a second address so we can remove one.
        vm.prank(admin);
        vault.addToWhitelist(user);

        vm.expectEmit(true, false, false, false, address(vault));
        emit InflowVault.WhitelistRemoved(user);

        vm.prank(admin);
        vault.removeFromWhitelist(user);

        assertFalse(vault.whitelist(user), "user should be removed");
    }

    function test_remove_from_whitelist_unauthorized() public {
        vm.prank(stranger);
        vm.expectRevert(InflowVault.Unauthorized.selector);
        vault.removeFromWhitelist(admin);
    }

    function test_remove_from_whitelist_not_whitelisted_reverts() public {
        vm.prank(admin);
        vm.expectRevert(InflowVault.NotWhitelisted.selector);
        vault.removeFromWhitelist(stranger);
    }

    function test_remove_from_whitelist_last_entry_reverts() public {
        // Only admin is in the whitelist.
        vm.prank(admin);
        vm.expectRevert(InflowVault.WhitelistCannotBeEmpty.selector);
        vault.removeFromWhitelist(admin);
    }

    function test_removed_address_loses_whitelist_privileges() public {
        vm.prank(admin);
        vault.addToWhitelist(user);

        vm.prank(admin);
        vault.removeFromWhitelist(user);

        vm.prank(user);
        vm.expectRevert(InflowVault.Unauthorized.selector);
        vault.updateDepositCap(999e6);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Deployed amount whitelist
    // ═══════════════════════════════════════════════════════════════════════

    function test_add_to_deployed_amount_whitelist_success() public {
        vm.expectEmit(true, false, false, false, address(vault));
        emit InflowVault.DeployedAmountWhitelistAdded(user);

        vm.prank(admin);
        vault.addToDeployedAmountWhitelist(user);

        assertTrue(vault.deployedAmountWhitelist(user));
    }

    function test_add_to_deployed_amount_whitelist_unauthorized() public {
        // Only primary whitelist members may manage the DA whitelist.
        vm.prank(stranger);
        vm.expectRevert(InflowVault.Unauthorized.selector);
        vault.addToDeployedAmountWhitelist(user);
    }

    function test_add_to_deployed_amount_whitelist_zero_address_reverts() public {
        vm.prank(admin);
        vm.expectRevert(InflowVault.ZeroAddress.selector);
        vault.addToDeployedAmountWhitelist(address(0));
    }

    function test_add_to_deployed_amount_whitelist_duplicate_reverts() public {
        vm.prank(admin);
        vm.expectRevert(InflowVault.AlreadyDeployedAmountWhitelisted.selector);
        vault.addToDeployedAmountWhitelist(admin); // admin already in DA whitelist
    }

    function test_remove_from_deployed_amount_whitelist_success() public {
        vm.prank(admin);
        vault.addToDeployedAmountWhitelist(user);

        vm.expectEmit(true, false, false, false, address(vault));
        emit InflowVault.DeployedAmountWhitelistRemoved(user);

        vm.prank(admin);
        vault.removeFromDeployedAmountWhitelist(user);

        assertFalse(vault.deployedAmountWhitelist(user));
    }

    function test_remove_from_deployed_amount_whitelist_last_entry_reverts() public {
        vm.prank(admin);
        vm.expectRevert(InflowVault.DeployedAmountWhitelistCannotBeEmpty.selector);
        vault.removeFromDeployedAmountWhitelist(admin);
    }

    function test_remove_from_deployed_amount_whitelist_not_member_reverts() public {
        vm.prank(admin);
        vm.expectRevert(InflowVault.NotWhitelisted.selector);
        vault.removeFromDeployedAmountWhitelist(stranger);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Whitelist enforcement on key functions
    // ═══════════════════════════════════════════════════════════════════════

    /// submitDeployedAmount requires deployedAmountWhitelist, NOT the primary whitelist.
    function test_submit_deployed_amount_requires_deployed_amount_whitelist() public {
        // Add `user` to the primary whitelist only.
        vm.prank(admin);
        vault.addToWhitelist(user);

        vm.prank(user);
        vm.expectRevert(InflowVault.Unauthorized.selector);
        vault.submitDeployedAmount(100_000e6);
    }

    /// A DA-whitelist-only address can submit deployed amount but cannot call
    /// primary-whitelist-only functions (e.g. withdrawForDeployment).
    function test_deployed_amount_whitelist_only_can_submit_not_withdraw() public {
        address daOnly = makeAddr("daOnly");
        vm.prank(admin);
        vault.addToDeployedAmountWhitelist(daOnly);

        // submitDeployedAmount should succeed.
        vm.prank(daOnly);
        vault.submitDeployedAmount(0); // allowed

        // withdrawForDeployment requires primary whitelist.
        _deposit(user, 100_000e6);
        vm.prank(daOnly);
        vm.expectRevert(InflowVault.Unauthorized.selector);
        vault.withdrawForDeployment(100_000e6);
    }

    function test_deposit_from_deployment_requires_primary_whitelist() public {
        asset.mint(stranger, 100_000e6);
        vm.prank(stranger);
        asset.approve(address(vault), 100_000e6);

        vm.prank(stranger);
        vm.expectRevert(InflowVault.Unauthorized.selector);
        vault.depositFromDeployment(100_000e6);
    }

    function test_update_deposit_cap_requires_primary_whitelist() public {
        vm.prank(stranger);
        vm.expectRevert(InflowVault.Unauthorized.selector);
        vault.updateDepositCap(500e6);
    }

    function test_register_adapter_requires_primary_whitelist() public {
        vm.prank(stranger);
        vm.expectRevert(InflowVault.Unauthorized.selector);
        vault.registerAdapter("x", stranger, true, false);
    }
}
