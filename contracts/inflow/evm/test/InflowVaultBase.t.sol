// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Test} from "forge-std/Test.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {ERC1967Proxy} from "@openzeppelin/contracts/proxy/ERC1967/ERC1967Proxy.sol";
import {InflowVault} from "../contracts/InflowVault.sol";
import {MockERC20, MockAdapterWithAsset} from "./mocks/Mocks.sol";

abstract contract InflowVaultBase is Test {
    // ── actors ───────────────────────────────────────────────────────────────

    address internal admin = makeAddr("admin");
    address internal user = makeAddr("user");
    address internal alice = makeAddr("alice");
    address internal bob = makeAddr("bob");
    address internal feeRecipient = makeAddr("feeRecipient");
    address internal stranger = makeAddr("stranger");

    // ── contracts ────────────────────────────────────────────────────────────

    InflowVault internal vault;
    MockERC20 internal asset;

    // ── constants ────────────────────────────────────────────────────────────

    uint256 internal constant DEPOSIT_CAP = 1_000_000e6;
    uint256 internal constant MAX_WITHDRAWALS = 10;
    uint256 internal constant WAD = 1e18;

    // ── setup ─────────────────────────────────────────────────────────────────

    function setUp() public virtual {
        asset = new MockERC20("USD Coin", "USDC", 6);
        vault = _deployVault(0, address(0));
    }

    /// @dev Deploy a fresh vault proxy. feeRate_ = 0 disables fees.
    function _deployVault(uint256 feeRate_, address feeRecipient_) internal returns (InflowVault) {
        address[] memory wl = new address[](1);
        wl[0] = admin;

        address[] memory daWl = new address[](1);
        daWl[0] = admin;

        InflowVault impl = new InflowVault();
        bytes memory init = abi.encodeCall(
            InflowVault.initialize,
            (
                IERC20(address(asset)),
                "Hydro Inflow Vault",
                "hvUSDC",
                DEPOSIT_CAP,
                MAX_WITHDRAWALS,
                wl,
                daWl,
                feeRate_,
                feeRecipient_
            )
        );
        return InflowVault(address(new ERC1967Proxy(address(impl), init)));
    }

    /// @dev Deploy a vault with fees enabled.
    function _deployVaultWithFees(uint256 feeRate_) internal returns (InflowVault) {
        return _deployVault(feeRate_, feeRecipient);
    }

    // ── helpers ───────────────────────────────────────────────────────────────

    function _mintAndApprove(address who, uint256 amount) internal {
        asset.mint(who, amount);
        vm.prank(who);
        asset.approve(address(vault), amount);
    }

    function _mintAndApproveVault(InflowVault v, address who, uint256 amount) internal {
        asset.mint(who, amount);
        vm.prank(who);
        asset.approve(address(v), amount);
    }

    function _deposit(address who, uint256 amount) internal returns (uint256 shares) {
        _mintAndApprove(who, amount);
        vm.prank(who);
        return vault.deposit(amount, who);
    }

    function _depositTo(address who, address receiver, uint256 amount) internal returns (uint256 shares) {
        _mintAndApprove(who, amount);
        vm.prank(who);
        return vault.deposit(amount, receiver);
    }

    /// @dev Register a MockAdapterWithAsset on `vault` (called as admin).
    function _registerAdapter(string memory name, MockAdapterWithAsset adapter, bool automated, bool tracked) internal {
        vm.prank(admin);
        vault.registerAdapter(name, address(adapter), automated, tracked);
    }

    /// @dev Register a MockAdapterWithAsset on a specific vault (called as admin).
    function _registerAdapterOn(
        InflowVault v,
        string memory name,
        MockAdapterWithAsset adapter,
        bool automated,
        bool tracked
    ) internal {
        vm.prank(admin);
        v.registerAdapter(name, address(adapter), automated, tracked);
    }
}
