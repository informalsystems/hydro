// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {InflowVaultBase, InflowVault, MockAdapterWithAsset} from "./InflowVaultBase.t.sol";
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
        assertEq(info.addr,      address(a));
        assertTrue(info.automated);
        assertFalse(info.tracked);
        assertEq(info.name,      "myAdapter");
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
        vault.registerAdapter("first",  address(a1), true,  false);
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

        assertEq(asset.balanceOf(address(a)),   0,          "manual adapter not used on deposit");
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

    // ═══════════════════════════════════════════════════════════════════════
    // Manual depositToAdapter
    // ═══════════════════════════════════════════════════════════════════════

    function test_deposit_to_adapter_success() public {
        MockAdapterWithAsset a = _newAdapter();
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
        _registerAdapter("a", a, false, false); // manual (not automated)

        _deposit(user, 50_000e6);
        vm.prank(admin);
        vault.depositToAdapter("a", 50_000e6); // explicit call works even in manual mode

        assertEq(asset.balanceOf(address(a)), 50_000e6);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // moveAdapterFunds
    // ═══════════════════════════════════════════════════════════════════════

    function test_move_adapter_funds_tracked_to_tracked() public {
        MockAdapterWithAsset a1 = _newAdapter();
        MockAdapterWithAsset a2 = _newAdapter();
        _registerAdapter("a1", a1, false, true);
        _registerAdapter("a2", a2, false, true);

        _deposit(user, 50_000e6);
        vm.prank(admin);
        vault.depositToAdapter("a1", 50_000e6);

        uint256 deployedBefore = vault.deployedAmount();
        vm.prank(admin);
        vault.moveAdapterFunds("a1", "a2", 50_000e6);

        assertEq(vault.deployedAmount(), deployedBefore, "tracked-to-tracked: deployedAmount unchanged");
        assertEq(asset.balanceOf(address(a1)), 0,          "a1 drained");
        assertEq(asset.balanceOf(address(a2)), 50_000e6,   "a2 received");
    }

    function test_move_adapter_funds_tracked_to_untracked() public {
        MockAdapterWithAsset a1 = _newAdapter();
        MockAdapterWithAsset a2 = _newAdapter();
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
        _registerAdapter("a1", a1, false, false); // untracked
        _registerAdapter("a2", a2, false, true);  // tracked

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
}

pragma solidity ^0.8.20;

import "forge-std/Test.sol";
import {ERC20} from "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {ERC1967Proxy} from "@openzeppelin/contracts/proxy/ERC1967/ERC1967Proxy.sol";
import {InflowVault} from "../contracts/InflowVault.sol";
import {InflowAdapterLib} from "../contracts/InflowAdapterLib.sol";
import {BasicInflowAdapter} from "../contracts/BasicInflowAdapter.sol";

contract MockERC20 is ERC20 {
    uint8 private _dec;
    constructor(string memory name, string memory symbol, uint8 dec) ERC20(name, symbol) { _dec = dec; }
    function decimals() public view override returns (uint8) { return _dec; }
    function mint(address to, uint256 amount) external { _mint(to, amount); }
}

contract InflowVaultAdaptersTest is Test {
    InflowVault vault;
    MockERC20 usdc;

    address admin = makeAddr("admin");
    address user  = makeAddr("user");

    uint256 constant AMOUNT          = 100_000e6;
    uint256 constant DEPOSIT_CAP     = 1_000_000e6;
    uint256 constant MAX_WITHDRAWALS = 10;

    function setUp() public {
        usdc = new MockERC20("USD Coin", "USDC", 6);

        address[] memory wl = new address[](1);
        wl[0] = admin;

        InflowVault impl = new InflowVault();
        bytes memory initData = abi.encodeCall(
            InflowVault.initialize,
            (IERC20(address(usdc)), "Hydro Inflow Vault", "hvUSDC",
             DEPOSIT_CAP, MAX_WITHDRAWALS, wl, wl, 0, address(0))
        );
        vault = InflowVault(address(new ERC1967Proxy(address(impl), initData)));

        usdc.mint(user, AMOUNT);
    }

    function _deployAdapter(address adminAddr) internal returns (BasicInflowAdapter) {
        address[] memory admins = new address[](1);
        admins[0] = adminAddr;
        BasicInflowAdapter impl = new BasicInflowAdapter();
        return BasicInflowAdapter(
            address(new ERC1967Proxy(address(impl),
                abi.encodeCall(BasicInflowAdapter.initialize, (admins))))
        );
    }

    /// @notice Adapter pull-pattern integration test:
    ///   1. Deploy BasicInflowAdapter and register it on the vault (automated, untracked).
    ///   2. User deposits — vault approves the adapter, adapter pulls tokens via transferFrom.
    ///      Tokens must land in the adapter, not the vault.
    ///   3. User redeems — adapter returns tokens to vault, vault forwards to user.
    function test_adapterPullPattern_depositAndRedeem() public {
        BasicInflowAdapter adapter = _deployAdapter(admin);

        vm.startPrank(admin);
        adapter.registerDepositor(address(vault), "");
        vault.registerAdapter("basic", address(adapter), true, false);
        vm.stopPrank();

        vm.startPrank(user);
        usdc.approve(address(vault), AMOUNT);
        uint256 sharesReceived = vault.deposit(AMOUNT, user);
        vm.stopPrank();

        assertEq(sharesReceived,                    AMOUNT, "1:1 shares minted");
        assertEq(usdc.balanceOf(address(vault)),    0,      "vault holds 0 - tokens routed to adapter");
        assertEq(usdc.balanceOf(address(adapter)),  AMOUNT, "adapter holds 100k USDC");
        assertEq(vault.totalAssets(),               AMOUNT, "totalAssets = 100k (via adapter position)");

        vm.startPrank(user);
        uint256 assetsReturned = vault.redeem(sharesReceived, user, user);
        vm.stopPrank();

        assertEq(assetsReturned,                    AMOUNT, "user receives 100k USDC back");
        assertEq(usdc.balanceOf(user),              AMOUNT, "user USDC balance restored");
        assertEq(usdc.balanceOf(address(adapter)),  0,      "adapter drained");
        assertEq(usdc.balanceOf(address(vault)),    0,      "vault holds 0");
        assertEq(vault.totalSupply(),               0,      "no shares outstanding");
        assertEq(vault.totalAssets(),               0,      "totalAssets = 0");
    }

    /// @notice Moves a non-deposit token (USDT) between two adapters via moveAdapterFundsToken.
    ///   1. Deploy USDT token and two adapters (both untracked).
    ///   2. Seed adapter A with USDT (simulates a Morpho position).
    ///   3. Call moveAdapterFundsToken → USDT moves to adapter B.
    ///   4. deployedAmount is unchanged (vault never held USDT).
    function test_moveAdapterFundsToken_nonDepositToken() public {
        MockERC20 usdt = new MockERC20("Tether USD", "USDT", 6);
        uint256 transferAmount = 50_000e6;

        BasicInflowAdapter adapterA = _deployAdapter(admin);
        BasicInflowAdapter adapterB = _deployAdapter(admin);

        vm.startPrank(admin);
        adapterA.registerDepositor(address(vault), "");
        adapterB.registerDepositor(address(vault), "");
        vault.registerAdapter("adapterA", address(adapterA), false, false);
        vault.registerAdapter("adapterB", address(adapterB), false, false);
        vm.stopPrank();

        usdt.mint(address(adapterA), transferAmount);

        uint256 deployedBefore = vault.deployedAmount();

        vm.prank(admin);
        vault.moveAdapterFundsToken("adapterA", "adapterB", transferAmount, address(usdt));

        assertEq(usdt.balanceOf(address(adapterA)), 0,              "adapterA: USDT drained");
        assertEq(usdt.balanceOf(address(adapterB)), transferAmount, "adapterB: USDT received");
        assertEq(vault.deployedAmount(),            deployedBefore, "deployedAmount unchanged");
    }

    /// @notice moveAdapterFundsToken reverts when adapters have different tracking modes.
    function test_moveAdapterFundsToken_trackingMismatch_reverts() public {
        MockERC20 usdt = new MockERC20("Tether USD", "USDT", 6);

        BasicInflowAdapter adapterA = _deployAdapter(admin);
        BasicInflowAdapter adapterB = _deployAdapter(admin);

        vm.startPrank(admin);
        adapterA.registerDepositor(address(vault), "");
        adapterB.registerDepositor(address(vault), "");
        vault.registerAdapter("trackedA",   address(adapterA), false, true);   // tracked
        vault.registerAdapter("untrackedB", address(adapterB), false, false);  // untracked
        vm.stopPrank();

        vm.expectRevert(
            abi.encodeWithSelector(
                InflowAdapterLib.AdapterTrackingMismatch.selector,
                "trackedA",
                "untrackedB"
            )
        );
        vm.prank(admin);
        vault.moveAdapterFundsToken("trackedA", "untrackedB", 1e6, address(usdt));
    }

    /// @notice moveAdapterFundsToken reverts when the caller passes the vault's deposit asset.
    function test_moveAdapterFundsToken_depositAsset_reverts() public {
        BasicInflowAdapter adapterA = _deployAdapter(admin);
        BasicInflowAdapter adapterB = _deployAdapter(admin);

        vm.startPrank(admin);
        adapterA.registerDepositor(address(vault), "");
        adapterB.registerDepositor(address(vault), "");
        vault.registerAdapter("adapterA", address(adapterA), false, false);
        vault.registerAdapter("adapterB", address(adapterB), false, false);
        vm.stopPrank();

        vm.expectRevert(InflowVault.TokenIsDepositAsset.selector);
        vm.prank(admin);
        vault.moveAdapterFundsToken("adapterA", "adapterB", 1e6, address(usdc));
    }
}
