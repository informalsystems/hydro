use crate::contract::{compute_round_end, CONTRACT_NAME, CONTRACT_VERSION};
use crate::error::ContractError;
use crate::msg::MigrateMsg;
use crate::state::{Constants, CONSTANTS, LOCKS_MAP};
use cosmwasm_std::{entry_point, DepsMut, Env, Order, Response, StdError, StdResult};
use cw2::{get_contract_version, set_contract_version};
use neutron_sdk::bindings::msg::NeutronMsg;
use neutron_sdk::bindings::query::NeutronQuery;

/// In the first version of Hydro, we allow contract to be un-paused through the Cosmos Hub governance
/// by migrating contract to the same code ID. This will trigger the migrate() function where we set
/// the paused flag to false.
/// Keep in mind that, for the future versions, this function should check the `CONTRACT_VERSION` and
/// perform any state changes needed. It should also handle the un-pausing of the contract, depending if
/// it was previously paused or not.
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(
    mut deps: DepsMut<NeutronQuery>,
    _env: Env,
    msg: MigrateMsg,
) -> Result<Response<NeutronMsg>, ContractError> {
    let contract_version = get_contract_version(deps.storage)?;
    CONSTANTS.update(
        deps.storage,
        |mut constants| -> Result<Constants, ContractError> {
            constants.paused = false;
            Ok(constants)
        },
    )?;

    if contract_version.version == CONTRACT_VERSION {
        return Err(ContractError::Std(StdError::generic_err(
            "Contract is already migrated to the newest version.",
        )));
    }

    if contract_version.version == "1.0.0" {
        // Perform the migration from 1.0.0 to 1.0.1
        migrate_v1_0_0_to_v1_1_0(&mut deps, msg)?;
    }

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    Ok(Response::default())
}

fn migrate_v1_0_0_to_v1_1_0(
    deps: &mut DepsMut<NeutronQuery>,
    msg: MigrateMsg,
) -> Result<(), ContractError> {
    // Migrate the contract to version 1.0.1

    // update the first_round_start to the value provided in the migration message
    CONSTANTS.update(
        deps.storage,
        |mut constants| -> Result<Constants, ContractError> {
            constants.first_round_start = msg.new_first_round_start;
            Ok(constants)
        },
    )?;

    // for each lock, update the lock_end to the new round_end
    let constants = CONSTANTS.load(deps.storage)?;
    let first_round_end = compute_round_end(&constants, 0)?;

    let locks = LOCKS_MAP
        .range(deps.storage, None, None, Order::Ascending)
        .collect::<StdResult<Vec<_>>>()?;

    for ((addr, lock_id), _) in locks {
        LOCKS_MAP.update(
            deps.storage,
            (addr.clone(), lock_id),
            |lock_entry_option| -> Result<_, ContractError> {
                // update the lock_end to the new round_end
                match lock_entry_option {
                    None => Err(ContractError::Std(StdError::generic_err(format!(
                        "Lock entry not found for address: {} and lock_id: {}",
                        addr, lock_id
                    )))),
                    Some(mut lock_entry) => {
                        lock_entry.lock_end = first_round_end;
                        Ok(lock_entry)
                    }
                }
            },
        )?;
    }

    Ok(())
}
