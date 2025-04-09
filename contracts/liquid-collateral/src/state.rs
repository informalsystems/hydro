use cosmwasm_std::{Addr, Decimal, Timestamp, Uint128};
use cw_storage_plus::{Item, Map};
use osmosis_std::types::cosmos::base::v1beta1::Coin;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct State {
    pub owner: Addr,
    pub hydro: Addr,
    pub pool_id: u64,
    pub position_id: Option<u64>,
    pub principal_denom: String,
    pub counterparty_denom: String,
    pub initial_principal_amount: Uint128,
    pub initial_counterparty_amount: Uint128,
    pub liquidity_shares: Option<String>,
    pub liquidator_address: Option<Addr>,
    pub round_length: u64,
    pub first_round_start: Timestamp,
    pub round_id: u64,
    pub position_created_price: Option<String>,
    pub auction_duration: u64,
    pub auction_period: bool,
    pub auction_end_time: Option<Timestamp>,
    pub principal_to_replenish: Option<Uint128>,
    pub counterparty_to_give: Option<Uint128>,
}

pub const STATE: Item<State> = Item::new("state");

pub const RESERVATIONS: Map<&str, Vec<Coin>> = Map::new("reservations");

// Each bid

#[derive(Serialize, Deserialize, Debug, JsonSchema)]
pub struct Bid {
    pub bidder: Addr,
    pub principal_amount: Uint128,
    pub tokens_requested: Uint128,
    pub percentage_replenished: Decimal,
}

pub const BIDS: Map<Addr, Bid> = Map::new("bids");
