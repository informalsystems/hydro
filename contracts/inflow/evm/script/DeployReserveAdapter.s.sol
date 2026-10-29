// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Script, console2} from "forge-std/Script.sol";
import {ERC1967Proxy} from "@openzeppelin/contracts/proxy/ERC1967/ERC1967Proxy.sol";
import {ReserveAdapter} from "../contracts/ReserveAdapter.sol";
import {InflowVault} from "../contracts/InflowVault.sol";
import {InflowAdapterLib} from "../contracts/InflowAdapterLib.sol";

/// @notice Deploys a ReserveAdapter implementation + ERC1967 proxy, initialized with
/// `[ADAPTER_ADMIN]`. One script for every environment: the target is selected entirely
/// through env vars, so there is no per-environment deploy code to keep in sync.
///
/// `WIRE_MODE` picks how the vault <-> adapter handshake happens, and is the only real
/// difference between environments:
///   safe     - deploy only. The handshake runs later from the admin Safe, because the
///              vault whitelist holds only that Safe and a deployer EOA call would revert.
///              Generate the batch with bin/03_safe_tx_link_adapter.sh.
///   deployer - deploy and wire in the same broadcast. Only possible where the deployer
///              EOA is itself vault-whitelisted, which in practice means test networks.
///
/// The adapter is always registered with `tracked = true`. Read the ReserveAdapter NatSpec
/// before operating it: every reserve transfer needs a compensating `submitDeployedAmount`.
///
/// Required env vars:
///   VAULT_ADDRESS     - InflowVault proxy to attach to
///   ADAPTER_ADMIN     - initial adapter admin; the admin Safe when WIRE_MODE=safe
///   EXPECTED_CHAIN_ID - chain the deploy is intended for; a mismatch aborts
///   DEPLOYER          - broadcasting address; required only when WIRE_MODE=deployer
///
/// Optional env vars:
///   WIRE_MODE    - "safe" (default) or "deployer"
///   ADAPTER_NAME - vault-side adapter name (default "reserve")
///
/// Prefer bin/02_deploy_reserve_adapter.sh, which sets all of these from .env.<target>.
contract DeployReserveAdapter is Script {
    bytes32 private constant IMPL_SLOT = bytes32(uint256(keccak256("eip1967.proxy.implementation")) - 1);

    function run() external {
        address vault = vm.envAddress("VAULT_ADDRESS");
        address adapterAdmin = vm.envAddress("ADAPTER_ADMIN");
        uint256 expectedChainId = vm.envUint("EXPECTED_CHAIN_ID");
        string memory adapterName = vm.envOr("ADAPTER_NAME", string("reserve"));
        string memory wireMode = vm.envOr("WIRE_MODE", string("safe"));

        bool wireFromDeployer = keccak256(bytes(wireMode)) == keccak256(bytes("deployer"));
        require(
            wireFromDeployer || keccak256(bytes(wireMode)) == keccak256(bytes("safe")),
            "WIRE_MODE must be 'safe' or 'deployer'"
        );

        // Everything below runs before startBroadcast, so a bad target fails without spending gas.

        require(block.chainid == expectedChainId, "chain id does not match EXPECTED_CHAIN_ID");
        require(vault.code.length > 0, "VAULT_ADDRESS has no code");
        require(adapterAdmin != address(0), "ADAPTER_ADMIN is zero");

        // Whoever performs the handshake must already be vault-whitelisted, or the wiring
        // step is guaranteed to revert later.
        address wiringActor = wireFromDeployer ? vm.envAddress("DEPLOYER") : adapterAdmin;
        require(InflowVault(vault).whitelist(wiringActor), "wiring address is not vault-whitelisted");

        try InflowVault(vault).getAdapterByName(adapterName) returns (InflowAdapterLib.AdapterInfo memory existing) {
            console2.log("Adapter already registered under this name:", existing.addr);
            revert("vault already has an adapter under ADAPTER_NAME; unregister it first");
        } catch {
            // AdapterNotFound, which is the expected state.
        }

        address[] memory admins = new address[](1);
        admins[0] = adapterAdmin;

        vm.startBroadcast();

        ReserveAdapter impl = new ReserveAdapter();
        address proxy = address(new ERC1967Proxy(address(impl), abi.encodeCall(ReserveAdapter.initialize, (admins))));

        if (wireFromDeployer) {
            ReserveAdapter(proxy).registerDepositor(vault, "");
            InflowVault(vault).registerAdapter(adapterName, proxy, false, true);
        }

        vm.stopBroadcast();

        console2.log("Adapter proxy: ", proxy);
        console2.log("Implementation:", address(uint160(uint256(vm.load(proxy, IMPL_SLOT)))));
        console2.log("Adapter admin: ", adapterAdmin);
        console2.log("Vault:         ", vault);

        if (wireFromDeployer) {
            console2.log("Wired to the vault in this broadcast.");
        } else {
            console2.log("NOT wired to the vault yet. Next:");
            console2.log("  ./bin/03_safe_tx_link_adapter.sh --env <target>", vault, proxy);
        }
    }
}
