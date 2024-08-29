use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Coin, Decimal};
use cw_storage_plus::{Item, Map};

pub const CONFIG: Item<Config> = Item::new("config");

#[cw_serde]
pub struct Config {
    pub hydro_contract: Addr,
    pub top_n_props_count: u64,
    pub community_pool_config: CommunityPoolConfig,
}

#[cw_serde]
pub struct CommunityPoolConfig {
    // The percentage of the tribute that goes to the community pool.
    // The rest is distributed among voters.
    // Should be a number between 0 and 1.
    pub tax_percent: Decimal,
    // The channel ID to send the tokens to the community pool over.
    pub channel_id: String,
    // The address of the community pool *on the remote chain*.
    pub community_pool_address: String,
}

pub const TRIBUTE_ID: Item<u64> = Item::new("tribute_id");

// TRIBUTE_MAP: key((round_id, tranche_id), prop_id, tribute_id) -> Tribute
pub const TRIBUTE_MAP: Map<((u64, u64), u64, u64), Tribute> = Map::new("tribute_map");
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

// TRIBUTE_CLAIMS: key(voter_addr, tribute_id) -> bool
pub const TRIBUTE_CLAIMS: Map<(Addr, u64), bool> = Map::new("tribute_claims");
