// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Test} from "forge-std/Test.sol";
import {ERC1967Proxy} from "@openzeppelin/contracts/proxy/ERC1967/ERC1967Proxy.sol";
import {BasicInflowAdapter} from "../contracts/BasicInflowAdapter.sol";
import {MockERC20} from "./mocks/Mocks.sol";

/// @notice Unit tests for BasicInflowAdapter's one-depositor-per-asset model: the fix for
/// the audit findings that (1) any registered depositor could withdraw another depositor's
/// funds from a shared pool, and (2) unregisterDepositor had no guard against stranding funds.
contract BasicInflowAdapterTest is Test {
    address internal admin = makeAddr("admin");
    address internal admin2 = makeAddr("admin2");
    address internal depositorA = makeAddr("depositorA");
    address internal depositorB = makeAddr("depositorB");
    address internal stranger = makeAddr("stranger");

    BasicInflowAdapter internal adapter;
    MockERC20 internal tokenA;
    MockERC20 internal tokenB;

    function setUp() public {
        adapter = _deployAdapter(admin);
        tokenA = new MockERC20("Token A", "TKA", 6);
        tokenB = new MockERC20("Token B", "TKB", 18);
    }

    function _deployAdapter(address a) internal returns (BasicInflowAdapter) {
        address[] memory admins = new address[](1);
        admins[0] = a;
        BasicInflowAdapter impl = new BasicInflowAdapter();
        bytes memory initData = abi.encodeCall(BasicInflowAdapter.initialize, (admins));
        return BasicInflowAdapter(address(new ERC1967Proxy(address(impl), initData)));
    }

    function _registerDepositor(address depositor, address token) internal {
        vm.prank(admin);
        adapter.registerDepositor(depositor, abi.encode(token));
    }

    function _mintAndApprove(MockERC20 token, address who, uint256 amount) internal {
        token.mint(who, amount);
        vm.prank(who);
        token.approve(address(adapter), amount);
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Initialize / admin management
    // ═══════════════════════════════════════════════════════════════════════════

    function test_initialize_success() public view {
        assertTrue(adapter.isAdmin(admin));
        address[] memory admins = adapter.getAdmins();
        assertEq(admins.length, 1);
        assertEq(admins[0], admin);
    }

    function test_initialize_empty_admins_reverts() public {
        address[] memory admins = new address[](0);
        BasicInflowAdapter impl = new BasicInflowAdapter();
        bytes memory initData = abi.encodeCall(BasicInflowAdapter.initialize, (admins));
        vm.expectRevert(BasicInflowAdapter.AdminListCannotBeEmpty.selector);
        new ERC1967Proxy(address(impl), initData);
    }

    function test_initialize_zero_address_admin_reverts() public {
        address[] memory admins = new address[](1);
        admins[0] = address(0);
        BasicInflowAdapter impl = new BasicInflowAdapter();
        bytes memory initData = abi.encodeCall(BasicInflowAdapter.initialize, (admins));
        vm.expectRevert(BasicInflowAdapter.ZeroAddress.selector);
        new ERC1967Proxy(address(impl), initData);
    }

    function test_initialize_duplicate_admins_deduplicated() public {
        address[] memory admins = new address[](2);
        admins[0] = admin;
        admins[1] = admin;
        BasicInflowAdapter impl = new BasicInflowAdapter();
        bytes memory initData = abi.encodeCall(BasicInflowAdapter.initialize, (admins));
        BasicInflowAdapter a = BasicInflowAdapter(address(new ERC1967Proxy(address(impl), initData)));
        assertEq(a.getAdmins().length, 1);
    }

    function test_add_admin_success() public {
        vm.prank(admin);
        adapter.addAdmin(admin2);
        assertTrue(adapter.isAdmin(admin2));
    }

    function test_add_admin_zero_address_reverts() public {
        vm.prank(admin);
        vm.expectRevert(BasicInflowAdapter.ZeroAddress.selector);
        adapter.addAdmin(address(0));
    }

    function test_add_admin_already_admin_reverts() public {
        vm.startPrank(admin);
        adapter.addAdmin(admin2);
        vm.expectRevert(BasicInflowAdapter.AlreadyAdmin.selector);
        adapter.addAdmin(admin2);
        vm.stopPrank();
    }

    function test_add_admin_unauthorized_reverts() public {
        vm.prank(stranger);
        vm.expectRevert(BasicInflowAdapter.Unauthorized.selector);
        adapter.addAdmin(admin2);
    }

    function test_remove_admin_success() public {
        vm.startPrank(admin);
        adapter.addAdmin(admin2);
        adapter.removeAdmin(admin2);
        vm.stopPrank();
        assertFalse(adapter.isAdmin(admin2));
    }

    function test_remove_admin_not_admin_reverts() public {
        vm.prank(admin);
        vm.expectRevert(BasicInflowAdapter.NotAdmin.selector);
        adapter.removeAdmin(admin2);
    }

    function test_remove_admin_last_admin_reverts() public {
        vm.prank(admin);
        vm.expectRevert(BasicInflowAdapter.AdminListCannotBeEmpty.selector);
        adapter.removeAdmin(admin);
    }

    function test_remove_admin_unauthorized_reverts() public {
        vm.prank(stranger);
        vm.expectRevert(BasicInflowAdapter.Unauthorized.selector);
        adapter.removeAdmin(admin);
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Registration - one depositor per asset
    // ═══════════════════════════════════════════════════════════════════════════

    function test_register_depositor_success_decodes_token_from_metadata() public {
        _registerDepositor(depositorA, address(tokenA));
        assertTrue(adapter.isDepositorRegistered(depositorA));
        assertTrue(adapter.isDepositorEnabled(depositorA));
        assertEq(adapter.depositorToken(depositorA), address(tokenA));
        assertEq(adapter.tokenDepositor(address(tokenA)), depositorA);
    }

    function test_register_depositor_invalid_metadata_length_reverts() public {
        vm.prank(admin);
        vm.expectRevert(BasicInflowAdapter.InvalidMetadata.selector);
        adapter.registerDepositor(depositorA, "");
    }

    function test_register_depositor_zero_token_reverts() public {
        vm.prank(admin);
        vm.expectRevert(BasicInflowAdapter.ZeroAddress.selector);
        adapter.registerDepositor(depositorA, abi.encode(address(0)));
    }

    function test_register_depositor_zero_address_depositor_reverts() public {
        vm.prank(admin);
        vm.expectRevert(BasicInflowAdapter.ZeroAddress.selector);
        adapter.registerDepositor(address(0), abi.encode(address(tokenA)));
    }

    function test_register_depositor_already_registered_reverts() public {
        _registerDepositor(depositorA, address(tokenA));
        vm.prank(admin);
        vm.expectRevert(BasicInflowAdapter.AlreadyRegistered.selector);
        adapter.registerDepositor(depositorA, abi.encode(address(tokenB)));
    }

    function test_register_depositor_token_already_claimed_reverts() public {
        _registerDepositor(depositorA, address(tokenA));
        vm.prank(admin);
        vm.expectRevert(abi.encodeWithSelector(BasicInflowAdapter.TokenAlreadyClaimed.selector, address(tokenA)));
        adapter.registerDepositor(depositorB, abi.encode(address(tokenA)));
    }

    function test_register_depositor_different_token_succeeds() public {
        _registerDepositor(depositorA, address(tokenA));
        _registerDepositor(depositorB, address(tokenB));
        assertEq(adapter.depositorToken(depositorA), address(tokenA));
        assertEq(adapter.depositorToken(depositorB), address(tokenB));
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Unregistration
    // ═══════════════════════════════════════════════════════════════════════════

    function test_unregister_depositor_zero_balance_succeeds() public {
        _registerDepositor(depositorA, address(tokenA));
        vm.prank(admin);
        adapter.unregisterDepositor(depositorA);
        assertFalse(adapter.isDepositorRegistered(depositorA));
        assertEq(adapter.depositorToken(depositorA), address(0));
        assertEq(adapter.tokenDepositor(address(tokenA)), address(0));
    }

    function test_unregister_depositor_nonzero_balance_reverts() public {
        _registerDepositor(depositorA, address(tokenA));
        _mintAndApprove(tokenA, depositorA, 100e6);
        vm.prank(depositorA);
        adapter.deposit(100e6, address(tokenA));

        vm.prank(admin);
        vm.expectRevert(abi.encodeWithSelector(BasicInflowAdapter.PositionNotEmpty.selector, address(tokenA)));
        adapter.unregisterDepositor(depositorA);
    }

    function test_unregister_depositor_not_registered_reverts() public {
        vm.prank(admin);
        vm.expectRevert(BasicInflowAdapter.NotRegistered.selector);
        adapter.unregisterDepositor(depositorA);
    }

    function test_unregister_depositor_unauthorized_reverts() public {
        _registerDepositor(depositorA, address(tokenA));
        vm.prank(stranger);
        vm.expectRevert(BasicInflowAdapter.Unauthorized.selector);
        adapter.unregisterDepositor(depositorA);
    }

    function test_unregister_depositor_frees_token_slot_for_reuse() public {
        _registerDepositor(depositorA, address(tokenA));
        vm.prank(admin);
        adapter.unregisterDepositor(depositorA);

        _registerDepositor(depositorB, address(tokenA));
        assertEq(adapter.depositorToken(depositorB), address(tokenA));
        assertEq(adapter.tokenDepositor(address(tokenA)), depositorB);
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Enable / disable
    // ═══════════════════════════════════════════════════════════════════════════

    function test_set_depositor_enabled_success() public {
        _registerDepositor(depositorA, address(tokenA));
        vm.prank(admin);
        adapter.setDepositorEnabled(depositorA, false);
        assertFalse(adapter.isDepositorEnabled(depositorA));

        vm.prank(admin);
        adapter.setDepositorEnabled(depositorA, true);
        assertTrue(adapter.isDepositorEnabled(depositorA));
    }

    function test_set_depositor_enabled_not_registered_reverts() public {
        vm.prank(admin);
        vm.expectRevert(BasicInflowAdapter.NotRegistered.selector);
        adapter.setDepositorEnabled(depositorA, false);
    }

    function test_set_depositor_enabled_unauthorized_reverts() public {
        _registerDepositor(depositorA, address(tokenA));
        vm.prank(stranger);
        vm.expectRevert(BasicInflowAdapter.Unauthorized.selector);
        adapter.setDepositorEnabled(depositorA, false);
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Deposit / withdraw access control
    // ═══════════════════════════════════════════════════════════════════════════

    function test_deposit_success_for_own_token() public {
        _registerDepositor(depositorA, address(tokenA));
        _mintAndApprove(tokenA, depositorA, 100e6);
        vm.prank(depositorA);
        adapter.deposit(100e6, address(tokenA));
        assertEq(tokenA.balanceOf(address(adapter)), 100e6);
    }

    function test_deposit_wrong_token_reverts() public {
        _registerDepositor(depositorA, address(tokenA));
        _mintAndApprove(tokenB, depositorA, 100e18);
        vm.prank(depositorA);
        vm.expectRevert(BasicInflowAdapter.Unauthorized.selector);
        adapter.deposit(100e18, address(tokenB));
    }

    function test_deposit_unregistered_depositor_reverts() public {
        _mintAndApprove(tokenA, depositorA, 100e6);
        vm.prank(depositorA);
        vm.expectRevert(BasicInflowAdapter.Unauthorized.selector);
        adapter.deposit(100e6, address(tokenA));
    }

    function test_deposit_disabled_depositor_reverts() public {
        _registerDepositor(depositorA, address(tokenA));
        vm.prank(admin);
        adapter.setDepositorEnabled(depositorA, false);

        _mintAndApprove(tokenA, depositorA, 100e6);
        vm.prank(depositorA);
        vm.expectRevert(BasicInflowAdapter.Unauthorized.selector);
        adapter.deposit(100e6, address(tokenA));
    }

    function test_withdraw_success_for_own_token() public {
        _registerDepositor(depositorA, address(tokenA));
        _mintAndApprove(tokenA, depositorA, 100e6);
        vm.prank(depositorA);
        adapter.deposit(100e6, address(tokenA));

        vm.prank(depositorA);
        adapter.withdraw(40e6, address(tokenA));
        assertEq(tokenA.balanceOf(address(adapter)), 60e6);
        assertEq(tokenA.balanceOf(depositorA), 40e6);
    }

    function test_withdraw_wrong_token_reverts() public {
        _registerDepositor(depositorA, address(tokenA));
        _mintAndApprove(tokenA, depositorA, 100e6);
        vm.prank(depositorA);
        adapter.deposit(100e6, address(tokenA));

        vm.prank(depositorA);
        vm.expectRevert(BasicInflowAdapter.Unauthorized.selector);
        adapter.withdraw(1, address(tokenB));
    }

    function test_withdraw_disabled_depositor_reverts() public {
        _registerDepositor(depositorA, address(tokenA));
        _mintAndApprove(tokenA, depositorA, 100e6);
        vm.prank(depositorA);
        adapter.deposit(100e6, address(tokenA));

        vm.prank(admin);
        adapter.setDepositorEnabled(depositorA, false);

        vm.prank(depositorA);
        vm.expectRevert(BasicInflowAdapter.Unauthorized.selector);
        adapter.withdraw(1e6, address(tokenA));
    }

    /// @notice Headline regression test for audit finding #1: two depositors on distinct
    /// tokens must never be able to touch each other's balance.
    function test_two_depositors_distinct_tokens_no_cross_contamination() public {
        _registerDepositor(depositorA, address(tokenA));
        _registerDepositor(depositorB, address(tokenB));

        _mintAndApprove(tokenA, depositorA, 100e6);
        vm.prank(depositorA);
        adapter.deposit(100e6, address(tokenA));

        _mintAndApprove(tokenB, depositorB, 50e18);
        vm.prank(depositorB);
        adapter.deposit(50e18, address(tokenB));

        // depositorA cannot withdraw depositorB's token, and vice versa.
        vm.prank(depositorA);
        vm.expectRevert(BasicInflowAdapter.Unauthorized.selector);
        adapter.withdraw(1e18, address(tokenB));

        vm.prank(depositorB);
        vm.expectRevert(BasicInflowAdapter.Unauthorized.selector);
        adapter.withdraw(1e6, address(tokenA));

        // Each depositor's position reflects only their own token.
        assertEq(adapter.depositorPosition(depositorA, address(tokenA)), 100e6);
        assertEq(adapter.depositorPosition(depositorA, address(tokenB)), 0);
        assertEq(adapter.depositorPosition(depositorB, address(tokenB)), 50e18);
        assertEq(adapter.depositorPosition(depositorB, address(tokenA)), 0);
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Views
    // ═══════════════════════════════════════════════════════════════════════════

    function test_depositorPosition_returns_balance_for_own_token() public {
        _registerDepositor(depositorA, address(tokenA));
        _mintAndApprove(tokenA, depositorA, 100e6);
        vm.prank(depositorA);
        adapter.deposit(100e6, address(tokenA));
        assertEq(adapter.depositorPosition(depositorA, address(tokenA)), 100e6);
    }

    function test_depositorPosition_returns_zero_for_mismatched_token() public {
        _registerDepositor(depositorA, address(tokenA));
        assertEq(adapter.depositorPosition(depositorA, address(tokenB)), 0);
    }

    function test_depositorPosition_returns_zero_for_unregistered_depositor() public view {
        assertEq(adapter.depositorPosition(stranger, address(tokenA)), 0);
    }

    function test_availableForDeposit_max_for_registered_enabled_own_token() public {
        _registerDepositor(depositorA, address(tokenA));
        assertEq(adapter.availableForDeposit(depositorA, address(tokenA)), type(uint256).max);
    }

    function test_availableForDeposit_zero_for_wrong_token_or_disabled() public {
        _registerDepositor(depositorA, address(tokenA));
        assertEq(adapter.availableForDeposit(depositorA, address(tokenB)), 0);

        vm.prank(admin);
        adapter.setDepositorEnabled(depositorA, false);
        assertEq(adapter.availableForDeposit(depositorA, address(tokenA)), 0);
    }

    function test_availableForWithdraw_returns_balance_for_own_token() public {
        _registerDepositor(depositorA, address(tokenA));
        _mintAndApprove(tokenA, depositorA, 100e6);
        vm.prank(depositorA);
        adapter.deposit(100e6, address(tokenA));
        assertEq(adapter.availableForWithdraw(depositorA, address(tokenA)), 100e6);
    }

    function test_availableForWithdraw_zero_for_wrong_token() public {
        _registerDepositor(depositorA, address(tokenA));
        assertEq(adapter.availableForWithdraw(depositorA, address(tokenB)), 0);
    }
}
