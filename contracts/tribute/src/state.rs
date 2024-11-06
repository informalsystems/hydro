use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Coin, Uint128};
use cw_storage_plus::{Item, Map};

pub const CONFIG: Item<Config> = Item::new("config");

#[cw_serde]
pub struct Config {
    pub hydro_contract: Addr,
    pub top_n_props_count: u64,
    pub min_prop_percent_for_claimable_tributes: Uint128,
}

// Need to have old version of Config in order to migrate data that is already stored on the chain.
#[cw_serde]
pub struct ConfigV1 {
    pub hydro_contract: Addr,
    pub top_n_props_count: u64,
}

pub const TRIBUTE_ID: Item<u64> = Item::new("tribute_id");

// tribute_id is part of the key and value to be able to store multiple tributes for the same proposal
// TRIBUTE_MAP: key(round_id, prop_id, tribute_id) -> tribute_id
pub const TRIBUTE_MAP: Map<(u64, u64, u64), u64> = Map::new("tribute_map");
#[cw_serde]
pub struct Tribute {
    pub round_id: u64,
    pub tranche_id: u64,
    pub proposal_id: u64,
    pub tribute_id: u64,
    pub depositor: Addr,
    pub funds: Coin,
    pub refunded: bool,
}

// For ease of accessing, maps each tribute_id to its Tribute struct
// This should always be in sync with the TRIBUTE_MAP above,
// and is used to quickly access a tribute by its ID.
pub const ID_TO_TRIBUTE_MAP: Map<u64, Tribute> = Map::new("id_to_tribute_map");

// Importantly, the TRIBUTE_CLAIMS for a voter_addr and tribute_id being present at all means the user has claimed that tribute.
// TRIBUTE_CLAIMS: key(voter_addr, tribute_id) -> amount_claimed
pub const TRIBUTE_CLAIMS: Map<(Addr, u64), Coin> = Map::new("tribute_claims");
