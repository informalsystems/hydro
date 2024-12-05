use std::str::FromStr;

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Decimal, DepsMut, Storage, Timestamp};
use cw_storage_plus::Item;
use neutron_sdk::bindings::query::NeutronQuery;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::ContractError;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct MigrateMsgUNRELEASED {}

#[cw_serde]
pub struct ConstantsV2_0_2 {
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
    pub max_deployment_duration: u64,
}

#[cw_serde]
pub struct ConstantsUNRELEASED {
    pub round_length: u64,
    pub lock_epoch_length: u64,
    pub first_round_start: Timestamp,
    pub max_locked_tokens: u128,
    pub max_validator_shares_participating: u64,
    pub hub_connection_id: String,
    pub hub_transfer_channel_id: String,
    pub icq_update_period: u64,
    pub paused: bool,
    pub max_deployment_duration: u64,
    pub round_lock_power_schedule: Vec<(u64, Decimal)>,
}

impl ConstantsUNRELEASED {
    pub fn from(old_constants: ConstantsV2_0_2) -> Self {
        Self {
            round_length: old_constants.round_length,
            lock_epoch_length: old_constants.lock_epoch_length,
            first_round_start: old_constants.first_round_start,
            max_locked_tokens: old_constants.max_locked_tokens,
            max_validator_shares_participating: old_constants.max_validator_shares_participating,
            hub_connection_id: old_constants.hub_connection_id,
            hub_transfer_channel_id: old_constants.hub_transfer_channel_id,
            icq_update_period: old_constants.icq_update_period,
            paused: old_constants.paused,
            max_deployment_duration: old_constants.max_deployment_duration,
            round_lock_power_schedule: get_default_power_schedule(),
        }
    }
}

pub fn get_default_power_schedule() -> Vec<(u64, Decimal)> {
    vec![
        (1, Decimal::from_str("1").unwrap()),
        (2, Decimal::from_str("1.25").unwrap()),
        (3, Decimal::from_str("1.5").unwrap()),
        (6, Decimal::from_str("2").unwrap()),
        (12, Decimal::from_str("4").unwrap()),
    ]
}

pub fn migrate_v2_0_2_to_unreleased(
    deps: &mut DepsMut<NeutronQuery>,
    _msg: MigrateMsgUNRELEASED,
) -> Result<(), ContractError> {
    migrate_constants(deps.storage)?;

    Ok(())
}

fn migrate_constants(storage: &mut dyn Storage) -> Result<(), ContractError> {
    const OLD_CONSTANTS: Item<ConstantsV2_0_2> = Item::new("constants");
    const NEW_CONSTANTS: Item<ConstantsUNRELEASED> = Item::new("constants");

    let old_constants = OLD_CONSTANTS.load(storage)?;
    let new_constants = ConstantsUNRELEASED::from(old_constants);
    NEW_CONSTANTS.save(storage, &new_constants)?;

    Ok(())
}
