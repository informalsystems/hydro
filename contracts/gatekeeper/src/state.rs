use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Timestamp, Uint128};
use cw_storage_plus::{Item, Map};

// Gatekeeper contract configuration
pub const CONFIG: Item<Config> = Item::new("config");

// Next stage identifier. Gets incremented each time a new stage is registered.
pub const STAGE_ID: Item<u64> = Item::new("stage_id");

// Wallet addresses allowed to register new stages
// ADMINS: key(admin_address) -> ()
pub const ADMINS: Map<Addr, ()> = Map::new("admins");

// One stage holds the merkle tree root that is used to determine if user should be allowed
// to lock the specified amount of tokens. When trying to lock tokens, users must provide a
// list of proofs that can be verified against this root hash, as well as the number of tokens
// user is allowed to lock as per currently active stage criteria. At any given point in time,
// only one stage can be active, which is why stages are saved under their activation timestamp.
// HRP field of StageData structure can hold an external chain addresses prefix (e.g. cosmos,
// celestia, osmosis, etc.) if the tree root was generated from a JSON file containing addresses
// from other chains than the one on which Hydro is deployed. One stage can handle only one address
// prefix, which means that addresses from different chains can't be mixed in a single JSON file.
// key(activation_timestamp) -> StageData
pub const STAGES: Map<u64, StageData> = Map::new("stages");

// Keeps track of stages that mark the start of epochs. Used to build keys for USER_LOCK_AMOUNTS.
// key(start_stage_id) -> ()
pub const EPOCHS: Map<u64, ()> = Map::new("epochs");

// Used to track how many tokens users have locked per epoch. This information is used to
// validate if user should be allowed to lock more tokens in the current stage and epoch.
// key(user_address, epoch_start_stage_id) -> amount_locked
pub const USER_LOCK_AMOUNTS: Map<(Addr, u64), Uint128> = Map::new("user_lock_amounts");

#[cw_serde]
pub struct Config {
    pub hydro_contract: Addr,
}

#[cw_serde]
pub struct StageData {
    pub stage_id: u64,
    pub activate_at: Timestamp,
    pub merkle_root: String,
    pub hrp: Option<String>,
}
