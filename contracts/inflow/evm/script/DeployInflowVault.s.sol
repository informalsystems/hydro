// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "forge-std/Script.sol";
import {Upgrades} from "openzeppelin-foundry-upgrades/Upgrades.sol";
import {InflowVault} from "../contracts/InflowVault.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";

contract DeployInflowVault is Script {
    function run() external {
        address asset          = vm.envAddress("ASSET");
        string memory name     = vm.envString("VAULT_NAME");
        string memory symbol   = vm.envString("VAULT_SYMBOL");
        uint256 depositCap     = vm.envUint("DEPOSIT_CAP");
        uint256 maxWithdrawals = vm.envUint("MAX_WITHDRAWALS_PER_USER");
        address initialAdmin   = vm.envAddress("INITIAL_ADMIN");
        uint256 feeRate        = vm.envOr("FEE_RATE", uint256(0));
        address feeRecipient   = vm.envOr("FEE_RECIPIENT", address(0));

        address[] memory whitelist = new address[](1);
        whitelist[0] = initialAdmin;

        vm.startBroadcast();

        address proxy = Upgrades.deployUUPSProxy(
            "InflowVault.sol",
            abi.encodeCall(
                InflowVault.initialize,
                (IERC20(asset), name, symbol, depositCap, maxWithdrawals,
                 whitelist, feeRate, feeRecipient)
            )
        );

        vm.stopBroadcast();

        console2.log("Proxy:", proxy);
        console2.log("Implementation:", Upgrades.getImplementationAddress(proxy));
    }
}
