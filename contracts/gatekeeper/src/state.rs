use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Uint128};
use cw_storage_plus::{Item, Map};

pub const CONFIG: Item<Config> = Item::new("config");
pub const ROOT_HASHES: Map<u64, String> = Map::new("root_hashes");
pub const ADMINS: Map<Addr, bool> = Map::new("admins");
pub const CLAIMED: Map<(String, Addr), Uint128> = Map::new("claimed");

#[cw_serde]
pub struct Config {
    pub hydro_contract: String,
}

#[cw_serde]
#[derive(Default)]
pub struct MerkleAirdropUserInfo {
    pub claimed_amount: Uint128,
    pub eligible_amount: Uint128,
}
