use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Coin, Decimal, Timestamp, Uint128};
use cw_storage_plus::{Item, Map, SnapshotMap, Strategy};
use cw_utils::Expiration;

use crate::{msg::CollectionInfo, msg::LiquidityDeployment, token_manager::TokenInfoProvider};

// The currently-active constants are always those with the largest activation_timestamp
// such that activation_timestamp <= current_block.timestamp
// CONSTANTS: key(activation_timestamp) -> Constants
pub const CONSTANTS: Map<u64, Constants> = Map::new("constants");

#[cw_serde]
pub struct LockPowerEntry {
    pub locked_rounds: u64,
    pub power_scaling_factor: Decimal,
}

// A vector of LockPowerEntries, where each entry contains a round number and the power scaling factor
// that a lockup has when it has this many rounds left at the end of the round.
// It will always be implicit that 0 rounds lock left corresponds to 0 voting power.
// Otherwise, it implicitly assumes that between two entries, the larger entries power is used.
// For example, if the schedule is [(1, 1), (2, 1.25), (3, 1.5), (6, 2), (12, 4)],
// where (i, j) means locked_rounds i, power_scaling_factor j,
// then the power scaling factors are
// 0x if lockup expires before the end of the round
// 1x if lockup has between 0 and 1 epochs left at the end of the round
// 1.25x if lockup has between 1 and 2 epochs left at the end of the round
// 1.5x if lockup has between 2 and 3 epochs left at the end of the round
// 2x if lockup has between 3 and 6 epochs left at the end of the round
// 4x if lockup has between 6 and 12 epochs left at the end of the round
#[cw_serde]
pub struct RoundLockPowerSchedule {
    pub round_lock_power_schedule: Vec<LockPowerEntry>,
}

impl RoundLockPowerSchedule {
    // This creates a new RoundLockPowerSchedule from a vector of tuples.
    // It will deduplicate the tuples by taking the first one if a round id appears twice.
    // It will also sort the tuples by round id.
    pub fn new(tuples: Vec<(u64, Decimal)>) -> Self {
        // deduplicate & sort
        let mut tuples = tuples;
        tuples.sort_by_key(|x| x.0);
        tuples.dedup_by_key(|x| x.0); // if a round id appears twice, only the first one will be used

        let round_lock_power_schedule = tuples
            .into_iter()
            .map(|d| LockPowerEntry {
                locked_rounds: d.0,
                power_scaling_factor: d.1,
            })
            .collect();
        RoundLockPowerSchedule {
            round_lock_power_schedule,
        }
    }
}

#[cw_serde]
pub struct Constants {
    pub round_length: u64,
    pub lock_epoch_length: u64,
    pub first_round_start: Timestamp,
    // The maximum number of tokens that can be locked by any users (currently known and the future ones)
    pub max_locked_tokens: u128,
    // The maximum number of tokens (out of the max_locked_tokens) that is reserved for locking only
    // for currently known users. This field is intended to be set to some value greater than zero at
    // the begining of the round, and such Constants would apply only for a predefined period of time.
    // After this period has expired, a new Constants would be activated that would set this value to
    // zero, which would allow any user to lock any amount that possibly wasn't filled, but was reserved
    // for this cap.
    pub known_users_cap: u128,
    pub paused: bool,
    pub max_deployment_duration: u64,
    pub round_lock_power_schedule: RoundLockPowerSchedule,
    pub cw721_collection_info: CollectionInfo,
    pub lock_expiry_duration_seconds: u64,
    pub lock_depth_limit: u64,
    pub slash_tokens_receiver_addr: String,
}

// Used to store a set of token info providers that are able to validate various denoms allowed to be locked
// in the contract. The key in this map is the address of the token info provider smart contract, except
// (temporarily) for the LSM one, which will have a hardcoded string key until we migrate LSM handling
// into a separate smart contract.
pub const TOKEN_INFO_PROVIDERS: Map<String, TokenInfoProvider> = Map::new("token_info_providers");

// Keeps the address of the associated Gatekeeper contract, if Hydro uses one. The Gatekeeper contract
// determines if a user should be allowed to lock the given amount of tokens at a given time.
pub const GATEKEEPER: Item<String> = Item::new("gatekeeper");
#[cw_serde]
pub struct DropTokenInfo {
    // The core drop address
    pub address: Addr,
    pub d_token_denom: String,
    pub puppeteer_address: Addr,
}
// Stores the information about the drop token, used for conversion of locked tokens into drop tokens.
pub const DROP_TOKEN_INFO: Item<DropTokenInfo> = Item::new("drop_token_info");

// the total number of tokens locked in the contract
pub const LOCKED_TOKENS: Item<u128> = Item::new("locked_tokens");

