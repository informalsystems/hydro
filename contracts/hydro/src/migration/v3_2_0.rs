use cosmwasm_schema::cw_serde;
use cosmwasm_std::Timestamp;

use crate::state::RoundLockPowerSchedule;

#[cw_serde]
pub struct ConstantsV3_2_0 {
    pub round_length: u64,
    pub lock_epoch_length: u64,
    pub first_round_start: Timestamp,
    pub max_locked_tokens: u128,
    pub known_users_cap: u128,
    pub paused: bool,
    pub max_deployment_duration: u64,
    pub round_lock_power_schedule: RoundLockPowerSchedule,
}
