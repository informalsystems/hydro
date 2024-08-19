use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Coin, Timestamp, Uint128};
use cw_storage_plus::{Item, Map};

pub const CONSTANTS: Item<Constants> = Item::new("constants");

#[cw_serde]
pub struct Constants {
    pub round_length: u64,
    pub lock_epoch_length: u64,
    pub first_round_start: Timestamp,
    pub max_locked_tokens: u128,
    pub hub_transfer_channel_id: String,
    pub paused: bool,
    pub max_validator_shares_participating: u64,
}

// the total number of tokens locked in the contract
pub const LOCKED_TOKENS: Item<u128> = Item::new("locked_tokens");

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
    pub title: String,
    pub description: String,
    pub power: Uint128,
    pub percentage: Uint128,
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

// TOTAL_ROUND_POWER: key(round_id) -> total_round_voting_power
pub const TOTAL_ROUND_POWER: Map<u64, Uint128> = Map::new("total_round_power");

pub const TRANCHE_ID: Item<u64> = Item::new("tranche_id");

// TRANCHE_MAP: key(tranche_id) -> Tranche
pub const TRANCHE_MAP: Map<u64, Tranche> = Map::new("tranche_map");
#[cw_serde]
pub struct Tranche {
    pub id: u64,
    pub name: String,
    pub metadata: String,
}

// The initial whitelist is set upon contract instantiation.
// It can be updated by anyone on the WHITELIST_ADMINS list
// via the update_whitelist message.
// The addresses in the WHITELIST are the only addresses that are
// allowed to submit proposals.
pub const WHITELIST: Item<Vec<Addr>> = Item::new("whitelist");

// Every address in this list can manage the whitelist.
pub const WHITELIST_ADMINS: Item<Vec<Addr>> = Item::new("whitelist_admins");
