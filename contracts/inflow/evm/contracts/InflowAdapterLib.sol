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
    error SameAdapter(string name);
    error AdapterPositionNotEmpty(string name);
    error AdapterTrackingMismatch(string fromAdapter, string toAdapter);
    error AdapterWithdrawShortfall(string name, uint256 requested, uint256 received);

    // MANAGEMENT

    function registerAdapter(
        AdapterStorage storage s,
        string calldata name,
        address addr,
        bool automated,
        bool tracked
    ) external {
        bytes32 key = keccak256(bytes(name));
        if (s.adapters[key].addr != address(0)) revert AdapterAlreadyExists(name);
        s.adapters[key] = AdapterInfo({
            addr: addr,
            automated: automated,
            tracked: tracked,
            name: name
        });
        s.adapterKeys.push(key);
    }

    /// @return removedAddr Passed back to the vault so it can emit AdapterUnregistered.
    function unregisterAdapter(AdapterStorage storage s, string calldata name, address assetAddr)
        external returns (address removedAddr)
    {
        bytes32 key = keccak256(bytes(name));
        AdapterInfo storage a = s.adapters[key];
        if (a.addr == address(0)) revert AdapterNotFound(name);
        // Require the position to be zero before removal
        uint256 position = IAdapter(a.addr).depositorPosition(address(this), assetAddr);
        if (position != 0) revert AdapterPositionNotEmpty(name);
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
        address assetAddr,
        uint256 deployedAmount
    ) external returns (uint256) {
        bytes32 key = keccak256(bytes(name));
        AdapterInfo storage a = s.adapters[key];
        if (a.addr == address(0)) revert AdapterNotFound(name);
        _withdrawChecked(a, amount, assetAddr);
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

        IERC20 assetERC20 = IERC20(assetAddr);
        IAdapter adapter = IAdapter(a.addr);
        uint256 availableForDeposit = adapter.availableForDeposit(address(this), assetAddr);
        if (availableForDeposit == 0) return deployedAmount;

        uint256 balanceBefore = assetERC20.balanceOf(address(this));
        // Cap the deposit to the adapter's available capacity.
        if (availableForDeposit < amount) amount = availableForDeposit;
        SafeERC20.forceApprove(assetERC20, a.addr, amount);
        adapter.deposit(amount, assetAddr);
        // Revoke any unconsumed approval so it cannot be used in a later transaction.
        // A well-behaved adapter pulls exactly `amount`, but a partial deposit (e.g. a
        // capacity-limited adapter that only absorbs part of the allowance) would leave
        // a residual allowance on the vault that must not persist.
        SafeERC20.forceApprove(assetERC20, a.addr, 0);
        uint256 balanceAfter = assetERC20.balanceOf(address(this));
        uint256 actualDeposit = balanceBefore > balanceAfter ? balanceBefore - balanceAfter : 0;
        if (a.tracked) deployedAmount += actualDeposit;
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
        if (fromKey == toKey) revert SameAdapter(fromName);
        AdapterInfo storage fromAdapter = s.adapters[fromKey];
        AdapterInfo storage toAdapter = s.adapters[toKey];
        if (fromAdapter.addr == address(0)) revert AdapterNotFound(fromName);
        if (toAdapter.addr == address(0)) revert AdapterNotFound(toName);

        _withdrawChecked(fromAdapter, amount, assetAddr);
        SafeERC20.forceApprove(IERC20(assetAddr), toAdapter.addr, amount);
        IAdapter(toAdapter.addr).deposit(amount, assetAddr);
        SafeERC20.forceApprove(IERC20(assetAddr), toAdapter.addr, 0); // revoke any unconsumed approval

        if (fromAdapter.tracked && !toAdapter.tracked) {
            deployedAmount = deployedAmount > amount ? deployedAmount - amount : 0;
        } else if (!fromAdapter.tracked && toAdapter.tracked) {
            deployedAmount += amount;
        }
        return deployedAmount;
    }

    /// @dev Moves `amount` of an arbitrary `tokenAddr` from one adapter to another.
    /// Unlike moveAdapterFunds, deployedAmount is never adjusted because the token is
    /// not in the vault's primary denomination. Both adapters must have the same
    /// deployment-tracking mode to prevent silent accounting corruption.
    function moveAdapterFundsToken(
        AdapterStorage storage s,
        string calldata fromName,
        string calldata toName,
        uint256 amount,
        address tokenAddr
    ) external {
        bytes32 fromKey = keccak256(bytes(fromName));
        bytes32 toKey   = keccak256(bytes(toName));
        if (fromKey == toKey) revert SameAdapter(fromName);
        AdapterInfo storage fromAdapter = s.adapters[fromKey];
        AdapterInfo storage toAdapter   = s.adapters[toKey];
        if (fromAdapter.addr == address(0)) revert AdapterNotFound(fromName);
        if (toAdapter.addr   == address(0)) revert AdapterNotFound(toName);

        // Moving a non-deposit token across a tracking boundary would corrupt deployedAmount
        // because we cannot convert the token to the vault's denomination without an oracle.
        if (fromAdapter.tracked != toAdapter.tracked)
            revert AdapterTrackingMismatch(fromName, toName);

        _withdrawChecked(fromAdapter, amount, tokenAddr);
        SafeERC20.forceApprove(IERC20(tokenAddr), toAdapter.addr, amount);
        IAdapter(toAdapter.addr).deposit(amount, tokenAddr);
        SafeERC20.forceApprove(IERC20(tokenAddr), toAdapter.addr, 0); // revoke any unconsumed approval
        // deployedAmount is intentionally not modified
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
            SafeERC20.forceApprove(IERC20(assetAddr), a.addr, amounts[i]);
            IAdapter(a.addr).deposit(amounts[i], assetAddr);
            SafeERC20.forceApprove(IERC20(assetAddr), a.addr, 0); // revoke any unconsumed approval
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
        uint256 deployedAmount,
        address assetAddr
    ) external returns (uint256) {
        uint256 trackedAmount;
        for (uint256 i = 0; i < keys.length; i++) {
            AdapterInfo storage a = s.adapters[keys[i]];
            _withdrawChecked(a, amounts[i], assetAddr);
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

            uint256 available;
            if (isDeposit) {
                try IAdapter(a.addr).availableForDeposit(address(this), assetAddr) returns (uint256 v) {
                    available = v;
                } catch { continue; }
            } else {
                try IAdapter(a.addr).availableForWithdraw(address(this), assetAddr) returns (uint256 v) {
                    available = v;
                } catch { continue; }
            }
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

    /// @dev Calls adapter.withdraw() and verifies the vault actually received at least `amount` of tokens.
    /// Using >= rather than == so adapters that legitimately deliver slightly more (e.g. accrued interest) are not rejected.
    function _withdrawChecked(AdapterInfo storage a, uint256 amount, address tokenAddr) private {
        uint256 before = IERC20(tokenAddr).balanceOf(address(this));
        IAdapter(a.addr).withdraw(amount, tokenAddr);
        uint256 received = IERC20(tokenAddr).balanceOf(address(this)) - before;
        if (received < amount) revert AdapterWithdrawShortfall(a.name, amount, received);
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
