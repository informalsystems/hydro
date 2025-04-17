use cosmwasm_std::{DepsMut, Order};
use cw_storage_plus::Map;
use neutron_sdk::bindings::query::NeutronQuery;

use crate::{
    error::{new_generic_error, ContractError},
    migration::v3_1_1::ConstantsV3_1_1,
    state::{Constants, CONSTANTS, TOKEN_INFO_PROVIDERS},
    token_manager::{TokenInfoProvider, TokenInfoProviderLSM, LSM_TOKEN_INFO_PROVIDER_ID},
};

pub fn migrate_v3_1_1_to_unreleased(deps: &mut DepsMut<NeutronQuery>) -> Result<(), ContractError> {
    const OLD_CONSTANTS: Map<u64, ConstantsV3_1_1> = Map::new("constants");

    let old_constants = OLD_CONSTANTS
        .range(deps.storage, None, None, Order::Ascending)
        .filter_map(|result| result.ok())
        .collect::<Vec<(u64, ConstantsV3_1_1)>>();

    let mut constants_to_add = vec![];
    let mut latest_constants: Option<ConstantsV3_1_1> = None;

    for constants_tuple in old_constants {
        latest_constants = Some(constants_tuple.1.clone());

        let new_constants = Constants {
            round_length: constants_tuple.1.round_length,
            lock_epoch_length: constants_tuple.1.lock_epoch_length,
            first_round_start: constants_tuple.1.first_round_start,
            max_locked_tokens: constants_tuple.1.max_locked_tokens,
            known_users_cap: constants_tuple.1.known_users_cap,
            paused: constants_tuple.1.paused,
            max_deployment_duration: constants_tuple.1.max_deployment_duration,
            round_lock_power_schedule: constants_tuple.1.round_lock_power_schedule,
        };

        constants_to_add.push((constants_tuple.0, new_constants));
    }

    if latest_constants.is_none() {
        return Err(new_generic_error(
            "Couldn't find any Constants in the store.",
        ));
    }

    for constants_to_add in constants_to_add {
        // No need to remove the old ones since we are rewriting all values under the same keys
        CONSTANTS.save(deps.storage, constants_to_add.0, &constants_to_add.1)?;
    }

    let latest_constants = latest_constants.unwrap();
    let lsm_token_info_provider = TokenInfoProviderLSM {
        hub_connection_id: latest_constants.hub_connection_id,
        hub_transfer_channel_id: latest_constants.hub_transfer_channel_id,
        icq_update_period: latest_constants.icq_update_period,
        max_validator_shares_participating: latest_constants.max_validator_shares_participating,
    };

    TOKEN_INFO_PROVIDERS.save(
        deps.storage,
        LSM_TOKEN_INFO_PROVIDER_ID.to_string(),
        &TokenInfoProvider::LSM(lsm_token_info_provider),
    )?;

    Ok(())
}
