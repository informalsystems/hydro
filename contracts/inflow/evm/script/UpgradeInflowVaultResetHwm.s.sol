// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Script, console2} from "forge-std/Script.sol";
import {Math} from "@openzeppelin/contracts/utils/math/Math.sol";
import {InflowVault} from "../contracts/InflowVault.sol";

/// @notice Prepares the upgrade of a version-1 InflowVault proxy to the current implementation
/// with the one-shot resetHighWaterMark() call, for execution by a whitelisted Safe.
///
/// The script never calls the proxy on-chain. It simulates the Safe's upgradeToAndCall
/// against the RPC state, checks the outcome, and prints the transaction for the Safe to sign.
/// Run without --broadcast it sends nothing at all (dry run). The only transaction it can
/// ever broadcast is the deployment of the new implementation, and only when
/// DEPLOY_IMPLEMENTATION=true is set and --broadcast is passed.
///
/// Required env vars:
///   PROXY                  - Address of the existing ERC1967 proxy
///   ADMIN_SAFE             - Whitelisted address that will execute upgradeToAndCall
///
/// Optional env vars:
///   IMPLEMENTATION         - Already deployed new implementation. When set, nothing is
///                            deployed and the printed calldata is final.
///   DEPLOY_IMPLEMENTATION  - Set to true to broadcast the implementation deployment
///                            (requires --broadcast). Defaults to false.
///
/// Example (dry run, nothing is sent; the implementation address is only a simulation):
///   forge script script/UpgradeInflowVaultResetHwm.s.sol --rpc-url $RPC_URL -vvvv
///
/// Example (print the final Safe transaction for an already deployed implementation):
///   export IMPLEMENTATION=0xNewImplementationAddress
///   forge script script/UpgradeInflowVaultResetHwm.s.sol --rpc-url $RPC_URL -vvvv
contract UpgradeInflowVaultResetHwm is Script {
    bytes32 private constant IMPL_SLOT = bytes32(uint256(keccak256("eip1967.proxy.implementation")) - 1);
    uint256 private constant WAD = 1e18;

    function run() external {
        prepare(
            vm.envAddress("PROXY"),
            vm.envAddress("ADMIN_SAFE"),
            vm.envOr("IMPLEMENTATION", address(0)),
            vm.envOr("DEPLOY_IMPLEMENTATION", false)
        );
    }

    /// @notice Resolves the new implementation (given, broadcast, or simulated), simulates
    /// the upgrade and prints the Safe transaction.
    /// @return upgradeCalldata The calldata `adminSafe` must send to `proxy`.
    function prepare(address proxy, address adminSafe, address implementation, bool deployImplementation)
        public
        returns (bytes memory upgradeCalldata)
    {
        bool finalAddress = implementation != address(0);
        if (!finalAddress) {
            if (deployImplementation) {
                vm.startBroadcast();
                implementation = address(new InflowVault());
                vm.stopBroadcast();
            } else {
                implementation = address(new InflowVault());
            }
        }

        upgradeCalldata = simulateUpgrade(proxy, adminSafe, implementation);

        console2.log("");
        console2.log("Safe transaction");
        console2.log("  to:        ", proxy);
        console2.log("  value:      0");
        console2.log("  operation:  0 (CALL)");
        console2.log("  data:");
        console2.logBytes(upgradeCalldata);
        if (!finalAddress && !deployImplementation) {
            console2.log("");
            console2.log("DRY RUN: the implementation above exists only in this simulation.");
            console2.log("Deploy it, then re-run with IMPLEMENTATION set to obtain the final data.");
        }
    }

    /// @notice Simulates `adminSafe` calling upgradeToAndCall(implementation, resetHighWaterMark())
    /// on `proxy`, reverting if any pre- or post-condition fails.
    /// @return upgradeCalldata The calldata `adminSafe` must send to `proxy`.
    function simulateUpgrade(address proxy, address adminSafe, address implementation)
        public
        returns (bytes memory upgradeCalldata)
    {
        InflowVault vault = InflowVault(proxy);

        require(implementation.code.length > 0, "implementation has no code");
        require(vault.whitelist(adminSafe), "ADMIN_SAFE is not whitelisted on the proxy");
        require(_implementation(proxy) != implementation, "proxy already uses this implementation");

        uint256 oldHighWaterMarkPrice = vault.highWaterMarkPrice();
        uint256 totalAssets = vault.totalAssets();
        uint256 totalSupply = vault.totalSupply();
        uint256 feeRate = vault.feeRate();
        address feeRecipient = vault.feeRecipient();
        uint256 sharePrice = Math.mulDiv(totalAssets, WAD, totalSupply);
        require(sharePrice < oldHighWaterMarkPrice, "share price is not below the high-water mark");

        console2.log("Proxy:                  ", proxy);
        console2.log("Admin Safe:             ", adminSafe);
        console2.log("Current implementation: ", _implementation(proxy));
        console2.log("New implementation:     ", implementation);
        console2.log("totalAssets:            ", totalAssets);
        console2.log("totalSupply:            ", totalSupply);
        console2.log("Share price (WAD):      ", sharePrice);
        console2.log("HWM before (WAD):       ", oldHighWaterMarkPrice);

        upgradeCalldata = abi.encodeCall(
            vault.upgradeToAndCall, (implementation, abi.encodeCall(InflowVault.resetHighWaterMark, ()))
        );

        vm.prank(adminSafe);
        (bool success, bytes memory returndata) = proxy.call(upgradeCalldata);
        if (!success) {
            assembly {
                revert(add(returndata, 0x20), mload(returndata))
            }
        }

        require(_implementation(proxy) == implementation, "implementation not switched");
        require(vault.highWaterMarkPrice() == sharePrice, "high-water mark not reset to the share price");
        require(vault.totalAssets() == totalAssets, "totalAssets changed");
        require(vault.totalSupply() == totalSupply, "totalSupply changed");
        require(vault.feeRate() == feeRate, "feeRate changed");
        require(vault.feeRecipient() == feeRecipient, "feeRecipient changed");
        require(vault.whitelist(adminSafe), "ADMIN_SAFE lost its whitelist entry");

        vm.prank(adminSafe);
        try vault.resetHighWaterMark() {
            revert("resetHighWaterMark is still callable");
        } catch {}

        console2.log("HWM after (WAD):        ", vault.highWaterMarkPrice());
        console2.log("Simulation OK: implementation switched, HWM reset, reset no longer callable.");
    }

    function _implementation(address proxy) private view returns (address) {
        return address(uint160(uint256(vm.load(proxy, IMPL_SLOT))));
    }
}
