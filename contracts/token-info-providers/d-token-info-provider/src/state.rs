use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Decimal};
use cw_storage_plus::{Item, Map};

pub const CONFIG: Item<Config> = Item::new("config");

#[cw_serde]
pub struct Config {
    pub hydro_contract_address: Addr,
    pub d_token_denom: String,
    pub token_group_id: String,
    pub drop_staking_core_contract: Addr,
}

// TOKEN_RATIO: key(round_id) -> token_ratio
pub const TOKEN_RATIO: Map<u64, Decimal> = Map::new("token_ratio");
