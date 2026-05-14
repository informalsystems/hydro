// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {SafeERC20} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import {Initializable} from "@openzeppelin/contracts-upgradeable/proxy/utils/Initializable.sol";
import {UUPSUpgradeable} from "@openzeppelin/contracts/proxy/utils/UUPSUpgradeable.sol";
import {IAdapter} from "./IAdapter.sol";

/// @title BasicInflowAdapter
/// @notice A minimal IAdapter implementation that holds tokens directly without
/// deploying them to any external protocol. Useful for testing the vault ↔ adapter
/// integration and as a reference for building real adapters.
///
/// Design:
/// - Multi-token: a single deployment can serve vaults with different assets.
/// - Shared pool per token: no per-depositor accounting; the full token balance
///   is available to any registered and enabled depositor.
/// - Multi-admin: any admin can add/remove other admins; at least one must remain.
/// - UUPS upgradeable: upgrade authority is guarded by the admin list.
/// - Intended to be registered as "untracked" in the vault so totalAssets() is
///   calculated by querying depositorPosition() rather than deployedAmount.
contract BasicInflowAdapter is IAdapter, Initializable, UUPSUpgradeable {

    // ERRORS

    error Unauthorized();
    error AlreadyRegistered();
    error NotRegistered();
    error ZeroAddress();
    error AlreadyAdmin();
    error NotAdmin();
    error AdminListCannotBeEmpty();

    // EVENTS

    event DepositorRegistered(address indexed depositor);
    event DepositorUnregistered(address indexed depositor);
    event DepositorEnabled(address indexed depositor, bool enabled);
    event AdminAdded(address indexed admin);
    event AdminRemoved(address indexed admin);

    // STATE

    mapping(address => bool) private _isAdmin;
    address[] private _adminList;

    mapping(address => bool) private _registered;
    mapping(address => bool) private _enabled;
    address[] private _depositorList;

    // MODIFIERS

    modifier onlyAdmin() {
        if (!_isAdmin[msg.sender]) revert Unauthorized();
        _;
    }

    modifier onlyDepositor() {
        if (!_registered[msg.sender] || !_enabled[msg.sender]) revert Unauthorized();
        _;
    }

    // CONSTRUCTOR / INITIALIZER

    /// @custom:oz-upgrades-unsafe-allow constructor
    constructor() {
        _disableInitializers();
    }

    /// @param initialAdmins At least one admin must be provided. Duplicates are skipped.
    function initialize(address[] memory initialAdmins) external initializer {
        if (initialAdmins.length == 0) revert AdminListCannotBeEmpty();
        for (uint256 i = 0; i < initialAdmins.length; i++) {
            address a = initialAdmins[i];
            if (a == address(0)) revert ZeroAddress();
            if (!_isAdmin[a]) {
                _isAdmin[a] = true;
                _adminList.push(a);
            }
        }
    }

    // IADAPTER — WRITE

    /// @inheritdoc IAdapter
    function deposit(uint256 amount, address token) external onlyDepositor {
        SafeERC20.safeTransferFrom(IERC20(token), msg.sender, address(this), amount);
    }

    /// @inheritdoc IAdapter
    function withdraw(uint256 amount, address token) external onlyDepositor {
        SafeERC20.safeTransfer(IERC20(token), msg.sender, amount);
    }

    // IADAPTER — VIEWS

    /// @inheritdoc IAdapter
    function availableForDeposit(address depositor, address) external view returns (uint256) {
        if (!_registered[depositor] || !_enabled[depositor]) return 0;
        return type(uint256).max;
    }

    /// @inheritdoc IAdapter
    function availableForWithdraw(address depositor, address token) external view returns (uint256) {
        if (!_registered[depositor] || !_enabled[depositor]) return 0;
        return IERC20(token).balanceOf(address(this));
    }

    /// @inheritdoc IAdapter
    /// @dev Shared pool: returns the full token balance regardless of which depositor is queried.
    function depositorPosition(address, address token) external view returns (uint256) {
        return IERC20(token).balanceOf(address(this));
    }

    // IADAPTER — DEPOSITOR MANAGEMENT

    /// @inheritdoc IAdapter
    function registerDepositor(address depositor, bytes calldata) external onlyAdmin {
        if (depositor == address(0)) revert ZeroAddress();
        if (_registered[depositor]) revert AlreadyRegistered();
        _registered[depositor] = true;
        _enabled[depositor] = true;
        _depositorList.push(depositor);
        emit DepositorRegistered(depositor);
    }

    /// @inheritdoc IAdapter
    function unregisterDepositor(address depositor) external onlyAdmin {
        if (!_registered[depositor]) revert NotRegistered();
        _registered[depositor] = false;
        _enabled[depositor] = false;
        _removeFromDepositorList(depositor);
        emit DepositorUnregistered(depositor);
    }

    /// @inheritdoc IAdapter
    function setDepositorEnabled(address depositor, bool enabled) external onlyAdmin {
        if (!_registered[depositor]) revert NotRegistered();
        _enabled[depositor] = enabled;
        emit DepositorEnabled(depositor, enabled);
    }

    /// @inheritdoc IAdapter
    function isDepositorRegistered(address depositor) external view returns (bool) {
        return _registered[depositor];
    }

    /// @inheritdoc IAdapter
    function isDepositorEnabled(address depositor) external view returns (bool) {
        return _registered[depositor] && _enabled[depositor];
    }

    /// @notice Returns all registered depositor addresses.
    function depositors() external view returns (address[] memory) {
        return _depositorList;
    }

    // IADAPTER — ADMIN MANAGEMENT

    /// @inheritdoc IAdapter
    function addAdmin(address admin) external onlyAdmin {
        if (admin == address(0)) revert ZeroAddress();
        if (_isAdmin[admin]) revert AlreadyAdmin();
        _isAdmin[admin] = true;
        _adminList.push(admin);
        emit AdminAdded(admin);
    }

    /// @inheritdoc IAdapter
    function removeAdmin(address admin) external onlyAdmin {
        if (!_isAdmin[admin]) revert NotAdmin();
        if (_adminList.length == 1) revert AdminListCannotBeEmpty();
        _isAdmin[admin] = false;
        _removeFromAdminList(admin);
        emit AdminRemoved(admin);
    }

    /// @inheritdoc IAdapter
    function getAdmins() external view returns (address[] memory) {
        return _adminList;
    }

    /// @notice Returns true if `addr` is an admin.
    function isAdmin(address addr) external view returns (bool) {
        return _isAdmin[addr];
    }

    // UUPS

    function _authorizeUpgrade(address) internal view override {
        if (!_isAdmin[msg.sender]) revert Unauthorized();
    }

    // INTERNAL

    function _removeFromDepositorList(address depositor) private {
        _removeFromList(_depositorList, depositor);
    }

    function _removeFromAdminList(address admin) private {
        _removeFromList(_adminList, admin);
    }

    function _removeFromList(address[] storage list, address target) private {
        uint256 n = list.length;
        for (uint256 i = 0; i < n; i++) {
            if (list[i] == target) {
                list[i] = list[n - 1];
                list.pop();
                return;
            }
        }
    }
}
