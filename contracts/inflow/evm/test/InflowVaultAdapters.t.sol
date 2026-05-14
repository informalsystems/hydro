// SPDX-License-Identifier: MIT
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
