use cosmwasm_std::{Addr, Decimal, Timestamp, Uint128};
use cw_storage_plus::{Item, Map};
use osmosis_std::types::cosmos::base::v1beta1::Coin;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct State {
    pub project_owner: Option<Addr>,
    pub principal_funds_owner: Addr,
    pub pool_id: u64,
    pub position_created_address: Option<Addr>,
    pub position_id: Option<u64>,
    pub principal_denom: String,
    pub counterparty_denom: String,
    pub initial_principal_amount: Uint128,
    pub initial_counterparty_amount: Uint128,
    pub liquidity_shares: Option<String>,
    pub liquidator_address: Option<Addr>,
    pub round_end_time: Timestamp,
    pub auction_duration: u64,
    pub auction_end_time: Option<Timestamp>,
    pub auction_principal_deposited: Uint128,
    pub principal_to_replenish: Uint128,
    pub counterparty_to_give: Option<Uint128>,
}

pub const STATE: Item<State> = Item::new("state");

pub const SORTED_BIDS: Item<Vec<(Addr, Decimal, Uint128)>> = Item::new("sorted_bids");

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub enum BidStatus {
    Submitted,
    Processed,
    Refunded,
}

#[derive(Serialize, Deserialize, Debug, JsonSchema)]
pub struct Bid {
    pub bidder: Addr,
    pub principal_deposited: Uint128,
    pub tokens_requested: Uint128,
    pub tokens_fulfilled: Uint128,
    pub tokens_refunded: Uint128,
    pub status: BidStatus,
}

pub const BIDS: Map<Addr, Bid> = Map::new("bids");
