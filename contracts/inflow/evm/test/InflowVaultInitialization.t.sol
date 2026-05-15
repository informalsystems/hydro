// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Test} from "forge-std/Test.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {ERC1967Proxy} from "@openzeppelin/contracts/proxy/ERC1967/ERC1967Proxy.sol";
import {InflowVault} from "../contracts/InflowVault.sol";
import {MockERC20} from "./mocks/Mocks.sol";

/// @notice Tests for InflowVault.initialize().
/// Corresponds to: test_instantiate_with_fee_config, test_instantiate_without_fee_config,
/// test_instantiate_invalid_fee_rate (control-center/testing_fees.rs).
contract InflowVaultInitializationTest is Test {
    MockERC20 internal asset;

    uint256 internal constant WAD             = 1e18;
    uint256 internal constant DEPOSIT_CAP     = 1_000_000e6;
    uint256 internal constant MAX_WITHDRAWALS = 10;

    address internal admin        = makeAddr("admin");
    address internal feeRecipient = makeAddr("feeRecipient");

    function setUp() public {
        asset = new MockERC20("USD Coin", "USDC", 6);
    }

    function _deploy(
        address[] memory whitelist,
        address[] memory daWhitelist,
        uint256 feeRate,
        address feeRec
    ) internal returns (InflowVault) {
        InflowVault impl = new InflowVault();
        bytes memory init = abi.encodeCall(
            InflowVault.initialize,
            (IERC20(address(asset)), "Hydro Inflow Vault", "hvUSDC",
             DEPOSIT_CAP, MAX_WITHDRAWALS, whitelist, daWhitelist, feeRate, feeRec)
        );
        return InflowVault(address(new ERC1967Proxy(address(impl), init)));
    }

    function _defaultWhitelist() internal view returns (address[] memory wl) {
        wl = new address[](1);
        wl[0] = admin;
    }

    // ── stored values ─────────────────────────────────────────────────────────

    function test_initialize_stores_asset_and_name() public {
        InflowVault v = _deploy(_defaultWhitelist(), _defaultWhitelist(), 0, address(0));
        assertEq(v.asset(), address(asset));
        assertEq(v.name(),   "Hydro Inflow Vault");
        assertEq(v.symbol(), "hvUSDC");
        assertEq(v.decimals(), asset.decimals());
    }

    function test_initialize_sets_deposit_cap() public {
        InflowVault v = _deploy(_defaultWhitelist(), _defaultWhitelist(), 0, address(0));
        assertEq(v.depositCap(), DEPOSIT_CAP);
    }

    function test_initialize_sets_max_withdrawals() public {
        InflowVault v = _deploy(_defaultWhitelist(), _defaultWhitelist(), 0, address(0));
        assertEq(v.maxWithdrawalsPerUser(), MAX_WITHDRAWALS);
    }

    function test_initialize_with_fee_config() public {
        uint256 rate = WAD / 10; // 10 %
        InflowVault v = _deploy(_defaultWhitelist(), _defaultWhitelist(), rate, feeRecipient);
        assertEq(v.feeRate(),            rate);
        assertEq(v.feeRecipient(),       feeRecipient);
        assertEq(v.highWaterMarkPrice(), WAD, "HWM should start at 1.0 WAD");
    }

    function test_initialize_without_fee_config() public {
        InflowVault v = _deploy(_defaultWhitelist(), _defaultWhitelist(), 0, address(0));
        assertEq(v.feeRate(),            0);
        assertEq(v.feeRecipient(),       address(0));
        assertEq(v.highWaterMarkPrice(), WAD, "HWM should start at 1.0 WAD");
    }

    function test_initialize_whitelist_members_stored() public {
        address second = makeAddr("second");
        address[] memory wl = new address[](2);
        wl[0] = admin;
        wl[1] = second;
        InflowVault v = _deploy(wl, _defaultWhitelist(), 0, address(0));
        assertTrue(v.whitelist(admin),  "admin should be whitelisted");
        assertTrue(v.whitelist(second), "second should be whitelisted");
    }

    // ── revert cases ──────────────────────────────────────────────────────────

    function test_initialize_invalid_fee_rate_reverts() public {
        InflowVault impl = new InflowVault();
        bytes memory init = abi.encodeCall(
            InflowVault.initialize,
            (IERC20(address(asset)), "Vault", "V",
             DEPOSIT_CAP, MAX_WITHDRAWALS,
             _defaultWhitelist(), _defaultWhitelist(),
             WAD + 1, feeRecipient)
        );
        vm.expectRevert(InflowVault.InvalidFeeRate.selector);
        new ERC1967Proxy(address(impl), init);
    }

    function test_initialize_nonzero_fee_rate_without_recipient_reverts() public {
        InflowVault impl = new InflowVault();
        bytes memory init = abi.encodeCall(
            InflowVault.initialize,
            (IERC20(address(asset)), "Vault", "V",
             DEPOSIT_CAP, MAX_WITHDRAWALS,
             _defaultWhitelist(), _defaultWhitelist(),
             WAD / 10, address(0))
        );
        vm.expectRevert(InflowVault.FeeRecipientNotSet.selector);
        new ERC1967Proxy(address(impl), init);
    }

    function test_initialize_empty_whitelist_reverts() public {
        InflowVault impl = new InflowVault();
        address[] memory empty = new address[](0);
        bytes memory init = abi.encodeCall(
            InflowVault.initialize,
            (IERC20(address(asset)), "Vault", "V",
             DEPOSIT_CAP, MAX_WITHDRAWALS,
             empty, _defaultWhitelist(), 0, address(0))
        );
        vm.expectRevert(InflowVault.WhitelistCannotBeEmpty.selector);
        new ERC1967Proxy(address(impl), init);
    }

    function test_initialize_empty_deployed_amount_whitelist_reverts() public {
        InflowVault impl = new InflowVault();
        address[] memory empty = new address[](0);
        bytes memory init = abi.encodeCall(
            InflowVault.initialize,
            (IERC20(address(asset)), "Vault", "V",
             DEPOSIT_CAP, MAX_WITHDRAWALS,
             _defaultWhitelist(), empty, 0, address(0))
        );
        vm.expectRevert(InflowVault.DeployedAmountWhitelistCannotBeEmpty.selector);
        new ERC1967Proxy(address(impl), init);
    }

    function test_initialize_zero_address_in_whitelist_reverts() public {
        InflowVault impl = new InflowVault();
        address[] memory wl = new address[](2);
        wl[0] = admin;
        wl[1] = address(0);
        bytes memory init = abi.encodeCall(
            InflowVault.initialize,
            (IERC20(address(asset)), "Vault", "V",
             DEPOSIT_CAP, MAX_WITHDRAWALS,
             wl, _defaultWhitelist(), 0, address(0))
        );
        vm.expectRevert(InflowVault.ZeroAddress.selector);
        new ERC1967Proxy(address(impl), init);
    }

    function test_initialize_duplicate_whitelist_entries_deduped() public {
        address[] memory wl = new address[](3);
        wl[0] = admin;
        wl[1] = admin; // duplicate
        wl[2] = admin; // duplicate
        InflowVault v = _deploy(wl, _defaultWhitelist(), 0, address(0));
        assertTrue(v.whitelist(admin), "admin should be whitelisted");
        // If we can add admin again it would revert with AlreadyWhitelisted, meaning count == 1.
        vm.prank(admin);
        vm.expectRevert(InflowVault.AlreadyWhitelisted.selector);
        v.addToWhitelist(admin);
    }

    function test_cannot_initialize_twice() public {
        InflowVault v = _deploy(_defaultWhitelist(), _defaultWhitelist(), 0, address(0));
        vm.expectRevert();
        v.initialize(
            IERC20(address(asset)), "Vault", "V",
            DEPOSIT_CAP, MAX_WITHDRAWALS,
            _defaultWhitelist(), _defaultWhitelist(), 0, address(0)
        );
    }
}
