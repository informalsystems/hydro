use crate::contract::{CONTRACT_NAME, CONTRACT_VERSION};
use crate::error::{new_generic_error, ContractError};
use crate::migration::unreleased::{
    cleanup_migration_progress, is_token_ids_migration_done, migrate_populate_token_ids,
};
use crate::state::CONSTANTS;
use crate::utils::load_constants_active_at_timestamp;
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{DepsMut, Env, Response, StdError};
use cw2::{get_contract_version, set_contract_version};
// entry_point is being used but for some reason clippy doesn't see that, hence the allow attribute here
#[allow(unused_imports)]
use cosmwasm_std::entry_point;
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
    PopulateTokenIds { limit: u32 },
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(
    mut deps: DepsMut<NeutronQuery>,
    env: Env,
    msg: MigrateMsg,
) -> Result<Response<NeutronMsg>, ContractError> {
    check_contract_version(deps.storage)?;
    pause_contract_before_migration(&mut deps, &env)?;

    let response = match msg {
        MigrateMsg::PopulateTokenIds { limit } => migrate_populate_token_ids(&mut deps, limit),
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
