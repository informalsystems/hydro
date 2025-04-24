use crate::contract::{CONTRACT_NAME, CONTRACT_VERSION};
use crate::error::{new_generic_error, ContractError};
use crate::state::CONSTANTS;
// entry_point is being used but for some reason clippy doesn't see that, hence the allow attribute here
use crate::utils::load_constants_active_at_timestamp;
#[allow(unused_imports)]
use cosmwasm_std::{entry_point, DepsMut, Env, Response, StdError};
use cw2::{get_contract_version, set_contract_version};
use neutron_sdk::bindings::msg::NeutronMsg;
use neutron_sdk::bindings::query::NeutronQuery;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::unreleased::{
    is_full_migration_done, migrate_locks_batch, migrate_v3_1_1_to_unreleased, migrate_votes_batch,
};

pub const CONTRACT_VERSION_V1_1_0: &str = "1.1.0";
pub const CONTRACT_VERSION_V2_0_1: &str = "2.0.1";
pub const CONTRACT_VERSION_V2_0_2: &str = "2.0.2";
pub const CONTRACT_VERSION_V2_1_0: &str = "2.1.0";
pub const CONTRACT_VERSION_V3_0_0: &str = "3.0.0";
pub const CONTRACT_VERSION_V3_1_0: &str = "3.1.0";
pub const CONTRACT_VERSION_V3_1_1: &str = "3.1.1";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub enum MigrateMsgV3_2_0 {
    MigrateToUnreleased {},
    MigrateLocksV1ToV2 {
        start: Option<usize>,
        limit: Option<usize>,
    },
    MigrateVotesV1ToV2 {
        start: Option<usize>,
        limit: Option<usize>,
    },
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(
    mut deps: DepsMut<NeutronQuery>,
    env: Env,
    msg: MigrateMsgV3_2_0,
) -> Result<Response<NeutronMsg>, ContractError> {
    check_contract_version(deps.storage, CONTRACT_VERSION_V3_1_1)?;
    pause_contract_before_migration(&mut deps, &env)?;

    let response = match msg {
        MigrateMsgV3_2_0::MigrateToUnreleased {} => migrate_v3_1_1_to_unreleased(&mut deps),
        MigrateMsgV3_2_0::MigrateLocksV1ToV2 { start, limit } => migrate_locks_batch(
            &mut deps,
            env.block.height,
            start.unwrap_or(0),
            limit.unwrap_or(50),
        ),
        MigrateMsgV3_2_0::MigrateVotesV1ToV2 { start, limit } => {
            migrate_votes_batch(&mut deps, start.unwrap_or(0), limit.unwrap_or(50))
        }
    }?;

    let migration_done = is_full_migration_done(deps.as_ref())?;
    if migration_done {
        // If the migration is done, we can set the contract version to the new one
        // and set the paused flag to false
        // This is the final migration step
        set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
        unpause_contract_after_migration(&mut deps, env)?;
        return Ok(response.add_attribute("migration_status", "complete"));
    }

    Ok(response.add_attribute("migration_status", "incomplete"))
}

fn check_contract_version(
    storage: &dyn cosmwasm_std::Storage,
    expected_version: &str,
) -> Result<(), ContractError> {
    let contract_version = get_contract_version(storage)?;

    if contract_version.version == CONTRACT_VERSION {
        return Err(ContractError::Std(StdError::generic_err(
            "Contract is already migrated to the newest version.",
        )));
    }

    if contract_version.version != expected_version {
        return Err(new_generic_error(format!(
            "In order to migrate the contract to the newest version, its current version must be {}, got {}.",
            expected_version, contract_version.version
        )));
    }

    Ok(())
}

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
