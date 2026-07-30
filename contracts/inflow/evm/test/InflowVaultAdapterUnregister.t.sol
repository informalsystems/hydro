// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {ERC1967Proxy} from "@openzeppelin/contracts/proxy/ERC1967/ERC1967Proxy.sol";
import {InflowVaultBase} from "./InflowVaultBase.t.sol";
import {InflowAdapterLib} from "../contracts/InflowAdapterLib.sol";
import {BasicInflowAdapter} from "../contracts/BasicInflowAdapter.sol";
import {ReserveAdapter} from "../contracts/ReserveAdapter.sol";

/// @notice Integration tests proving InflowAdapterLib.unregisterAdapter's real interaction
/// with the two production adapter implementations: it blocks disconnecting from
/// BasicInflowAdapter while a real position remains, but always allows disconnecting from
/// ReserveAdapter regardless of balance (by design, see ReserveAdapter's NatSpec). Uses the
/// real adapter contracts rather than Mocks.sol, since the point is to exercise their actual
/// depositorPosition() implementations through the vault's guard.
contract InflowVaultAdapterUnregisterTest is InflowVaultBase {
    BasicInflowAdapter internal basicAdapter;
    ReserveAdapter internal reserveAdapter;

    function setUp() public override {
        super.setUp();
        basicAdapter = _deployBasicAdapter();
        reserveAdapter = _deployReserveAdapter();
    }

    function _deployBasicAdapter() internal returns (BasicInflowAdapter) {
        address[] memory admins = new address[](1);
        admins[0] = admin;
        BasicInflowAdapter impl = new BasicInflowAdapter();
        bytes memory initData = abi.encodeCall(BasicInflowAdapter.initialize, (admins));
        BasicInflowAdapter a = BasicInflowAdapter(address(new ERC1967Proxy(address(impl), initData)));
        vm.prank(admin);
        a.registerDepositor(address(vault), abi.encode(address(asset)));
        return a;
    }

    function _deployReserveAdapter() internal returns (ReserveAdapter) {
        address[] memory admins = new address[](1);
        admins[0] = admin;
        ReserveAdapter impl = new ReserveAdapter();
        bytes memory initData = abi.encodeCall(ReserveAdapter.initialize, (admins));
        ReserveAdapter a = ReserveAdapter(address(new ERC1967Proxy(address(impl), initData)));
        vm.prank(admin);
        a.registerDepositor(address(vault), "");
        return a;
    }

    function test_vault_unregisterAdapter_blocked_while_basic_adapter_holds_position() public {
        vm.prank(admin);
        vault.registerAdapter("basic", address(basicAdapter), false, false);

        _deposit(user, 1_000e6);
        vm.prank(admin);
        vault.depositToAdapter("basic", 500e6);

        vm.prank(admin);
        vm.expectRevert(abi.encodeWithSelector(InflowAdapterLib.AdapterPositionNotEmpty.selector, "basic"));
        vault.unregisterAdapter("basic");

        vm.prank(admin);
        vault.withdrawFromAdapter("basic", 500e6);

        vm.prank(admin);
        vault.unregisterAdapter("basic");
    }

    function test_vault_unregisterAdapter_always_succeeds_for_reserve_adapter_regardless_of_balance() public {
        vm.prank(admin);
        vault.registerAdapter("reserve", address(reserveAdapter), false, true);

        _deposit(user, 1_000e6);
        vm.prank(admin);
        vault.depositToAdapter("reserve", 500e6);
        assertEq(asset.balanceOf(address(reserveAdapter)), 500e6);

        // Succeeds immediately despite the nonzero real balance sitting in the reserve.
        vm.prank(admin);
        vault.unregisterAdapter("reserve");
    }

    /// @notice Demonstrates the required ReserveAdapter operational runbook end-to-end:
    /// depositToAdapter alone is net-zero on totalAssets (tracked adapter: balance drops,
    /// deployedAmount bumps by the same amount), and only the compensating
    /// submitDeployedAmount call, computed to cancel that automatic bump back out, is
    /// what actually keeps the reserve's funds out of the vault's reported totalAssets().
    function test_reserve_adapter_deposit_bumps_deployedAmount_and_compensating_submit_restores_totalAssets() public {
        vm.prank(admin);
        vault.registerAdapter("reserve", address(reserveAdapter), false, true);

        _deposit(user, 1_000e6);
        uint256 totalAssetsBefore = vault.totalAssets();

        vm.prank(admin);
        vault.depositToAdapter("reserve", 200e6);
        assertEq(vault.totalAssets(), totalAssetsBefore);

        vm.prank(admin);
        vault.submitDeployedAmount(0);
        assertEq(vault.totalAssets(), totalAssetsBefore - 200e6);
    }
}
