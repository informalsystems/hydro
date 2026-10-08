// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {SafeERC20} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import {Initializable} from "@openzeppelin/contracts-upgradeable/proxy/utils/Initializable.sol";
import {UUPSUpgradeable} from "@openzeppelin/contracts/proxy/utils/UUPSUpgradeable.sol";
import {IAdapter} from "./IAdapter.sol";

/// @title ReserveAdapter
/// @notice A shared backstop reserve that one or more vaults draw on to smooth reported APY:
/// funds move into the reserve when a vault's raw yield for a period is above target and back
/// out when it is below target, so reported growth stays steadier over time.
///
/// Design:
/// - Shared pool: no per-depositor accounting, so any registered and enabled depositor can
///   access the full balance. Unlike BasicInflowAdapter this is intentional, since several
///   vaults holding the same asset are meant to draw on one reserve.
/// - Multi-token: a single deployment can serve vaults with different assets.
/// - Multi-admin: any admin can add or remove other admins; at least one must remain.
/// - UUPS upgradeable: upgrade authority is guarded by the admin list.
///
/// @dev OPERATIONAL CONTRACT, safety-critical and not enforced on-chain.
///
/// Register this adapter with `tracked = true` on every vault that uses it. The vault's
/// `depositToAdapter` / `withdrawFromAdapter` then adjust `deployedAmount` by the moved amount
/// unconditionally, from the vault's adapter library. Parked reserve funds must not count toward
/// the share ratio in either direction, so the operator MUST follow every reserve transfer with a
/// `submitDeployedAmount` call that cancels that adjustment back out, leaving only the true value
/// of the vault's real tracked venues. Omitting or miscomputing it inflates or deflates
/// `totalAssets()` until corrected, with no on-chain guard.
///
/// `depositorPosition()` is hardcoded to 0 (see below). That is what makes this adapter always
/// disconnectable via `vault.unregisterAdapter(...)` regardless of real balance, and it is also why
/// that function's position guard offers no protection here: reconciling real funds against
/// `deployedAmount` before disconnecting is entirely an operational responsibility.
contract ReserveAdapter is IAdapter, Initializable, UUPSUpgradeable {
    // ERRORS

    error Unauthorized();
    error AlreadyRegistered();
    error NotRegistered();
    error ZeroAddress();
    error AlreadyAdmin();
    error NotAdmin();
    error AdminListCannotBeEmpty();
    error PositionNotEmpty();

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
        _onlyAdmin();
        _;
    }

    modifier onlyDepositor() {
        _onlyDepositor();
        _;
    }

    function _onlyAdmin() internal view {
        if (!_isAdmin[msg.sender]) revert Unauthorized();
    }

    function _onlyDepositor() internal view {
        if (!_registered[msg.sender] || !_enabled[msg.sender]) revert Unauthorized();
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

    // IADAPTER - WRITE

    /// @inheritdoc IAdapter
    function deposit(uint256 amount, address token) external onlyDepositor {
        SafeERC20.safeTransferFrom(IERC20(token), msg.sender, address(this), amount);
    }

    /// @inheritdoc IAdapter
    function withdraw(uint256 amount, address token) external onlyDepositor {
        SafeERC20.safeTransfer(IERC20(token), msg.sender, amount);
    }

    // IADAPTER - VIEWS

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
    /// @dev Deliberately always 0, whatever the real balance. Not a balance query; see the
    /// contract NatSpec for the reconciliation duty this imposes on the operator.
    function depositorPosition(address, address) external pure returns (uint256) {
        return 0;
    }

    // IADAPTER - DEPOSITOR MANAGEMENT

    /// @inheritdoc IAdapter
    /// @dev The `metadata` parameter is unused: this adapter has no per-depositor configuration.
    function registerDepositor(address depositor, bytes calldata) external onlyAdmin {
        if (depositor == address(0)) revert ZeroAddress();
        if (_registered[depositor]) revert AlreadyRegistered();
        _registered[depositor] = true;
        _enabled[depositor] = true;
        _depositorList.push(depositor);
        emit DepositorRegistered(depositor);
    }

    /// @inheritdoc IAdapter
    /// @dev The position check can never revert, since `depositorPosition()` is hardcoded to 0. It is
    /// kept for interface symmetry with BasicInflowAdapter; a shared multi-token pool has no
    /// well-defined per-depositor position to guard on.
    function unregisterDepositor(address depositor) external onlyAdmin {
        if (!_registered[depositor]) revert NotRegistered();
        if (this.depositorPosition(depositor, address(0)) != 0) revert PositionNotEmpty();
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

    // IADAPTER - ADMIN MANAGEMENT

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
