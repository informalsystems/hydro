use cosmwasm_schema::cw_serde;
use cosmwasm_std::Addr;
use cw_storage_plus::{Item, Map};
use interface::lsm::ValidatorInfo;

pub const CONFIG: Item<Config> = Item::new("config");

// Every address in this list can manage the contract (reserved for future use).
pub const ADMINS: Map<Addr, bool> = Map::new("admins");

// The following two store entries are used to store information about the validators in each round.
// The concept behind these maps is as follows:
// * The maps for the current round get updated when update_validators_ratios() is called.
// * When a new round starts, all transactions that depend on validator information will first check if the
//   information for the new round has been initialized yet. If not, the information from the previous round
//   will be copied over to the new round, to "seed" the info.
// * The information for the new round will then be updated as update_validators_ratios() calls come in.
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

#[cw_serde]
pub struct Config {
    pub hydro_contract_address: Addr,
    pub max_validator_shares_participating: u64,
}
