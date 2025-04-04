use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Timestamp, Uint128};
use cw_storage_plus::{Item, Map};
use osmosis_std::types::cosmos::base::v1beta1::Coin;
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
    pub auction_period: bool,
}

pub const STATE: Item<State> = Item::new("state");

pub const RESERVATIONS: Map<&str, Vec<Coin>> = Map::new("reservations");
