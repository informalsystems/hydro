// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {SafeERC20} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import {IAdapter} from "./IAdapter.sol";

/// @title InflowAdapterLib
/// @notice Manages the adapter registry, allocation logic, and deployed-amount tracking
/// for InflowVault. All external functions execute via DELEGATECALL in the vault's storage
/// context, so state changes and events are attributed to the vault.
library InflowAdapterLib {

    // STRUCTS

    struct AdapterInfo {
        address addr;
        bool automated;
        bool tracked;
        string name;
        string description;
    }

    /// @dev All adapter-related state packed into one struct so it can be passed as a
    /// single storage pointer to library functions.
    struct AdapterStorage {
        mapping(bytes32 => AdapterInfo) adapters;
        bytes32[] adapterKeys;
    }

    // ERRORS

    error AdapterAlreadyExists(string name);
    error AdapterNotFound(string name);

    // MANAGEMENT

    function registerAdapter(
        AdapterStorage storage s,
        string calldata name,
        address addr,
        bool automated,
        bool tracked,
        string calldata description
    ) external {
        bytes32 key = keccak256(bytes(name));
        if (s.adapters[key].addr != address(0)) revert AdapterAlreadyExists(name);
        s.adapters[key] = AdapterInfo({
            addr: addr,
            automated: automated,
            tracked: tracked,
            name: name,
            description: description
        });
        s.adapterKeys.push(key);
    }

    /// @return removedAddr Passed back to the vault so it can emit AdapterUnregistered.
    function unregisterAdapter(AdapterStorage storage s, string calldata name)
        external returns (address removedAddr)
    {
        bytes32 key = keccak256(bytes(name));
        AdapterInfo storage a = s.adapters[key];
        if (a.addr == address(0)) revert AdapterNotFound(name);
        removedAddr = a.addr;
        delete s.adapters[key];
        _removeAdapterKey(s, key);
    }

    function setAdapterAllocationMode(
        AdapterStorage storage s,
        string calldata name,
        bool automated
    ) external {
        bytes32 key = keccak256(bytes(name));
        if (s.adapters[key].addr == address(0)) revert AdapterNotFound(name);
        s.adapters[key].automated = automated;
    }

    function setAdapterDeploymentTracking(
        AdapterStorage storage s,
        string calldata name,
        bool tracked
    ) external {
        bytes32 key = keccak256(bytes(name));
        if (s.adapters[key].addr == address(0)) revert AdapterNotFound(name);
        s.adapters[key].tracked = tracked;
    }

    function withdrawFromAdapter(
        AdapterStorage storage s,
        string calldata name,
        uint256 amount,
        uint256 deployedAmount
    ) external returns (uint256) {
        bytes32 key = keccak256(bytes(name));
        AdapterInfo storage a = s.adapters[key];
        if (a.addr == address(0)) revert AdapterNotFound(name);
        IAdapter(a.addr).withdraw(amount);
        if (a.tracked) {
            deployedAmount = deployedAmount > amount ? deployedAmount - amount : 0;
        }
        return deployedAmount;
    }

    function depositToAdapter(
        AdapterStorage storage s,
        string calldata name,
        uint256 amount,
        address assetAddr,
        uint256 deployedAmount
    ) external returns (uint256) {
        bytes32 key = keccak256(bytes(name));
        AdapterInfo storage a = s.adapters[key];
        if (a.addr == address(0)) revert AdapterNotFound(name);
        SafeERC20.safeTransfer(IERC20(assetAddr), a.addr, amount);
        IAdapter(a.addr).deposit(amount);
        if (a.tracked) deployedAmount += amount;
        return deployedAmount;
    }

    function moveAdapterFunds(
        AdapterStorage storage s,
        string calldata fromName,
        string calldata toName,
        uint256 amount,
        address assetAddr,
        uint256 deployedAmount
    ) external returns (uint256) {
        bytes32 fromKey = keccak256(bytes(fromName));
        bytes32 toKey = keccak256(bytes(toName));
        AdapterInfo storage fromAdapter = s.adapters[fromKey];
        AdapterInfo storage toAdapter = s.adapters[toKey];
        if (fromAdapter.addr == address(0)) revert AdapterNotFound(fromName);
        if (toAdapter.addr == address(0)) revert AdapterNotFound(toName);

        IAdapter(fromAdapter.addr).withdraw(amount);
        SafeERC20.safeTransfer(IERC20(assetAddr), toAdapter.addr, amount);
        IAdapter(toAdapter.addr).deposit(amount);

        if (fromAdapter.tracked && !toAdapter.tracked) {
            deployedAmount = deployedAmount > amount ? deployedAmount - amount : 0;
        } else if (!fromAdapter.tracked && toAdapter.tracked) {
            deployedAmount += amount;
        }
        return deployedAmount;
    }

    // ALLOCATION

    /// @dev Greedily allocates `amount` across automated adapters in registration order.
    /// Returns parallel arrays of adapter keys and allocated amounts; their sum may be less
    /// than `amount` if adapter capacity is insufficient.
    function calculateAllocation(
        AdapterStorage storage s,
        uint256 amount,
        bool isDeposit,
        address assetAddr
    ) external view returns (bytes32[] memory keys, uint256[] memory amounts) {
        return _calculateAllocation(s, amount, isDeposit, assetAddr);
    }

    /// @dev Allocates `amount` to automated adapters, executes transfers and adapter deposits,
    /// and returns the updated deployedAmount (incremented for tracked adapters).
    function allocateToAdapters(
        AdapterStorage storage s,
        uint256 amount,
        address assetAddr,
        uint256 deployedAmount
    ) external returns (uint256) {
        (bytes32[] memory keys, uint256[] memory amounts) = _calculateAllocation(s, amount, true, assetAddr);
        uint256 trackedTotal;
        for (uint256 i = 0; i < keys.length; i++) {
            AdapterInfo storage a = s.adapters[keys[i]];
            SafeERC20.safeTransfer(IERC20(assetAddr), a.addr, amounts[i]);
            IAdapter(a.addr).deposit(amounts[i]);
            if (a.tracked) trackedTotal += amounts[i];
        }
        return deployedAmount + trackedTotal;
    }

    /// @dev Executes adapter withdrawals from a pre-computed allocation and adjusts
    /// deployedAmount for tracked adapters. Called from the vault's _withdraw on the
    /// immediate-fulfilment path.
    function executeAdapterWithdrawals(
        AdapterStorage storage s,
        bytes32[] memory keys,
        uint256[] memory amounts,
        uint256 deployedAmount
    ) external returns (uint256) {
        uint256 trackedAmount;
        for (uint256 i = 0; i < keys.length; i++) {
            AdapterInfo storage a = s.adapters[keys[i]];
            IAdapter(a.addr).withdraw(amounts[i]);
            if (a.tracked) trackedAmount += amounts[i];
        }
        return deployedAmount > trackedAmount ? deployedAmount - trackedAmount : 0;
    }

    // VIEWS

    /// @dev Sums positions from all untracked adapters. Tracked adapter positions are
    /// already counted in deployedAmount and must not be queried to avoid double-counting.
    function queryUntrackedPositions(AdapterStorage storage s, address assetAddr)
        external view returns (uint256 total)
    {
        for (uint256 i = 0; i < s.adapterKeys.length; i++) {
            AdapterInfo storage a = s.adapters[s.adapterKeys[i]];
            if (a.tracked) continue;
            try IAdapter(a.addr).depositorPosition(address(this), assetAddr) returns (uint256 pos) {
                total += pos;
            } catch {}
        }
    }

    function getAdapters(AdapterStorage storage s) external view returns (AdapterInfo[] memory result) {
        result = new AdapterInfo[](s.adapterKeys.length);
        for (uint256 i = 0; i < s.adapterKeys.length; i++) {
            result[i] = s.adapters[s.adapterKeys[i]];
        }
    }

    function getAdapterByName(AdapterStorage storage s, string calldata name)
        external view returns (AdapterInfo memory)
    {
        bytes32 key = keccak256(bytes(name));
        AdapterInfo storage a = s.adapters[key];
        if (a.addr == address(0)) revert AdapterNotFound(name);
        return a;
    }

    // INTERNAL HELPERS

    function _calculateAllocation(
        AdapterStorage storage s,
        uint256 amount,
        bool isDeposit,
        address assetAddr
    ) internal view returns (bytes32[] memory keys, uint256[] memory amounts) {
        if (amount == 0) return (new bytes32[](0), new uint256[](0));

        uint256 n = s.adapterKeys.length;
        bytes32[] memory tempKeys = new bytes32[](n);
        uint256[] memory tempAmounts = new uint256[](n);
        uint256 count;
        uint256 remaining = amount;

        for (uint256 i = 0; i < n && remaining > 0; i++) {
            bytes32 key = s.adapterKeys[i];
            AdapterInfo storage a = s.adapters[key];
            if (!a.automated) continue;

            uint256 available = isDeposit
                ? IAdapter(a.addr).availableForDeposit(address(this), assetAddr)
                : IAdapter(a.addr).availableForWithdraw(address(this), assetAddr);

            if (available == 0) continue;

            uint256 alloc = available < remaining ? available : remaining;
            tempKeys[count] = key;
            tempAmounts[count] = alloc;
            count++;
            remaining -= alloc;
        }

        keys = new bytes32[](count);
        amounts = new uint256[](count);
        for (uint256 i = 0; i < count; i++) {
            keys[i] = tempKeys[i];
            amounts[i] = tempAmounts[i];
        }
    }

    function _removeAdapterKey(AdapterStorage storage s, bytes32 key) internal {
        uint256 n = s.adapterKeys.length;
        for (uint256 i = 0; i < n; i++) {
            if (s.adapterKeys[i] == key) {
                s.adapterKeys[i] = s.adapterKeys[n - 1];
                s.adapterKeys.pop();
                return;
            }
        }
    }
}
