use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Coin, Timestamp, Uint128};
use cw_storage_plus::{Item, Map};

pub const CONSTANTS: Item<Constants> = Item::new("constants");

#[cw_serde]
pub struct Constants {
    pub denom: String,
    pub round_length: u64,
    pub total_pool: Uint128,
    pub first_round_start: Timestamp,
}

pub const LOCK_ID: Item<u64> = Item::new("lock_id");

pub const PROP_ID: Item<u64> = Item::new("prop_id");

// LOCKS_MAP: key(sender_address, lock_id) -> LockEntry
pub const LOCKS_MAP: Map<(Addr, u64), LockEntry> = Map::new("locks_map");
#[cw_serde]
pub struct LockEntry {
    pub funds: Coin,
    pub lock_start: Timestamp,
    pub lock_end: Timestamp,
}

// PROPOSAL_MAP: key(round_id, tranche_id, prop_id) -> Proposal
pub const PROPOSAL_MAP: Map<(u64, u64, u64), Proposal> = Map::new("prop_map");
#[cw_serde]
pub struct Proposal {
    pub round_id: u64,
    pub tranche_id: u64,
    pub proposal_id: u64,
    pub covenant_params: CovenantParams,
    pub executed: bool, // TODO: maybe remove in the future
    pub power: Uint128,
    pub percentage: Uint128,
}

#[cw_serde]
pub struct CovenantParams {
    // identifies the pool in which to deploy the funds
    pub pool_id: String,

    // Identifies the channel to the chain on which the funds should be deployed
    pub outgoing_channel_id: String,

    // Another identifier to check the destination of the funds, e.g. Astroport, Osmosis, etc.
    pub funding_destination_name: String,
}

// VOTE_MAP: key(round_id, tranche_id, sender_addr) -> Vote
pub const VOTE_MAP: Map<(u64, u64, Addr), Vote> = Map::new("vote_map");
#[cw_serde]
pub struct Vote {
    pub prop_id: u64,
    pub power: Uint128,
}

// PROPS_BY_SCORE: key((round_id, tranche_id), score, prop_id) -> prop_id
pub const PROPS_BY_SCORE: Map<((u64, u64), u128, u64), u64> = Map::new("props_by_score");

// TOTAL_POWER_VOTING: key(round_id, tranche_id) -> Uint128
pub const TOTAL_POWER_VOTING: Map<(u64, u64), Uint128> = Map::new("total_power_voting");

// TRANCHE_MAP: key(tranche_id) -> Tranche
pub const TRANCHE_MAP: Map<u64, Tranche> = Map::new("tranche_map");
#[cw_serde]
pub struct Tranche {
    pub tranche_id: u64,
    pub metadata: String,
}

// The initial whitelist is set upon contract instantiation.
// It can be updated by anyone on the WHITELIST_ADMINS list
// via the update_whitelist message.
pub const WHITELIST: Item<Vec<CovenantParams>> = Item::new("whitelist");

// Every address in this list can manage the whitelist.
pub const WHITELIST_ADMINS: Item<Vec<Addr>> = Item::new("whitelist_admins");
