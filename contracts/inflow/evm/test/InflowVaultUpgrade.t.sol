// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Test} from "forge-std/Test.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {ERC1967Proxy} from "@openzeppelin/contracts/proxy/ERC1967/ERC1967Proxy.sol";
import {InflowVault} from "../contracts/InflowVault.sol";
import {MockERC20} from "./mocks/Mocks.sol";

/// @notice Tests for UUPS upgrade authorization.
/// No direct CosmWasm counterpart; covers _authorizeUpgrade().
contract InflowVaultUpgradeTest is Test {
    MockERC20   internal asset;
    InflowVault internal vault;

    address internal admin    = makeAddr("admin");
    address internal stranger = makeAddr("stranger");
    address internal user     = makeAddr("user");

    uint256 internal constant DEPOSIT_CAP     = 1_000_000e6;
    uint256 internal constant MAX_WITHDRAWALS = 10;

    function setUp() public {
        asset = new MockERC20("USD Coin", "USDC", 6);

        address[] memory wl = new address[](1);
        wl[0] = admin;

        InflowVault impl = new InflowVault();
        bytes memory init = abi.encodeCall(
            InflowVault.initialize,
            (IERC20(address(asset)), "Hydro Inflow Vault", "hvUSDC",
             DEPOSIT_CAP, MAX_WITHDRAWALS, wl, wl, 0, address(0))
        );
        vault = InflowVault(address(new ERC1967Proxy(address(impl), init)));
    }

    // ── upgrade succeeds for whitelisted address ──────────────────────────────

    function test_upgrade_authorized_by_whitelisted() public {
        InflowVault newImpl = new InflowVault();

        vm.prank(admin);
        vault.upgradeToAndCall(address(newImpl), "");

        // After upgrade the vault should still work — totalAssets returns 0.
        assertEq(vault.totalAssets(), 0);
    }

    // ── upgrade reverts for non-whitelisted ───────────────────────────────────

    function test_upgrade_unauthorized_reverts() public {
        InflowVault newImpl = new InflowVault();

        vm.prank(stranger);
        vm.expectRevert(InflowVault.Unauthorized.selector);
        vault.upgradeToAndCall(address(newImpl), "");
    }

    // ── state preserved after upgrade ────────────────────────────────────────

    function test_state_preserved_after_upgrade() public {
        // Deposit 100k, record state.
        asset.mint(user, 100_000e6);
        vm.prank(user);
        asset.approve(address(vault), 100_000e6);
        vm.prank(user);
        vault.deposit(100_000e6, user);

        vm.prank(admin);
        vault.withdrawForDeployment(50_000e6);
        vm.prank(admin);
        vault.submitDeployedAmount(60_000e6);

        uint256 sharesBefore     = vault.balanceOf(user);
        uint256 totalAssetBefore = vault.totalAssets();
        uint256 deployedBefore   = vault.deployedAmount();
        uint256 capBefore        = vault.depositCap();

        // Upgrade.
        InflowVault newImpl = new InflowVault();
        vm.prank(admin);
        vault.upgradeToAndCall(address(newImpl), "");

        // All state intact.
        assertEq(vault.balanceOf(user), sharesBefore,     "shares preserved");
        assertEq(vault.totalAssets(),   totalAssetBefore, "totalAssets preserved");
        assertEq(vault.deployedAmount(), deployedBefore,  "deployedAmount preserved");
        assertEq(vault.depositCap(),    capBefore,        "depositCap preserved");
        assertTrue(vault.whitelist(admin),                "whitelist preserved");
    }
}
