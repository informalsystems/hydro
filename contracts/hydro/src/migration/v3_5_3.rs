use cosmwasm_schema::cw_serde;
use cosmwasm_std::Timestamp;

use crate::{msg::CollectionInfo, state::RoundLockPowerSchedule};

#[cw_serde]
pub struct ConstantsV3_5_3 {
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
}
