use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Decimal, Timestamp};

use crate::{msg::CollectionInfo, state::RoundLockPowerSchedule};

#[cw_serde]
pub struct ConstantsV3_6_4 {
    pub round_length: u64,
    pub lock_epoch_length: u64,
    pub first_round_start: Timestamp,
    pub max_locked_tokens: u128,
    pub known_users_cap: u128,
    pub paused: bool,
    pub max_deployment_duration: u64,
    pub round_lock_power_schedule: RoundLockPowerSchedule,
    pub cw721_collection_info: CollectionInfo,
    pub lock_expiry_duration_seconds: u64,
    pub lock_depth_limit: u64,
    pub slash_percentage_threshold: Decimal,
    pub slash_tokens_receiver_addr: String,
}
