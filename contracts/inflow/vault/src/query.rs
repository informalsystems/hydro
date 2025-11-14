use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Order};
// When compiling for wasm32 platform, compiler doesn't recognize that this type is used in one of the queries.
#[allow(unused_imports)]
use cosmwasm_std::Uint128;
// When compiling for wasm32 platform, compiler doesn't recognize that this type is used in one of the queries.
#[allow(unused_imports)]
use interface::inflow::PoolInfoResponse;

use crate::state::{Config, PayoutEntry, WithdrawalEntry, WithdrawalQueueInfo};

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
}

#[cw_serde]
pub struct ConfigResponse {
    pub config: Config,
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
