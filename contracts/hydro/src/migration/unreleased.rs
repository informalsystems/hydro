use cosmwasm_std::{Decimal, DepsMut, Order, Response};
use cw_storage_plus::Map;

use crate::{
    error::{new_generic_error, ContractError},
    migration::v3_6_4::ConstantsV3_6_4,
    state::{Constants, CONSTANTS},
};

pub fn migrate_constants(
    deps: &mut DepsMut,
    lockup_conversion_fee_percent: Decimal,
) -> Result<Response, ContractError> {
    const OLD_CONSTANTS: Map<u64, ConstantsV3_6_4> = Map::new("constants");

    let old_constants_entries = OLD_CONSTANTS
        .range(deps.storage, None, None, Order::Ascending)
        .filter_map(|result| result.ok())
        .collect::<Vec<(u64, ConstantsV3_6_4)>>();

    if old_constants_entries.is_empty() {
        return Err(new_generic_error(
            "Couldn't find any Constants in the store.",
        ));
    }

    if lockup_conversion_fee_percent < Decimal::zero()
        || lockup_conversion_fee_percent > Decimal::percent(100)
    {
        return Err(new_generic_error(
            "Lockup conversion fee percent must be between 0% and 100%",
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
            cw721_collection_info: old_constants.cw721_collection_info,
            lock_expiry_duration_seconds: old_constants.lock_expiry_duration_seconds,
            lock_depth_limit: old_constants.lock_depth_limit,
            slash_percentage_threshold: old_constants.slash_percentage_threshold,
            slash_tokens_receiver_addr: old_constants.slash_tokens_receiver_addr,
            lockup_conversion_fee_percent,
        };

        updated_constants_entries.push((timestamp, new_constants));
    }

    for (timestamp, new_constants) in &updated_constants_entries {
        CONSTANTS.save(deps.storage, *timestamp, new_constants)?;
    }

    Ok(Response::new()
        .add_attribute("action", "migrate_constants")
        .add_attribute(
            "lockup_conversion_fee_percent",
            lockup_conversion_fee_percent.to_string(),
        ))
}
