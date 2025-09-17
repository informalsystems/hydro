use cosmwasm_schema::cw_serde;
use cosmwasm_std::Addr;
use cw_storage_plus::{Item, Map};
use interface::lsm::ValidatorInfo;

pub const CONFIG: Item<Config> = Item::new("config");

// Every address in this list can manage the ICQ_MANAGERS list.
pub const ADMINS: Map<Addr, bool> = Map::new("admins");

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

// Stores the accounts that can attempt to create ICQs without sending funds to the contract
// in the same message, which will then implicitly be paid for by the contract.
// These accounts can also withdraw native tokens from the contract.
pub const ICQ_MANAGERS: Map<Addr, bool> = Map::new("icq_managers");

#[cw_serde]
pub struct Config {
    pub hydro_contract_address: Addr,
    pub max_validator_shares_participating: u64,
    pub hub_connection_id: String,
    pub hub_transfer_channel_id: String,
    pub icq_update_period: u64,
}
