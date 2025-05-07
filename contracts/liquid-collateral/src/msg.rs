use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Decimal, Timestamp, Uint128};

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
pub struct EndRoundBidMsg {
    pub requested_amount: Uint128,
}

#[cw_serde]
pub enum ExecuteMsg {
    CreatePosition(CreatePositionMsg),
    Liquidate,
    EndRound,
    EndRoundBid(EndRoundBidMsg),
    WithdrawBid,
    ResolveAuction,
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(StateResponse)]
    State {},
    #[returns(Bid)]
    Bid { address: String },
    #[returns(Vec<(String, Bid)>)]
    Bids {},
    #[returns(Vec<(Addr, Decimal)>)]
    SortedBids {},
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
    pub initial_principal_amount: Uint128,
    pub principal_to_replenish: Uint128,
    pub initial_counterparty_amount: Uint128,
    pub liquidity_shares: Option<String>,
    pub auction_end_time: Option<Timestamp>,
    pub counterparty_to_give: Option<Uint128>,
    pub auction_principal_deposited: Uint128,
}

#[cw_serde]
pub struct CalculatedDataResponse {
    pub strategy: String,   // Strategy name (tight, passive, conservative, etc.)
    pub upper_tick: String, // Upper tick for the given strategy
    pub counterparty_amount: String, // Amount of COUNTERPARTY token for the given strategy
}
