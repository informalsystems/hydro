// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {ERC20} from "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {SafeERC20} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";

// ─────────────────────────────────────────────────────────────────────────────
// MockERC20
// ─────────────────────────────────────────────────────────────────────────────

contract MockERC20 is ERC20 {
    uint8 private _dec;

    constructor(string memory name_, string memory symbol_, uint8 decimals_)
        ERC20(name_, symbol_)
    {
        _dec = decimals_;
    }

    function decimals() public view override returns (uint8) { return _dec; }
    function mint(address to, uint256 amount) external { _mint(to, amount); }
}

// ─────────────────────────────────────────────────────────────────────────────
// MockAdapterWithAsset
// ─────────────────────────────────────────────────────────────────────────────

/// @dev Minimal IAdapter for testing.
///
/// The vault transfers `amount` tokens to this address before calling deposit().
/// On withdraw(), this adapter sends `amount` tokens back to msg.sender (the vault).
///
/// Capacity is controlled per depositor via setAvailableForDeposit /
/// setAvailableForWithdraw. Position is the adapter's live token balance.
contract MockAdapterWithAsset {
    address public immutable ASSET;

    bool public revertOnDeposit;
    bool public revertOnWithdraw;

    mapping(address => uint256) private _depositCap;
    mapping(address => uint256) private _withdrawCap;

    constructor(address asset_) {
        ASSET = asset_;
    }

    // ── test helpers ─────────────────────────────────────────────────────────

    /// @dev Set how much `depositor` can deposit.
    function setDepositCap(address depositor, uint256 amount) external {
        _depositCap[depositor] = amount;
    }

    /// @dev Set how much `depositor` can withdraw.
    function setWithdrawCap(address depositor, uint256 amount) external {
        _withdrawCap[depositor] = amount;
    }

    function setRevertOnDeposit(bool flag) external { revertOnDeposit = flag; }
    function setRevertOnWithdraw(bool flag) external { revertOnWithdraw = flag; }

    // ── IAdapter ─────────────────────────────────────────────────────────────

    function deposit(uint256 amount, address token) external {
        require(!revertOnDeposit, "MockAdapter: deposit reverted");
        SafeERC20.safeTransferFrom(IERC20(token), msg.sender, address(this), amount);
    }

    function withdraw(uint256 amount, address token) external {
        require(!revertOnWithdraw, "MockAdapter: withdraw reverted");
        SafeERC20.safeTransfer(IERC20(token), msg.sender, amount);
    }

    function availableForDeposit(address depositor, address) external view returns (uint256) {
        return _depositCap[depositor];
    }

    function availableForWithdraw(address depositor, address) external view returns (uint256) {
        return _withdrawCap[depositor];
    }

    function depositorPosition(address, address token) external view returns (uint256) {
        return IERC20(token).balanceOf(address(this));
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ShortfallAdapter
// ─────────────────────────────────────────────────────────────────────────────

/// @dev Adapter that returns normally on withdraw() but delivers fewer tokens than
/// requested (simulates fee deductions or rounding without an explicit revert).
contract ShortfallAdapter {
    address public immutable ASSET;
    uint256 public shortfall; // tokens withheld on each withdraw

    constructor(address asset_, uint256 shortfall_) {
        ASSET = asset_;
        shortfall = shortfall_;
    }

    function deposit(uint256 amount, address token) external {
        SafeERC20.safeTransferFrom(IERC20(token), msg.sender, address(this), amount);
    }

    function withdraw(uint256 amount, address token) external {
        uint256 actual = amount > shortfall ? amount - shortfall : 0;
        SafeERC20.safeTransfer(IERC20(token), msg.sender, actual);
    }

    function availableForDeposit(address, address) external pure returns (uint256) {
        return type(uint256).max;
    }

    function availableForWithdraw(address, address token) external view returns (uint256) {
        return IERC20(token).balanceOf(address(this));
    }

    function depositorPosition(address, address token) external view returns (uint256) {
        return IERC20(token).balanceOf(address(this));
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// RevertingAvailabilityAdapter
// ─────────────────────────────────────────────────────────────────────────────

/// @dev Adapter whose availableForDeposit and availableForWithdraw always revert.
/// Used to verify that _calculateAllocation isolates per-adapter failures.
contract RevertingAvailabilityAdapter {
    address public immutable ASSET;

    constructor(address asset_) {
        ASSET = asset_;
    }

    function deposit(uint256 amount, address token) external {
        SafeERC20.safeTransferFrom(IERC20(token), msg.sender, address(this), amount);
    }

    function withdraw(uint256 amount, address token) external {
        SafeERC20.safeTransfer(IERC20(token), msg.sender, amount);
    }

    function availableForDeposit(address, address) external pure returns (uint256) {
        revert("adapter unavailable");
    }

    function availableForWithdraw(address, address) external pure returns (uint256) {
        revert("adapter unavailable");
    }

    function depositorPosition(address, address token) external view returns (uint256) {
        return IERC20(token).balanceOf(address(this));
    }
}
