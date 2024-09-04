use std::collections::HashMap;

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Coin, Decimal, Timestamp, Uint128};
use cw_storage_plus::{Item, Map};

pub const CONSTANTS: Item<Constants> = Item::new("constants");

#[cw_serde]
pub struct Constants {
    pub round_length: u64,
    pub lock_epoch_length: u64,
    pub first_round_start: Timestamp,
    pub max_locked_tokens: u128,
    pub max_validator_shares_participating: u64,
    pub hub_connection_id: String,
    pub hub_transfer_channel_id: String,
    pub icq_update_period: u64,
    pub paused: bool,
}

// the total number of tokens locked in the contract
pub const LOCKED_TOKENS: Item<u128> = Item::new("locked_tokens");

pub const LOCK_ID: Item<u64> = Item::new("lock_id");

// stores the current PROP_ID, in order to ensure that each proposal has a unique ID
// this is incremented every time a new proposal is created
pub const PROP_ID: Item<u64> = Item::new("prop_id");

// LOCKS_MAP: key(sender_address, lock_id) -> LockEntry
pub const LOCKS_MAP: Map<(Addr, u64), LockEntry> = Map::new("locks_map");
#[cw_serde]
pub struct LockEntry {
    pub lock_id: u64,
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
    // for each validator, stores the amount of shares of that validator the user voted with
    // (already scaled according to lockup scaling)
    pub time_weighted_shares: HashMap<String, Decimal>,
}

#[cw_serde]
// VoteWithPower is used to store a vote, where the time_weighted_shares
// have been resolved to compute the total power of the vote.
pub struct VoteWithPower {
    pub prop_id: u64,
    pub power: Decimal,
}

// PROPS_BY_SCORE: key((round_id, tranche_id), score, prop_id) -> prop_id
pub const PROPS_BY_SCORE: Map<((u64, u64), u128, u64), u64> = Map::new("props_by_score");

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

// VALIDATOR_TO_QUERY_ID: key(validator address) -> interchain query ID
pub const VALIDATOR_TO_QUERY_ID: Map<String, u64> = Map::new("validator_to_query_id");

// QUERY_ID_TO_VALIDATOR: key(interchain query ID) -> validator_address
pub const QUERY_ID_TO_VALIDATOR: Map<u64, String> = Map::new("query_id_to_validator");

// Duplicates some information from VALIDATORS_INFO to have the validators easily accessible by number of delegated tokens
// to compute the top N
// VALIDATORS_PER_ROUND: key(round_id, delegated_tokens, validator_address) -> validator_address
pub const VALIDATORS_PER_ROUND: Map<(u64, u128, String), String> = Map::new("validators_per_round");

// VALIDATORS_INFO: key(round_id, validator_address) -> ValidatorInfo
pub const VALIDATORS_INFO: Map<(u64, String), ValidatorInfo> = Map::new("validators_info");

// For each round and validator, it stores the time-scaled number of shares of that validator
// that are locked in Hydro.
// Concretely, the time weighted shares for each round are scaled by the lockup scaling factor,
// see scale_lockup_power in contract.rs
// SCALED_ROUND_POWER_SHARES_MAP: key(round_id, validator_address) -> number_of_shares
pub const SCALED_ROUND_POWER_SHARES_MAP: Map<(u64, String), Decimal> =
    Map::new("scaled_round_power_shares");

#[cw_serde]
#[derive(Default)]
pub struct ValidatorInfo {
    pub address: String,
    pub delegated_tokens: Uint128,
    pub power_ratio: Decimal,
}

impl ValidatorInfo {
    pub fn new(address: String, delegated_tokens: Uint128, power_ratio: Decimal) -> Self {
        Self {
            address,
            delegated_tokens,
            power_ratio,
        }
    }
}