// Tracks the total number of tokens locked in known users cap, for the given round
// EXTRA_LOCKED_TOKENS_ROUND_TOTAL: key(round_id) -> uint128
pub const EXTRA_LOCKED_TOKENS_ROUND_TOTAL: Map<u64, u128> =
    Map::new("extra_locked_tokens_round_total");

// Tracks the number of tokens locked in known users cap by specific user, for the given round
// EXTRA_LOCKED_TOKENS_CURRENT_USERS: key(round_id, sender_address) -> uint128
pub const EXTRA_LOCKED_TOKENS_CURRENT_USERS: Map<(u64, Addr), u128> =
    Map::new("extra_locked_tokens_current_users");

pub const LOCK_ID: Item<u64> = Item::new("lock_id");

// stores the current PROP_ID, in order to ensure that each proposal has a unique ID
// this is incremented every time a new proposal is created
pub const PROP_ID: Item<u64> = Item::new("prop_id");

// LOCKS_MAP_V1: Previous structure, now preserved for migration, and for historical queries
// LOCKS_MAP_V1: key(sender_address, lock_id) -> LockEntry
// Note: as discussed in https://github.com/informalsystems/hydro/pull/244#discussion_r2065764776,
//  if the upgrade goes live during round 7, it could be safely removed in round 12
pub const LOCKS_MAP_V1: SnapshotMap<(Addr, u64), LockEntryV1> = SnapshotMap::new(
    "locks_map",
    "locks_map__checkpoints",
    "locks_map__changelog",
    Strategy::EveryBlock,
);

// LOCKS_MAP_V2: New structure without address in key, to enable NFT features
// LOCKS_MAP_V2: key(lock_id) -> LockEntry
pub const LOCKS_MAP_V2: SnapshotMap<u64, LockEntryV2> = SnapshotMap::new(
    "locks_map_v2",
    "locks_map_v2__checkpoints",
    "locks_map_v2__changelog",
    Strategy::EveryBlock,
);

#[cw_serde]
pub struct LockEntryV1 {
    pub lock_id: u64,
    pub funds: Coin,
    pub lock_start: Timestamp,
    pub lock_end: Timestamp,
}

impl LockEntryV1 {
    pub fn into_v2(self, owner: Addr) -> LockEntryV2 {
        LockEntryV2 {
            lock_id: self.lock_id,
            owner,
            funds: self.funds,
            lock_start: self.lock_start,
            lock_end: self.lock_end,
        }
    }
}

#[cw_serde]
pub struct LockEntryV2 {
    pub lock_id: u64,
    pub owner: Addr,
    pub funds: Coin,
    pub lock_start: Timestamp,
    pub lock_end: Timestamp,
}

// Stores the lockup IDs that belong to a user. Snapshoted so that we can determine which lockups
// user had at a given height and use this info to compute users voting power at that height.
// USER_LOCKS: key(user_address) -> Vec<lock_ids>
pub const USER_LOCKS: SnapshotMap<Addr, Vec<u64>> = SnapshotMap::new(
    "user_locks",
    "user_locks__checkpoints",
    "user_locks__changelog",
    Strategy::EveryBlock,
);

// Tracks how a lock was split or merged. Each parent lock ID maps to its resulting child lock IDs
// along with the corresponding fraction of value each child represents.
// LOCK_ID_TRACKING: key(parent_lock_id) -> Vec<(child_lock_id, fraction)>
pub const LOCK_ID_TRACKING: Map<u64, Vec<(u64, Decimal)>> = Map::new("lock_id_tracking");

// Inverse of LOCK_ID_TRACKING. Allows upward traversal to find all parent lock IDs for a given lock.
// Used for computing ancestry depth of lock compositions.
// REVERSE_LOCK_ID_TRACKING: key(child_lock_id) -> Vec<parent_lock_id>
pub const REVERSE_LOCK_ID_TRACKING: Map<u64, Vec<u64>> = Map::new("reverse_lock_id_tracking");

// Stores the deletion timestamp of each lock. Used to determine whether a lock (or its ancestors)
// is still active or has expired
// LOCK_ID_EXPIRY: key(lock_id) -> expiry_time (Timestamp)
pub const LOCK_ID_EXPIRY: Map<u64, Timestamp> = Map::new("lock_id_expiry");

// This is the total voting power of all users combined.
// TOTAL_VOTING_POWER_PER_ROUND: key(round_id) -> total_voting_power
pub const TOTAL_VOTING_POWER_PER_ROUND: SnapshotMap<u64, Uint128> = SnapshotMap::new(
    "total_voting_power_per_round",
    "total_voting_power_per_round__checkpoints",
    "total_voting_power_per_round__changelog",
    Strategy::EveryBlock,
);

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
    pub deployment_duration: u64, // number of rounds liquidity is allocated excluding voting round.
    pub minimum_atom_liquidity_request: Uint128,
}

