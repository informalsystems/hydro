// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Test} from "forge-std/Test.sol";
import {ERC1967Proxy} from "@openzeppelin/contracts/proxy/ERC1967/ERC1967Proxy.sol";
import {ReserveAdapter} from "../contracts/ReserveAdapter.sol";
import {MockERC20} from "./mocks/Mocks.sol";

/// @notice Unit tests for ReserveAdapter: the shared-pool backstop reserve. Unlike
/// BasicInflowAdapter, the shared-pool/no-per-depositor-accounting behavior here is
/// intentional (multiple vaults are meant to draw on the same pool), and
/// depositorPosition() is hardcoded to 0 so the adapter is always disconnectable
/// from any vault regardless of real balance.
contract ReserveAdapterTest is Test {
    address internal admin = makeAddr("admin");
    address internal admin2 = makeAddr("admin2");
    address internal depositorA = makeAddr("depositorA");
    address internal depositorB = makeAddr("depositorB");
    address internal stranger = makeAddr("stranger");

    ReserveAdapter internal adapter;
    MockERC20 internal token;

    function setUp() public {
        adapter = _deployAdapter(admin);
        token = new MockERC20("Token", "TKN", 6);
    }

    function _deployAdapter(address a) internal returns (ReserveAdapter) {
        address[] memory admins = new address[](1);
        admins[0] = a;
        ReserveAdapter impl = new ReserveAdapter();
        bytes memory initData = abi.encodeCall(ReserveAdapter.initialize, (admins));
        return ReserveAdapter(address(new ERC1967Proxy(address(impl), initData)));
    }

    function _registerDepositor(address depositor) internal {
        vm.prank(admin);
        adapter.registerDepositor(depositor, "");
    }

    function _mintAndApprove(address who, uint256 amount) internal {
        token.mint(who, amount);
        vm.prank(who);
        token.approve(address(adapter), amount);
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Initialize / admin management
    // ═══════════════════════════════════════════════════════════════════════════

    function test_initialize_success() public view {
        assertTrue(adapter.isAdmin(admin));
        assertEq(adapter.getAdmins().length, 1);
    }

    function test_initialize_empty_admins_reverts() public {
        address[] memory admins = new address[](0);
        ReserveAdapter impl = new ReserveAdapter();
        bytes memory initData = abi.encodeCall(ReserveAdapter.initialize, (admins));
        vm.expectRevert(ReserveAdapter.AdminListCannotBeEmpty.selector);
        new ERC1967Proxy(address(impl), initData);
    }

    function test_initialize_zero_address_admin_reverts() public {
        address[] memory admins = new address[](1);
        admins[0] = address(0);
        ReserveAdapter impl = new ReserveAdapter();
        bytes memory initData = abi.encodeCall(ReserveAdapter.initialize, (admins));
        vm.expectRevert(ReserveAdapter.ZeroAddress.selector);
        new ERC1967Proxy(address(impl), initData);
    }

    function test_initialize_duplicate_admins_deduplicated() public {
        address[] memory admins = new address[](2);
        admins[0] = admin;
        admins[1] = admin;
        ReserveAdapter impl = new ReserveAdapter();
        bytes memory initData = abi.encodeCall(ReserveAdapter.initialize, (admins));
        ReserveAdapter a = ReserveAdapter(address(new ERC1967Proxy(address(impl), initData)));
        assertEq(a.getAdmins().length, 1);
    }

    function test_add_admin_success() public {
        vm.prank(admin);
        adapter.addAdmin(admin2);
        assertTrue(adapter.isAdmin(admin2));
    }

    function test_add_admin_zero_address_reverts() public {
        vm.prank(admin);
        vm.expectRevert(ReserveAdapter.ZeroAddress.selector);
        adapter.addAdmin(address(0));
    }

    function test_add_admin_already_admin_reverts() public {
        vm.startPrank(admin);
        adapter.addAdmin(admin2);
        vm.expectRevert(ReserveAdapter.AlreadyAdmin.selector);
        adapter.addAdmin(admin2);
        vm.stopPrank();
    }

    function test_add_admin_unauthorized_reverts() public {
        vm.prank(stranger);
        vm.expectRevert(ReserveAdapter.Unauthorized.selector);
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
        vm.expectRevert(ReserveAdapter.NotAdmin.selector);
        adapter.removeAdmin(admin2);
    }

    function test_remove_admin_last_admin_reverts() public {
        vm.prank(admin);
        vm.expectRevert(ReserveAdapter.AdminListCannotBeEmpty.selector);
        adapter.removeAdmin(admin);
    }

    function test_remove_admin_unauthorized_reverts() public {
        vm.prank(stranger);
        vm.expectRevert(ReserveAdapter.Unauthorized.selector);
        adapter.removeAdmin(admin);
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Registration - no per-asset exclusivity
    // ═══════════════════════════════════════════════════════════════════════════

    function test_register_depositor_ignores_metadata() public {
        vm.prank(admin);
        adapter.registerDepositor(depositorA, "");
        assertTrue(adapter.isDepositorRegistered(depositorA));
    }

    function test_register_depositor_already_registered_reverts() public {
        _registerDepositor(depositorA);
        vm.prank(admin);
        vm.expectRevert(ReserveAdapter.AlreadyRegistered.selector);
        adapter.registerDepositor(depositorA, "");
    }

    function test_register_depositor_multiple_depositors_same_token_succeeds() public {
        _registerDepositor(depositorA);
        _registerDepositor(depositorB);
        assertTrue(adapter.isDepositorRegistered(depositorA));
        assertTrue(adapter.isDepositorRegistered(depositorB));
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Shared-pool behavior (intentional, unlike BasicInflowAdapter)
    // ═══════════════════════════════════════════════════════════════════════════

    function test_shared_pool_two_depositors_same_token_can_both_deposit_and_withdraw() public {
        _registerDepositor(depositorA);
        _registerDepositor(depositorB);

        _mintAndApprove(depositorA, 100e6);
        vm.prank(depositorA);
        adapter.deposit(100e6, address(token));

        // depositorB, who deposited nothing, can withdraw part of depositorA's deposit:
        // this is the intended shared-reserve semantics.
        vm.prank(depositorB);
        adapter.withdraw(40e6, address(token));

        assertEq(token.balanceOf(depositorB), 40e6);
        assertEq(token.balanceOf(address(adapter)), 60e6);
    }

    function test_availableForWithdraw_reflects_full_shared_balance_for_any_enabled_depositor() public {
        _registerDepositor(depositorA);
        _registerDepositor(depositorB);

        _mintAndApprove(depositorA, 100e6);
        vm.prank(depositorA);
        adapter.deposit(100e6, address(token));

        assertEq(adapter.availableForWithdraw(depositorB, address(token)), 100e6);
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Always-zero position (enables always-disconnectable)
    // ═══════════════════════════════════════════════════════════════════════════

    function test_depositorPosition_always_zero_despite_real_balance() public {
        _registerDepositor(depositorA);
        _mintAndApprove(depositorA, 100e6);
        vm.prank(depositorA);
        adapter.deposit(100e6, address(token));

        assertEq(adapter.depositorPosition(depositorA, address(token)), 0);
    }

    function test_depositorPosition_always_zero_for_arbitrary_caller_and_token_args() public view {
        assertEq(adapter.depositorPosition(stranger, address(0xdead)), 0);
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Unregister always succeeds, regardless of balance
    // ═══════════════════════════════════════════════════════════════════════════

    function test_unregister_depositor_succeeds_regardless_of_balance() public {
        _registerDepositor(depositorA);
        _mintAndApprove(depositorA, 100e6);
        vm.prank(depositorA);
        adapter.deposit(100e6, address(token));

        // Contrast with BasicInflowAdapter, which would revert PositionNotEmpty here.
        vm.prank(admin);
        adapter.unregisterDepositor(depositorA);
        assertFalse(adapter.isDepositorRegistered(depositorA));
    }

    function test_unregister_depositor_not_registered_reverts() public {
        vm.prank(admin);
        vm.expectRevert(ReserveAdapter.NotRegistered.selector);
        adapter.unregisterDepositor(depositorA);
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Deposit / withdraw access control
    // ═══════════════════════════════════════════════════════════════════════════

    function test_deposit_unregistered_reverts() public {
        _mintAndApprove(depositorA, 100e6);
        vm.prank(depositorA);
        vm.expectRevert(ReserveAdapter.Unauthorized.selector);
        adapter.deposit(100e6, address(token));
    }

    function test_deposit_disabled_reverts() public {
        _registerDepositor(depositorA);
        vm.prank(admin);
        adapter.setDepositorEnabled(depositorA, false);

        _mintAndApprove(depositorA, 100e6);
        vm.prank(depositorA);
        vm.expectRevert(ReserveAdapter.Unauthorized.selector);
        adapter.deposit(100e6, address(token));
    }

    function test_withdraw_unregistered_reverts() public {
        vm.prank(depositorA);
        vm.expectRevert(ReserveAdapter.Unauthorized.selector);
        adapter.withdraw(1e6, address(token));
    }

    function test_withdraw_disabled_reverts() public {
        _registerDepositor(depositorA);
        _mintAndApprove(depositorA, 100e6);
        vm.prank(depositorA);
        adapter.deposit(100e6, address(token));

        vm.prank(admin);
        adapter.setDepositorEnabled(depositorA, false);

        vm.prank(depositorA);
        vm.expectRevert(ReserveAdapter.Unauthorized.selector);
        adapter.withdraw(1e6, address(token));
    }
}
