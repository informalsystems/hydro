use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Coin, Decimal, Timestamp, Uint128};
use cw_storage_plus::{Item, Map};

use crate::msg::LiquidityDeployment;

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
    pub is_in_pilot_mode: bool,
    pub max_bid_duration: u64,
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
    pub bid_duration: u64, // number of rounds liquidity is allocated excluding voting round.
    pub minimum_atom_liquidity_request: Uint128,
}

// VOTE_MAP: key((round_id, tranche_id), sender_addr, lock_id) -> Vote
pub const VOTE_MAP: Map<((u64, u64), Addr, u64), Vote> = Map::new("vote_map");

// Tracks the next round in which user is allowed to vote with the given lock_id.
// VOTING_ALLOWED_ROUND: key(tranche_id, lock_id) -> round_id
pub const VOTING_ALLOWED_ROUND: Map<(u64, u64), u64> = Map::new("voting_allowed_round");

#[cw_serde]
pub struct Vote {
    pub prop_id: u64,
    // stores the amount of shares of that validator the user voted with
    // (already scaled according to lockup scaling)
    pub time_weighted_shares: (String, Decimal),
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

// The following two store entries are used to store information about the validators in each round.
// The concept behind these maps is as follows:
// * The maps for the current round get updated when results from the interchain query are received.
// * When a new round starts, all transactions that depend on validator information will first check if the
//   information for the new round has been initialized yet. If not, the information from the previous round
//   will be copied over to the new round, to "seed" the info.
// * The information for the new round will then be updated as the interchain query results come in.
// The fact that the maps have been initialized for a round is stored in the VALIDATORS_STORE_INITIALIZED map.

// Duplicates some information from VALIDATORS_INFO to have the validators easily accessible by number of delegated tokens
// to compute the top N
// VALIDATORS_PER_ROUND: key(round_id, delegated_tokens, validator_address) -> validator_address
pub const VALIDATORS_PER_ROUND: Map<(u64, u128, String), String> = Map::new("validators_per_round");

// VALIDATORS_INFO: key(round_id, validator_address) -> ValidatorInfo
pub const VALIDATORS_INFO: Map<(u64, String), ValidatorInfo> = Map::new("validators_info");

// For each round, stores whether the VALIDATORS_INFO and the VALIDATORS_PER_ROUND
// have been initialized for this round yet by copying the information from the previous round.
// This is only done starting in the second round.
// VALIDATORS_STORE_INITIALIZED: key(round_id) -> bool
pub const VALIDATORS_STORE_INITIALIZED: Map<u64, bool> = Map::new("round_store_initialized");

// For each round and validator, it stores the time-scaled number of shares of that validator
// that are locked in Hydro.
// Concretely, the time weighted shares for each round are scaled by the lockup scaling factor,
// see scale_lockup_power in contract.rs
// SCALED_ROUND_POWER_SHARES_MAP: key(round_id, validator_address) -> number_of_shares
pub const SCALED_ROUND_POWER_SHARES_MAP: Map<(u64, String), Decimal> =
    Map::new("scaled_round_power_shares");

// The following two store fields are supposed to be kept in sync,
// i.e. whenever the shares of a proposal (or the power ratio of a validator)
// get updated, the total power of the proposal should be updated as well.
// For each proposal and validator, it stores the time-scaled number of shares of that validator
// that voted for the proposal.
// SCALED_PROPOSAL_SHARES_MAP: key(proposal_id, validator_address) -> number_of_shares
pub const SCALED_PROPOSAL_SHARES_MAP: Map<(u64, String), Decimal> =
    Map::new("scaled_proposal_power_shares");

// Stores the total power for each proposal.
// PROPOSAL_TOTAL_MAP: key(proposal_id) -> total_power
pub const PROPOSAL_TOTAL_MAP: Map<u64, Decimal> = Map::new("proposal_power_total");

// Stores the accounts that can attempt to create ICQs without sending funds to the contract
// in the same message, which will then implicitly be paid for by the contract.
// These accounts can also withdraw native tokens (but not voting tokens locked by users)
// from the contract.
pub const ICQ_MANAGERS: Map<Addr, bool> = Map::new("icq_managers");

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

// This map stores the liquidity deployments that were performed.
// These can be set by whitelist admins via the SetLiquidityDeployments message.
// LIQUIDITY_DEPLOYMENTS_MAP: key(round_id, tranche_id, prop_id) -> deployment
pub const LIQUIDITY_DEPLOYMENTS_MAP: Map<(u64, u64, u64), LiquidityDeployment> =
    Map::new("liquidity_deployments_map");