// VOTE_MAP_V1: Previous structure, we need to keep it until all the tributes are claimed for round 0
// VOTE_MAP: key((round_id, tranche_id), sender_addr, lock_id) -> Vote
pub const VOTE_MAP_V1: Map<((u64, u64), Addr, u64), Vote> = Map::new("vote_map");

// VOTE_MAP_V2: New structure without address in key, to enable NFT features
// VOTE_MAP: key((round_id, tranche_id), lock_id) -> Vote
pub const VOTE_MAP_V2: Map<((u64, u64), u64), Vote> = Map::new("vote_map_v2");

// Tracks the next round in which user is allowed to vote with the given lock_id.
// VOTING_ALLOWED_ROUND: key(tranche_id, lock_id) -> round_id
pub const VOTING_ALLOWED_ROUND: Map<(u64, u64), u64> = Map::new("voting_allowed_round");

#[cw_serde]
pub struct Vote {
    pub prop_id: u64,
    // stores the amount of shares of the token group ID the user voted with
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

// For each round and token group ID, it stores the time-scaled number of shares of that token group
// that are locked in Hydro.
// Concretely, the time weighted shares for each round are scaled by the lockup scaling factor,
// see scale_lockup_power in contract.rs
// SCALED_ROUND_POWER_SHARES_MAP: key(round_id, token_group_ID) -> number_of_shares
pub const SCALED_ROUND_POWER_SHARES_MAP: Map<(u64, String), Decimal> =
    Map::new("scaled_round_power_shares");

// The following two store fields are supposed to be kept in sync,
// i.e. whenever the shares of a proposal (or the power ratio of a token group)
// get updated, the total power of the proposal should be updated as well.
// For each proposal and token group ID, it stores the time-scaled number of shares of that token group
// that voted for the proposal.
// SCALED_PROPOSAL_SHARES_MAP: key(proposal_id, token_group_ID) -> number_of_shares
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

// Stores the mapping between the round_id and the range of known block heights for that round.
// The lowest_known_height is the height at which the first transaction was executed, and the
// highest_known_height is the height at which the last transaction was executed against the smart
// contract in the given round.
// Notice that the round could span beyond these boundaries, but we don't have a way to know that.
// Besides, the info we store here is sufficient for our needs.
// ROUND_TO_HEIGHT_RANGE: key(round_id) -> HeightRange
pub const ROUND_TO_HEIGHT_RANGE: Map<u64, HeightRange> = Map::new("round_to_height_range");

// Stores the mapping between the block height and round. It gets populated
// each time a transaction is executed against the smart contract.
// HEIGHT_TO_ROUND: key(block_height) -> round_id
pub const HEIGHT_TO_ROUND: Map<u64, u64> = Map::new("height_to_round");

// The height at which the snapshot maps such as LOCKS_MAP, USER_LOCKS, TOTAL_VOTING_POWER_PER_ROUND, etc.
// were populated for the first time, making some historical queries available.
// Required because the initial implementation of the contract didn't have this maps, and the deployed
// contracts will be able to handle some queries only after the migration to the newest code is done.
pub const SNAPSHOTS_ACTIVATION_HEIGHT: Item<u64> = Item::new("snapshots_activation_height");

#[cw_serde]
#[derive(Default)]
pub struct HeightRange {
    pub lowest_known_height: u64,
    pub highest_known_height: u64,
}

// NFT_APPROVALS: key(lock_id, spender) -> Approval
pub const NFT_APPROVALS: Map<(u64, Addr), Approval> = Map::new("nft_approvals");

#[cw_serde]
#[derive(Default)]
pub struct Approval {
    pub spender: String,
    pub expires: Expiration,
}

/// Stored as (granter, operator), giving operator full control over granter's NFTs.
/// NOTE: granter is the owner, so operator has control only for NFTs owned by granter
pub const NFT_OPERATORS: Map<(Addr, Addr), Expiration> = Map::new("nft_operators");
// Maps users to the lock IDs they can claim tributes for (either they are current owner,
// or last owner before lock was removed)
// USER_LOCKS_FOR_CLAIM: key(user_address) -> Vec<lock_ids>
pub const USER_LOCKS_FOR_CLAIM: Map<Addr, Vec<u64>> = Map::new("user_locks_for_claim");
// LOCKS_PENDING_SLASHES: key(lock_id) -> token_num_to_slash
pub const LOCKS_PENDING_SLASHES: Map<u64, Uint128> = Map::new("locks_pending_slashes");
