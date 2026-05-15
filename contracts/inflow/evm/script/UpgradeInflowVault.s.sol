// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Script, console2} from  "forge-std/Script.sol";
import {InflowVault} from "../contracts/InflowVault.sol";

/// @notice Deploys a new InflowVault implementation and upgrades the existing proxy to it.
///
/// The broadcaster must be whitelisted on the proxy (_authorizeUpgrade enforces this).
///
/// Required env vars:
///   PROXY           - Address of the existing ERC1967 proxy
///
/// Optional env vars:
///   MIGRATION_DATA  - ABI-encoded calldata forwarded to upgradeToAndCall (e.g. a
///                     reinitializer call). Defaults to empty (no migration call).
///
/// Example (no migration):
///   forge script script/UpgradeInflowVault.s.sol \
///     --rpc-url $RPC_URL --broadcast --private-key $PRIVATE_KEY -vvvv
///
/// Example (with migration function):
///   export MIGRATION_DATA=$(cast calldata "migrateV2(uint256)" 42)
///   forge script script/UpgradeInflowVault.s.sol \
///     --rpc-url $RPC_URL --broadcast --private-key $PRIVATE_KEY -vvvv
contract UpgradeInflowVault is Script {
    bytes32 private constant IMPL_SLOT =
        bytes32(uint256(keccak256("eip1967.proxy.implementation")) - 1);

    function run() external {
        address proxy          = vm.envAddress("PROXY");
        bytes memory migration = vm.envOr("MIGRATION_DATA", new bytes(0));

        vm.startBroadcast();
        InflowVault newImpl = new InflowVault();
        InflowVault(proxy).upgradeToAndCall(address(newImpl), migration);
        vm.stopBroadcast();

        address implAddr = address(uint160(uint256(vm.load(proxy, IMPL_SLOT))));
        console2.log("Proxy:         ", proxy);
        console2.log("Implementation:", implAddr);
    }
}
