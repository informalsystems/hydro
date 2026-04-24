// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/// @title IAdapter
/// @notice Interface that all Inflow protocol adapters must implement.
/// Adapters wrap external yield protocols (lending, staking, etc.) and expose
/// a uniform deposit/withdraw surface to the InflowVault.
interface IAdapter {
    /// @notice Deposit `amount` of the vault's asset token into the underlying protocol.
    /// The vault transfers tokens to the adapter address before calling this function.
    /// @param amount Amount of asset tokens already transferred to this adapter.
    function deposit(uint256 amount) external;

    /// @notice Withdraw `amount` of the vault's asset token from the underlying protocol
    /// back to the caller (the vault). MUST revert if the requested amount cannot be
    /// fully withdrawn.
    /// @param amount Amount of asset tokens to withdraw.
    function withdraw(uint256 amount) external;

    /// @notice Returns the maximum amount the vault can still deposit into this adapter.
    /// Takes into account any adapter-level caps or liquidity limits.
    /// @param depositor Address of the vault querying availability.
    /// @param token     Address of the asset token.
    function availableForDeposit(address depositor, address token) external view returns (uint256);

    /// @notice Returns the maximum amount the vault can immediately withdraw from this adapter.
    /// @param depositor Address of the vault querying availability.
    /// @param token     Address of the asset token.
    function availableForWithdraw(address depositor, address token) external view returns (uint256);

    /// @notice Returns the current balance / position held by `depositor` in this adapter
    /// for `token`. Used by the vault to calculate totalAssets() for untracked adapters.
    /// @param depositor Address of the vault.
    /// @param token     Address of the asset token.
    function depositorPosition(address depositor, address token) external view returns (uint256);
}
