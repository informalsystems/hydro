use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Uint128};
use cw_storage_plus::Item;

#[cw_serde]
pub struct State {
    pub owner: Addr,
    pub pool_id: u64,
    pub position_id: Option<u64>,
    pub token0_denom: String,
    pub token1_denom: String,
    pub initial_token0_amount: Uint128,
}

pub const STATE: Item<State> = Item::new("state"); 