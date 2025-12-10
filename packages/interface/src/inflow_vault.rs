use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Order, Timestamp, Uint128};

#[cw_serde]
pub struct Config {
    /// Token denom that users can deposit into the vault.
    pub deposit_denom: String,
    /// Denom of the vault shares token that is issued to users when they deposit tokens into the vault.
    pub vault_shares_denom: String,
    /// Address of the Control Center contract.
    pub control_center_contract: Addr,
    /// Address of the token info provider contract used to obtain the ratio of the
    /// deposit token to the base token. If None, then the deposit token is the base token.
    pub token_info_provider_contract: Option<Addr>,
    /// Maximum number of pending withdrawal requests allowed per user.
    pub max_withdrawals_per_user: u64,
}

#[cw_serde]
pub struct WithdrawalQueueInfo {
    // Total shares burned for all withdrawals currently in the withdrawal queue.
    pub total_shares_burned: Uint128,
    // Total amount to be withdrawn for all withdrawal entries currently in the withdrawal queue.
    // Sum of all `amount_to_receive` fields in all withdrawal entries, regardless of whether the
    // funds for payouts have been provided to the smart contract or not.
    pub total_withdrawal_amount: Uint128,
    // Sum of `amount_to_receive` fields of all withdrawal entries from the withdrawal queue
    // for which the funds have not been provided yet.
    pub non_funded_withdrawal_amount: Uint128,
}

#[cw_serde]
pub struct WithdrawalEntry {
    pub id: u64,
    pub initiated_at: Timestamp,
    pub withdrawer: Addr,
    pub shares_burned: Uint128,
    pub amount_to_receive: Uint128,
    pub is_funded: bool,
}
#[cw_serde]
pub struct PayoutEntry {
    pub id: u64,
    pub executed_at: Timestamp,
    pub recipient: Addr,
    pub vault_shares_burned: Uint128,
    pub amount_received: Uint128,
}

/// Controls whether an adapter participates in automated allocation
#[cw_serde]
pub enum AllocationMode {
    /// Adapter is included in automated deposit/withdrawal allocation via calculate_venues_allocation
    Automated,
    /// Adapter is only accessible via manual DepositToAdapter/WithdrawFromAdapter operations
    Manual,
}

/// Controls whether adapter operations update the Control Center's deployed amount
///
/// # Race Condition Warnings
///
/// ## Automated + Tracked (DANGEROUS)
/// Using `Tracked` with `AllocationMode::Automated` creates a race condition:
/// 1. Automated deposits/withdrawals update deployed amount without admin knowledge
/// 2. If a manual `SubmitDeployedAmount` call is in progress, it may overwrite with stale data
/// 3. Result: Deployed amount becomes inaccurate
///
/// **Recommendation**: Use `NotTracked` for automated adapters unless absolutely necessary.
///
/// ## Manual + Tracked
/// When using `Tracked` with `AllocationMode::Manual`:
/// - Ensure no `SubmitDeployedAmount` proposal is pending before manual operations
/// - After manual deposit/withdraw, re-snapshot deployed amount if a proposal was in flight
/// - Otherwise, the proposal will overwrite the auto-updated value with stale data
#[cw_serde]
pub enum DeploymentTracking {
    /// Deposits/withdrawals update Control Center's deployed amount
    /// WARNING: See race condition notes above
    Tracked,
    /// Position is queryable but not included in deployed amount
    /// RECOMMENDED for automated adapters
    NotTracked,
}

/// Information about a registered adapter
#[cw_serde]
pub struct AdapterInfo {
    /// Contract address of the adapter
    pub address: Addr,
    /// Controls whether adapter participates in automated deposit/withdrawal allocation.
    /// When Manual, the adapter is skipped by calculate_venues_allocation.
    /// Admin operations (DepositToAdapter, WithdrawFromAdapter) work regardless of this setting.
    pub allocation_mode: AllocationMode,
    /// Controls whether deposits/withdrawals to/from this adapter update Control Center's deployed amount.
    /// When Tracked, operations call AddToDeployedAmount/SubFromDeployedAmount.
    /// When NotTracked, position is queryable via DepositorPosition but not in deployed amount.
    pub deployment_tracking: DeploymentTracking,
    /// Human-readable name for display purposes
    pub name: String,
    /// Optional description of the adapter and what protocol it integrates with
    pub description: Option<String>,
}

