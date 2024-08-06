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
    pub paused: bool,

    // The number of validators whose LST tokens may vote in Hydro.
    // When determining voting power, locked tokens, etc, only
    // LSTs of the top MAX_VALIDATOR_SHARES_PARTICIPATING
    // validators (by delegated tokens) are considered.
    //
    // This is to avoid DoS attacks, where someone could create
    // a large number of validators with extremely small amounts of shares
    // and lock/vote with all of them. Hydro must not need to iterate
    // over all of those different validators when tallying votes,
    // since that would be very expensive.
    pub max_validator_shares_participating: u64,
}

// the number of tokenized shares of each validator locked in the contract.
// LOCKED_VALIDATOR_SHARES: key(validator_address) -> u128
pub const LOCKED_VALIDATOR_SHARES: Map<String, u128> = Map::new("locked_tokens");

// the current lock id, auto-incrementing for each new lock
pub const LOCK_ID: Item<u64> = Item::new("lock_id");

// the current proposal id, auto-incrementing for each new proposal
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
    pub covenant_params: CovenantParams,
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

// This keeps, for each round/tranche and each validator, the amount of shares of that validator
// that voted for each proposal. This is used to compute the score of each proposal.
// PROPS_BY_VALIDATOR_SHARES: key((round_id, tranche_id), validator_operator_address, prop_id) -> u64
pub const PROPS_BY_VALIDATOR_SHARES: Map<((u64, u64), String, u64), u128> =
    Map::new("props_by_validator_shares");

// TOTAL_VOTED_POWER: key(round_id, tranche_id) -> Uint128
pub const TOTAL_VOTED_POWER: Map<(u64, u64), Uint128> = Map::new("total_voted_power");

// TOTAL_ROUND_POWER: key(round_id) -> total_round_voting_power
pub const TOTAL_ROUND_POWER: Map<u64, Uint128> = Map::new("total_round_power");

// a mapping from round_id and validator addresses to the ratio of their shares to underlying staked tokens
// for example, a validator who has 1000 outstanding shares and 950 tokens staked
// has a ratio of 0.95
// We need to keep this data per-round because these ratios can change over time,
// and we want to keep the historical data to be able to reconstruct proposal scores
// if called after the round has ended.
// VALIDATOR_SHARE_TO_TOKEN_RATIO: key(round_id, validator_address) -> Decimal
pub const VALIDATOR_SHARE_TO_TOKEN_RATIO: Map<(u64, String), Decimal> =
    Map::new("share_to_weight_map");

// For each round, stores the list of validators whose shares are eligible to vote.
// We only store the top MAX_VALIDATOR_SHARES_PARTICIPATING validators by delegated tokens,
// to avoid DoS attacks where someone creates a large number of validators with very small amounts of shares.
pub const VALIDATORS_PER_ROUND: Map<u64, Vec<String>> = Map::new("validators_per_round");

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
