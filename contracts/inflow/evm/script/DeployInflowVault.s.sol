// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Script, console2} from "forge-std/Script.sol";
import {ERC1967Proxy} from "@openzeppelin/contracts/proxy/ERC1967/ERC1967Proxy.sol";
import {InflowVault} from "../contracts/InflowVault.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";

/// @notice Deploys InflowVault implementation + ERC1967 proxy and initializes it.
///
/// Required env vars:
///   ASSET                           - ERC-20 token address accepted as deposit
///   VAULT_NAME                      - Share token name
///   VAULT_SYMBOL                    - Share token symbol
///   DEPOSIT_CAP                     - Max total assets (token base units)
///   MAX_WITHDRAWALS_PER_USER        - Max concurrent queued withdrawals per address
///   INITIAL_ADMIN                   - Initial whitelisted address (deployer or multisig)
///
/// Optional env vars (default to 0 / address(0)):
///   INITIAL_DEPLOYED_AMOUNT_ADMIN   - Initial deployed-amount whitelisted address; defaults to INITIAL_ADMIN
///   FEE_RATE                        - Performance fee rate in WAD (0 = disabled, 1e18 = 100%)
///   FEE_RECIPIENT                   - Fee recipient; required when FEE_RATE > 0
///
/// Example:
///   forge script script/DeployInflowVault.s.sol \
///     --rpc-url $RPC_URL --broadcast --private-key $PRIVATE_KEY -vvvv
contract DeployInflowVault is Script {
    bytes32 private constant IMPL_SLOT = bytes32(uint256(keccak256("eip1967.proxy.implementation")) - 1);

    function run() external {
        address asset = vm.envAddress("ASSET");
        string memory name = vm.envString("VAULT_NAME");
        string memory symbol = vm.envString("VAULT_SYMBOL");
        uint256 depositCap = vm.envUint("DEPOSIT_CAP");
        uint256 maxWithdrawals = vm.envUint("MAX_WITHDRAWALS_PER_USER");
        address initialAdmin = vm.envAddress("INITIAL_ADMIN");
        address initialDeployedAmountAdmin = vm.envOr("INITIAL_DEPLOYED_AMOUNT_ADMIN", initialAdmin);
        uint256 feeRate = vm.envOr("FEE_RATE", uint256(0));
        address feeRecipient = vm.envOr("FEE_RECIPIENT", address(0));

        address[] memory whitelist = new address[](1);
        whitelist[0] = initialAdmin;

        address[] memory deployedAmountWhitelist = new address[](1);
        deployedAmountWhitelist[0] = initialDeployedAmountAdmin;

        bytes memory initData = abi.encodeCall(
            InflowVault.initialize,
            (
                IERC20(asset),
                name,
                symbol,
                depositCap,
                maxWithdrawals,
                whitelist,
                deployedAmountWhitelist,
                feeRate,
                feeRecipient
            )
        );

        vm.startBroadcast();
        InflowVault impl = new InflowVault();
        address proxy = address(new ERC1967Proxy(address(impl), initData));
        vm.stopBroadcast();

        address implAddr = address(uint160(uint256(vm.load(proxy, IMPL_SLOT))));
        console2.log("Proxy:         ", proxy);
        console2.log("Implementation:", implAddr);
    }
}
