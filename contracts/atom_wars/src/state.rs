use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Coin, Timestamp, Uint128};
use cw_storage_plus::{Item, Map};

pub const CONSTANTS: Item<Constants> = Item::new("constants");

#[cw_serde]
pub struct Constants {
    pub denom: String,
    pub round_length: u64,
    pub total_pool: Uint128,
}

pub const LOCK_ID: Item<u64> = Item::new("lock_id");

pub const PROP_ID: Item<u64> = Item::new("prop_id");

pub const ROUND_ID: Item<u64> = Item::new("round_id");

// LOCKS_MAP: key(sender_address, lock_id) -> LockEntry {
//     funds: Coin,
//     lock_start: Timestamp,
//     lock_end: Timestamp
// }
pub const LOCKS_MAP: Map<(Addr, u64), LockEntry> = Map::new("locks_map");
#[cw_serde]
pub struct LockEntry {
    pub funds: Coin,
    pub lock_start: Timestamp,
    pub lock_end: Timestamp,
}

// PROP_MAP: key(round_id, prop_id) -> Proposal {
//     round_id: u64,
//     covenant_params: String,
//     executed: bool,
//     power: Uint128
// }
pub const PROPOSAL_MAP: Map<(u64, u64), Proposal> = Map::new("prop_map");
#[cw_serde]
pub struct Proposal {
    pub round_id: u64,
    pub covenant_params: String,
    pub executed: bool,
    pub power: Uint128,
    pub percentage: Uint128,
    pub amount: Uint128,
}

// VOTE_MAP: key(round_id, sender_addr) -> Vote {
//     prop_id: u64,
//     power: Uint128,
//     tribute_claimed: bool
// }
pub const VOTE_MAP: Map<(u64, Addr), Vote> = Map::new("vote_map");
#[cw_serde]
pub struct Vote {
    pub prop_id: u64,
    pub power: Uint128,
}

// ROUND_MAP: key(round_id) -> Round {
//     round_id: u64,
//     round_end: Timestamp
// }
pub const ROUND_MAP: Map<u64, Round> = Map::new("round_map");
#[cw_serde]
pub struct Round {
    pub round_id: u64,
    pub round_end: Timestamp,
}

// PROPS_BY_SCORE: key(round_id, score, prop_id) -> prop_id
pub const PROPS_BY_SCORE: Map<(u64, u128, u64), u64> = Map::new("props_by_score");

// TOTAL_POWER_VOTING: key(round_id) -> Uint128
pub const TOTAL_POWER_VOTING: Map<u64, Uint128> = Map::new("total_power_voting");
