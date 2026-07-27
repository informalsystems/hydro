// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/// @title IAdapter
/// @notice Interface that all Inflow protocol adapters must implement.
/// Adapters wrap external yield protocols (lending, staking, etc.) and expose
/// a uniform deposit/withdraw surface to the InflowVault.
///
/// Multi-token: a single adapter deployment can serve multiple vaults with different assets.
/// The token address is passed explicitly on every call so the adapter can route accordingly.
interface IAdapter {
    /// @notice Deposit `amount` of `token` into the underlying protocol.
    /// The vault transfers `amount` tokens to the adapter before calling this function.
    /// MUST revert if `msg.sender` is not a registered and enabled depositor.
    /// @param amount Amount of tokens already transferred to this adapter.
    /// @param token  Address of the asset token transferred.
    function deposit(uint256 amount, address token) external;

    /// @notice Withdraw `amount` of `token` from the underlying protocol back to the caller.
    /// MUST revert if `msg.sender` is not a registered and enabled depositor.
    /// MUST revert if the requested amount cannot be fully withdrawn.
    /// @param amount Amount of tokens to withdraw.
    /// @param token  Address of the asset token to withdraw.
    function withdraw(uint256 amount, address token) external;

    /// @notice Returns the maximum amount the vault can still deposit into this adapter.
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

    // ── Depositor management (admin-only on implementing contract) ────────────────

    /// @notice Register a new depositor. Must revert if already registered.
    /// @param depositor Address to register (typically the vault address).
    /// @param metadata  Optional adapter-specific configuration encoded as raw bytes.
    function registerDepositor(address depositor, bytes calldata metadata) external;

    /// @notice Remove a depositor registration. Should revert if the depositor still
    /// holds a non-zero position in this adapter.
    function unregisterDepositor(address depositor) external;

    /// @notice Enable or disable a registered depositor without removing the registration.
    function setDepositorEnabled(address depositor, bool enabled) external;

    /// @notice Returns true if `depositor` has been registered.
    function isDepositorRegistered(address depositor) external view returns (bool);

    /// @notice Returns true if `depositor` is registered and currently enabled.
    function isDepositorEnabled(address depositor) external view returns (bool);

    // ── Admin management ──────────────────────────────────────────────────────────

    /// @notice Add a new admin. Must revert if `admin` is already an admin.
    function addAdmin(address admin) external;

    /// @notice Remove an admin. Must revert if it would leave the admin list empty.
    function removeAdmin(address admin) external;

    /// @notice Returns all current admin addresses.
    function getAdmins() external view returns (address[] memory);
}
