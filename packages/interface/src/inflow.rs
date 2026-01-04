use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Order, Timestamp, Uint128};

use super::inflow_control_center::PoolInfoResponse as ControlCenterPoolInfoResponse;

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

#[cw_serde]
pub enum ExecuteMsg {
    Deposit { on_behalf_of: Option<String> },
    Withdraw { on_behalf_of: Option<String> },
    CancelWithdrawal { withdrawal_ids: Vec<u64> },
    FulfillPendingWithdrawals { limit: u64 },
    ClaimUnbondedWithdrawals { withdrawal_ids: Vec<u64> },
    WithdrawForDeployment { amount: Uint128 },
    SetTokenInfoProviderContract { address: Option<String> },
    AddToWhitelist { address: String },
    RemoveFromWhitelist { address: String },
    UpdateConfig { config: UpdateConfigData },
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

    /// Returns the pool info of the Control Center contract.
    #[returns(ControlCenterPoolInfoResponse)]
    ControlCenterPoolInfo {},
}

#[cw_serde]
pub struct ConfigResponse {
    pub config: Config,
}

#[cw_serde]
pub struct PoolInfoResponse {
    pub shares_issued: Uint128,
    pub balance_base_tokens: Uint128,
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
