use crate::contract::{CONTRACT_NAME, CONTRACT_VERSION};
use crate::error::{new_generic_error, ContractError};
use crate::state::CONSTANTS;
use crate::utils::load_constants_active_at_timestamp;
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{DepsMut, Env, Response, StdError};
use cw2::{get_contract_version, set_contract_version};
// entry_point is being used but for some reason clippy doesn't see that, hence the allow attribute here
#[allow(unused_imports)]
use cosmwasm_std::entry_point;

#[cw_serde]
pub struct MigrateMsg {}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    check_contract_version(deps.storage)?;

    // No state migrations needed from v3.6.5 to v3.6.6

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    Ok(Response::new())
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
fn _pause_contract_before_migration(deps: &mut DepsMut, env: &Env) -> Result<(), ContractError> {
    let (timestamp, mut constants) =
        load_constants_active_at_timestamp(&deps.as_ref(), env.block.time)?;

    if !constants.paused {
        constants.paused = true;
        CONSTANTS.save(deps.storage, timestamp, &constants)?;
    }

    Ok(())
}

fn _unpause_contract_after_migration(deps: &mut DepsMut, env: Env) -> Result<(), ContractError> {
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
