// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import "forge-std/Script.sol";
import {ERC1967Proxy} from "@openzeppelin/contracts/proxy/ERC1967/ERC1967Proxy.sol";
import {BasicInflowAdapter} from "../contracts/BasicInflowAdapter.sol";
import {InflowVault} from "../contracts/InflowVault.sol";

/// @notice Deploys BasicInflowAdapter implementation + ERC1967 proxy, then wires it
/// into an existing InflowVault in a single broadcast:
///   1. Deploy implementation
///   2. Deploy proxy (calls initialize)
///   3. registerDepositor(vault) on the adapter
///   4. registerAdapter("basic", proxy, automated=true, tracked=false) on the vault
///
/// Required env vars:
///   VAULT_ADDRESS   - Address of the existing InflowVault proxy
///   ADAPTER_ADMIN   - Initial admin of the adapter (typically the deployer)
///   PRIVATE_KEY     - Signing key
///   RPC_URL         - Node endpoint
///
/// Example:
///   forge script script/DeployBasicAdapter.s.sol \
///     --rpc-url $RPC_URL --broadcast --private-key $PRIVATE_KEY -vvvv
contract DeployBasicAdapter is Script {
    bytes32 private constant IMPL_SLOT =
        bytes32(uint256(keccak256("eip1967.proxy.implementation")) - 1);

    function run() external {
        address vault        = vm.envAddress("VAULT_ADDRESS");
        address adapterAdmin = vm.envAddress("ADAPTER_ADMIN");

        address[] memory admins = new address[](1);
        admins[0] = adapterAdmin;

        bytes memory initData = abi.encodeCall(BasicInflowAdapter.initialize, (admins));

        vm.startBroadcast();

        BasicInflowAdapter impl = new BasicInflowAdapter();
        address proxy = address(new ERC1967Proxy(address(impl), initData));

        BasicInflowAdapter(proxy).registerDepositor(vault, "");
        InflowVault(vault).registerAdapter("basic", proxy, true, false);

        vm.stopBroadcast();

        address implAddr = address(uint160(uint256(vm.load(proxy, IMPL_SLOT))));
        console2.log("Proxy:         ", proxy);
        console2.log("Implementation:", implAddr);
        console2.log("Vault:         ", vault);
    }
}
