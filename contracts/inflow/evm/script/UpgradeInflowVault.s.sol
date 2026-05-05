// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "forge-std/Script.sol";
import {Upgrades} from "openzeppelin-foundry-upgrades/Upgrades.sol";

contract UpgradeInflowVault is Script {
    function run() external {
        address proxy = vm.envAddress("PROXY");

        vm.startBroadcast();
        Upgrades.upgradeProxy(proxy, "InflowVault.sol", "");
        vm.stopBroadcast();

        console2.log("Proxy:          ", proxy);
        console2.log("Implementation: ", Upgrades.getImplementationAddress(proxy));
    }
}
