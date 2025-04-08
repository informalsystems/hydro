use cosmwasm_schema::{cw_serde, schemars::JsonSchema, QueryResponses};
use cosmwasm_std::{Timestamp, Uint128};
use osmosis_std::types::cosmos::base::v1beta1::Coin;
use serde::{Deserialize, Serialize};

use crate::state::Bid;

#[cw_serde]
pub struct InstantiateMsg {
    pub pool_id: u64,
    pub principal_denom: String,
    pub counterparty_denom: String,
    pub first_round_start: Timestamp,
    pub round_length: u64,
    pub hydro: String,
    pub auction_duration: u64,
}

#[cw_serde]
pub struct CreatePositionMsg {
    pub lower_tick: i64,
    pub upper_tick: i64,
    pub principal_token_min_amount: Uint128,
    pub counterparty_token_min_amount: Uint128,
}

#[cw_serde]
pub struct CalculatePositionMsg {
    pub lower_tick: i64,
    pub principal_token_amount: Uint128,
    pub liquidation_bonus: f64,
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
    WidthdrawBid,
    ResolveAuction,
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(StateResponse)]
    GetState {},
    #[returns(Vec<(String, Vec<Coin>)>)]
    GetReservations {},
    #[returns(Vec<(String, Bid)>)]
    GetBids {},
}

#[cw_serde]
pub struct StateResponse {
    pub owner: String,
    pub pool_id: u64,
    pub position_id: Option<u64>,
    pub principal_denom: String,
    pub counterparty_denom: String,
    pub initial_principal_amount: Uint128,
    pub initial_counterparty_amount: Uint128,
    pub liquidity_shares: Option<String>,
    pub position_created_price: Option<String>,
    pub auction_period: bool,
    pub auction_end_time: Option<Timestamp>,
    pub principal_to_replenish: Option<Uint128>,
    pub counterparty_to_give: Option<Uint128>,
}
