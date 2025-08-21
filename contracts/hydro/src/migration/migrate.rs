use crate::contract::{CONTRACT_NAME, CONTRACT_VERSION};
use crate::error::{new_generic_error, ContractError};
use crate::migration::unreleased::{
    cleanup_migration_progress, is_token_ids_migration_done, migrate_populate_token_ids,
};
use crate::migration::v3_5_3::ConstantsV3_5_3;
use crate::state::{Constants, CONSTANTS};
use crate::utils::load_constants_active_at_timestamp;
use cosmwasm_std::{Decimal, DepsMut, Env, Order, Response, StdError};
use cw2::{get_contract_version, set_contract_version};
// entry_point is being used but for some reason clippy doesn't see that, hence the allow attribute here
use cosmwasm_schema::cw_serde;
#[allow(unused_imports)]
use cosmwasm_std::entry_point;
use cw_storage_plus::Map;
use neutron_sdk::bindings::msg::NeutronMsg;
use neutron_sdk::bindings::query::NeutronQuery;

pub const CONTRACT_VERSION_V1_1_0: &str = "1.1.0";
pub const CONTRACT_VERSION_V2_0_1: &str = "2.0.1";
pub const CONTRACT_VERSION_V2_0_2: &str = "2.0.2";
pub const CONTRACT_VERSION_V2_1_0: &str = "2.1.0";
pub const CONTRACT_VERSION_V3_0_0: &str = "3.0.0";
pub const CONTRACT_VERSION_V3_1_0: &str = "3.1.0";
pub const CONTRACT_VERSION_V3_1_1: &str = "3.1.1";
pub const CONTRACT_VERSION_V3_2_0: &str = "3.2.0";
pub const CONTRACT_VERSION_V3_4_1: &str = "3.4.1";
pub const CONTRACT_VERSION_V3_4_2: &str = "3.4.2";
pub const CONTRACT_VERSION_V3_5_0: &str = "3.5.0";
pub const CONTRACT_VERSION_V3_5_1: &str = "3.5.1";
pub const CONTRACT_VERSION_V3_5_2: &str = "3.5.2";
pub const CONTRACT_VERSION_V3_5_3: &str = "3.5.3";

#[cw_serde]
pub enum MigrateMsg {
    // Constants MUST be migrated before any other migrations!
    MigrateConstants {
        slash_percentage_threshold: Decimal,
        slash_tokens_receiver_addr: String,
    },
    PopulateTokenIds {
        limit: u32,
    },
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(
    mut deps: DepsMut<NeutronQuery>,
    env: Env,
    msg: MigrateMsg,
) -> Result<Response<NeutronMsg>, ContractError> {
    check_contract_version(deps.storage)?;

    let response = match msg {
        MigrateMsg::MigrateConstants {
            slash_percentage_threshold,
            slash_tokens_receiver_addr,
        } => migrate_constants(
            &mut deps,
            slash_percentage_threshold,
            slash_tokens_receiver_addr,
        ),
        MigrateMsg::PopulateTokenIds { limit } => {
            pause_contract_before_migration(&mut deps, &env)?;
            migrate_populate_token_ids(&mut deps, limit)
        }
    }?;

    let migration_done = is_token_ids_migration_done(deps.as_ref())?;
    if migration_done {
        // Clean up migration progress (remove TOKEN_IDS_MIGRATION_PROGRESS)
        cleanup_migration_progress(&mut deps);

        // If the migration is done, we can set the contract version to the new one
        // and unpause the contract
        set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
        unpause_contract_after_migration(&mut deps, env)?;
        return Ok(response.add_attribute("migration_status", "complete"));
    }

    Ok(response.add_attribute("migration_status", "incomplete"))
}

fn check_contract_version(storage: &dyn cosmwasm_std::Storage) -> Result<(), ContractError> {
    let contract_version = get_contract_version(storage)?;

    if contract_version.version == CONTRACT_VERSION {
        return Err(ContractError::Std(StdError::generic_err(
            "Contract is already migrated to the newest version.",
        )));
    }

    Ok(())
}

// Keep in mind that this function will store the new version of the Constants into the store.
// If there were some changes to the Constants structure, make sure to first migrate the constants,
// and then do the migration that requires pausing/unpausing the contract. Otherwise, the contract
// store will be broken.
fn pause_contract_before_migration(
    deps: &mut DepsMut<NeutronQuery>,
    env: &Env,
) -> Result<(), ContractError> {
    let (timestamp, mut constants) =
        load_constants_active_at_timestamp(&deps.as_ref(), env.block.time)?;

    if !constants.paused {
        constants.paused = true;
        CONSTANTS.save(deps.storage, timestamp, &constants)?;
    }

    Ok(())
}

fn unpause_contract_after_migration(
    deps: &mut DepsMut<NeutronQuery>,
    env: Env,
) -> Result<(), ContractError> {
    let (timestamp, mut constants) =
        load_constants_active_at_timestamp(&deps.as_ref(), env.block.time)?;

    if !constants.paused {
        return Err(new_generic_error(
            "Contract is already unpaused. Error in migration process",
        ));
    }

    constants.paused = false;
    CONSTANTS.save(deps.storage, timestamp, &constants)?;

    Ok(())
}

pub fn migrate_constants(
    deps: &mut DepsMut<NeutronQuery>,
    slash_percentage_threshold: Decimal,
    slash_tokens_receiver_addr: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    const OLD_CONSTANTS: Map<u64, ConstantsV3_5_3> = Map::new("constants");

    let old_constants_entries = OLD_CONSTANTS
        .range(deps.storage, None, None, Order::Ascending)
        .filter_map(|result| result.ok())
        .collect::<Vec<(u64, ConstantsV3_5_3)>>();

    if old_constants_entries.is_empty() {
        return Err(new_generic_error(
            "Couldn't find any Constants in the store.",
        ));
    }

    deps.api.addr_validate(&slash_tokens_receiver_addr)?;

    if slash_percentage_threshold > Decimal::percent(100) {
        return Err(new_generic_error(
            "Slash percentage threshold must be between 0% and 100%",
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
            slash_percentage_threshold,
            slash_tokens_receiver_addr: slash_tokens_receiver_addr.clone(),
        };

        updated_constants_entries.push((timestamp, new_constants));
    }

    for (timestamp, new_constants) in &updated_constants_entries {
        CONSTANTS.save(deps.storage, *timestamp, new_constants)?;
    }

    Ok(Response::new()
        .add_attribute("action", "migrate_constants")
        .add_attribute(
            "slash_percentage_threshold",
            slash_percentage_threshold.to_string(),
        )
        .add_attribute("slash_tokens_receiver_addr", slash_tokens_receiver_addr))
}
