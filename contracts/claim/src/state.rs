use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Coin, Timestamp, Uint128};
use cw_storage_plus::{Item, Map};

pub const CONFIG: Item<Config> = Item::new("config");
pub const NEXT_DISTRIBUTION_ID: Item<u64> = Item::new("next_distribution_id");

/// key(distribution_id) -> Distribution
pub const DISTRIBUTIONS: Map<u64, Distribution> = Map::new("distributions");

/// key(user_address, distribution_id) -> weight
pub const CLAIMS: Map<(Addr, u64), Uint128> = Map::new("claims");

/// key(user_address, distribution_id) -> ClaimRecord
pub const CLAIM_HISTORY: Map<(Addr, u64), ClaimRecord> = Map::new("claim_history");

#[cw_serde]
pub struct Config {
    pub admin: Addr,
    pub treasury: Addr,
}

#[cw_serde]
pub struct Distribution {
    pub id: u64,
    pub original_funds: Vec<Coin>,
    pub remaining_funds: Vec<Coin>,
    pub total_weight: Uint128,
    pub expiry: Timestamp,
}

#[cw_serde]
pub struct ClaimRecord {
    pub distribution_id: u64,
    pub funds_claimed: Vec<Coin>,
    pub claimed_at: Timestamp,
}