#[cw_serde]
pub enum ExecuteMsg {
    Deposit {
        on_behalf_of: Option<String>,
    },
    Withdraw {
        on_behalf_of: Option<String>,
    },
    CancelWithdrawal {
        withdrawal_ids: Vec<u64>,
    },
    FulfillPendingWithdrawals {
        limit: u64,
    },
    ClaimUnbondedWithdrawals {
        withdrawal_ids: Vec<u64>,
    },
    WithdrawForDeployment {
        amount: Uint128,
    },
    DepositFromDeployment {},
    SetTokenInfoProviderContract {
        address: Option<String>,
    },
    AddToWhitelist {
        address: String,
    },
    RemoveFromWhitelist {
        address: String,
    },
    UpdateConfig {
        config: UpdateConfigData,
    },

    /// Register a new adapter for protocol integrations
    RegisterAdapter {
        name: String,
        address: String,
        description: Option<String>,
        /// Controls whether adapter participates in automated allocation
        allocation_mode: AllocationMode,
        /// Controls whether operations update Control Center's deployed amount
        deployment_tracking: DeploymentTracking,
    },
    /// Unregister an existing adapter
    UnregisterAdapter {
        name: String,
    },
    /// Set adapter's allocation mode (whitelisted only)
    SetAdapterAllocationMode {
        name: String,
        allocation_mode: AllocationMode,
    },
    /// Set adapter's deployment tracking mode (whitelisted only)
    SetAdapterDeploymentTracking {
        name: String,
        deployment_tracking: DeploymentTracking,
    },
    /// Withdraw funds from an adapter to the vault contract (whitelisted only)
    /// Funds stay in contract until withdraw_for_deployment is called
    WithdrawFromAdapter {
        adapter_name: String,
        amount: Uint128,
    },
    /// Deposit funds from vault contract balance to an adapter (whitelisted only)
    /// Used for manual rebalancing between adapters
    DepositToAdapter {
        adapter_name: String,
        amount: Uint128,
    },
}

#[cw_serde]
pub struct UpdateConfigData {
    pub max_withdrawals_per_user: Option<u64>,
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(ConfigResponse)]
    Config {},

    #[returns(PoolInfoResponse)]
    PoolInfo {},

    #[returns(Uint128)]
    SharesEquivalentValue { shares: Uint128 },

    #[returns(Uint128)]
    UserSharesEquivalentValue { address: String },

    /// Returns the number of tokens that are available for deployment,
    /// considering the amount required for any pending withdrawals.
    #[returns(Uint128)]
    AvailableForDeployment {},

    #[returns(WithdrawalQueueInfoResponse)]
    WithdrawalQueueInfo {},

    /// Number of tokens that needs to be provided to the contract in order
    /// to fund all pending withdrawal requests that have not been funded yet.
    #[returns(Uint128)]
    AmountToFundPendingWithdrawals {},

    /// Returns a list of withdrawal request IDs that have been marked as funded
    /// and are ready to be paid out to users, but have not been paid out yet.
    #[returns(FundedWithdrawalRequestsResponse)]
    FundedWithdrawalRequests { limit: u64 },

    /// Returns a list of withdrawal requests made by the given user.
    #[returns(UserWithdrawalRequestsResponse)]
    UserWithdrawalRequests {
        address: String,
        start_from: u32,
        limit: u32,
    },

    /// Returns a list of executed payouts for the given user.
    #[returns(UserPayoutsHistoryResponse)]
    UserPayoutsHistory {
        address: String,
        start_from: u32,
        limit: u32,
        order: Order,
    },

    #[returns(WhitelistResponse)]
    Whitelist {},

    /// Returns a list of all registered adapters
    #[returns(AdaptersListResponse)]
    ListAdapters {},

    /// Returns information about a specific adapter
    #[returns(AdapterInfoResponse)]
    AdapterInfo { name: String },
}

#[cw_serde]
pub struct ConfigResponse {
    pub config: Config,
}

#[cw_serde]
pub struct PoolInfoResponse {
    pub shares_issued: Uint128,
    pub balance_base_tokens: Uint128,
    pub adapter_deposits_base_tokens: Uint128,
    pub withdrawal_queue_base_tokens: Uint128,
}

#[cw_serde]
pub struct FundedWithdrawalRequestsResponse {
    pub withdrawal_ids: Vec<u64>,
}

#[cw_serde]
pub struct WithdrawalQueueInfoResponse {
    pub info: WithdrawalQueueInfo,
}

#[cw_serde]
pub struct UserWithdrawalRequestsResponse {
    pub withdrawals: Vec<WithdrawalEntry>,
}

#[cw_serde]
pub struct UserPayoutsHistoryResponse {
    pub payouts: Vec<PayoutEntry>,
}

#[cw_serde]
pub struct WhitelistResponse {
    pub whitelist: Vec<Addr>,
}

#[cw_serde]
pub struct AdaptersListResponse {
    pub adapters: Vec<(String, AdapterInfo)>,
}

#[cw_serde]
pub struct AdapterInfoResponse {
    pub info: AdapterInfo,
}
