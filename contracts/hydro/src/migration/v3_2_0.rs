use cosmwasm_schema::cw_serde;
use cosmwasm_std::Timestamp;

use crate::error::{new_generic_error, ContractError};
use crate::msg::CollectionInfo;
use crate::state::{Constants, RoundLockPowerSchedule, CONSTANTS};

use cosmwasm_std::{DepsMut, Order, Response};
use cw_storage_plus::Map;
use neutron_sdk::bindings::{msg::NeutronMsg, query::NeutronQuery};

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

pub fn migrate_v3_2_0_to_v3_3_0(
    deps: &mut DepsMut<NeutronQuery>,
) -> Result<Response<NeutronMsg>, ContractError> {
    const OLD_CONSTANTS: Map<u64, ConstantsV3_2_0> = Map::new("constants");

    let old_constants_entries = OLD_CONSTANTS
        .range(deps.storage, None, None, Order::Ascending)
        .filter_map(|result| result.ok())
        .collect::<Vec<(u64, ConstantsV3_2_0)>>();

    // Check if there are any constants to migrate
    if old_constants_entries.is_empty() {
        return Err(new_generic_error(
            "Couldn't find any Constants in the store.",
        ));
    }

    let mut updated_constants_entries = vec![];

    for (timestamp, old_constants) in old_constants_entries {
        let new_constants = Constants {
            round_length: old_constants.round_length,
            lock_epoch_length: old_constants.lock_epoch_length,
            first_round_start: old_constants.first_round_start,
            max_locked_tokens: old_constants.max_locked_tokens,
            known_users_cap: old_constants.known_users_cap,
            paused: old_constants.paused,
            max_deployment_duration: old_constants.max_deployment_duration,
            round_lock_power_schedule: old_constants.round_lock_power_schedule,
            cw721_collection_info: CollectionInfo {
                name: "Hydro Lockups".to_string(),
                symbol: "hydro-lockups".to_string(),
            },
        };

        updated_constants_entries.push((timestamp, new_constants));
    }

    // No need to remove the old entries since we are rewriting all values under the same keys
    for (timestamp, new_constants) in &updated_constants_entries {
        CONSTANTS.save(deps.storage, *timestamp, new_constants)?;
    }

    Ok(Response::new()
        .add_attribute("action", "migrate_v3_2_0_to_v3_3_0")
        .add_attribute(
            "constants_updated",
            updated_constants_entries.len().to_string(),
        ))
}
