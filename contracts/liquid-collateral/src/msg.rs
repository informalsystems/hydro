use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Coin, Decimal, Timestamp, Uint128};

use crate::state::Bid;

#[cw_serde]
pub struct InstantiateMsg {
    pub pool_id: u64,
    pub principal_denom: String,
    pub counterparty_denom: String,
    pub round_duration: u64,
    pub project_owner: Option<String>,
    pub principal_funds_owner: String,
    pub auction_duration: u64,
    pub principal_first: bool,
}

#[cw_serde]
pub struct CreatePositionMsg {
    pub lower_tick: i64,
    pub upper_tick: i64,
    pub principal_token_min_amount: Uint128,
    pub counterparty_token_min_amount: Uint128,
}
#[cw_serde]
pub struct PlaceBidMsg {
    pub requested_amount: Uint128,
}

#[cw_serde]
pub struct WithdrawBidMsg {
    pub bid_id: u64,
}

#[cw_serde]
pub enum ExecuteMsg {
    CreatePosition(CreatePositionMsg),
    Liquidate,
    EndRound,
    PlaceBid(PlaceBidMsg),
    WithdrawBid(WithdrawBidMsg),
    ResolveAuction,
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(StateResponse)]
    State {},
    #[returns(Bid)]
    Bid { bid_id: u64 },
    #[returns(Vec<(String, Bid)>)]
    Bids { start_from: u32, limit: u32 },
    #[returns(Vec<(Addr, Decimal)>)]
    SortedBids {},
    #[returns(bool)]
    IsLiquidatable,
    #[returns(String)]
    SimulateLiquidation { principal_amount: Uint128 },
}

#[cw_serde]
pub struct StateResponse {
    pub project_owner: Option<Addr>,
    pub position_created_address: Option<Addr>,
    pub principal_funds_owner: String,
    pub pool_id: u64,
    pub position_id: Option<u64>,
    pub principal_denom: String,
    pub counterparty_denom: String,
    pub principal_first: bool,
    pub initial_principal_amount: Uint128,
    pub principal_to_replenish: Uint128,
    pub initial_counterparty_amount: Uint128,
    pub liquidity_shares: Option<String>,
    pub auction_end_time: Option<Timestamp>,
    pub counterparty_to_give: Option<Uint128>,
    pub auction_principal_deposited: Uint128,
    pub position_rewards: Option<Vec<Coin>>,
    pub round_end_time: Timestamp,
}
