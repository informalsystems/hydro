// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {ERC20} from "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {IAdapter} from "../../contracts/IAdapter.sol";

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
contract MockAdapterWithAsset is IAdapter {
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

    function deposit(uint256) external view override {
        require(!revertOnDeposit, "MockAdapter: deposit reverted");
        // Tokens already transferred to this contract by the vault.
    }

    function withdraw(uint256 amount) external override {
        require(!revertOnWithdraw, "MockAdapter: withdraw reverted");
        bool success = IERC20(ASSET).transfer(msg.sender, amount);
        require(success, "MockAdapter: transfer failed");
    }

    function availableForDeposit(address depositor, address) external view override returns (uint256) {
        return _depositCap[depositor];
    }

    function availableForWithdraw(address depositor, address) external view override returns (uint256) {
        return _withdrawCap[depositor];
    }

    function depositorPosition(address, address token) external view override returns (uint256) {
        return IERC20(token).balanceOf(address(this));
    }
}
