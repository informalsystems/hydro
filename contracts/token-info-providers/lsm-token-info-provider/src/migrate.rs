use cosmwasm_schema::cw_serde;
use cosmwasm_std::{entry_point, DepsMut, Env, Order, Response, StdError};
use cw2::{get_contract_version, set_contract_version};
use neutron_sdk::bindings::query::NeutronQuery;

use crate::{
    contract::{CONTRACT_NAME, CONTRACT_VERSION},
    error::ContractError,
    state::{CONFIG, VALIDATORS_INFO, VALIDATORS_PER_ROUND, VALIDATOR_TO_QUERY_ID},
    utils::query_current_round_id,
};

#[cw_serde]
pub struct MigrateMsg {}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(
    deps: DepsMut<NeutronQuery>,
    _env: Env,
    _msg: MigrateMsg,
) -> Result<Response, ContractError> {
    check_contract_version(deps.storage)?;

    // Clean up validators without active ICQs
    let config = CONFIG.load(deps.storage)?;
    let current_round_id = query_current_round_id(&deps.as_ref(), &config.hydro_contract_address)?;

    // Collect validators to remove
    let validators_to_remove: Vec<_> = VALIDATORS_INFO
        .prefix(current_round_id)
        .range(deps.storage, None, None, Order::Ascending)
        .filter_map(|item| {
            match item {
                Ok((_, validator_info)) => {
                    // Check if there's an active query for this validator
                    match VALIDATOR_TO_QUERY_ID
                        .may_load(deps.storage, validator_info.address.clone())
                        .unwrap_or(None)
                    {
                        None => Some(validator_info),
                        Some(_) => None, // Active query exists, do not remove
                    }
                }
                _ => None,
            }
        })
        .collect();

    // Remove validators without active ICQs
    for validator_info in &validators_to_remove {
        VALIDATORS_INFO.remove(
            deps.storage,
            (current_round_id, validator_info.address.clone()),
        );
        VALIDATORS_PER_ROUND.remove(
            deps.storage,
            (
                current_round_id,
                validator_info.delegated_tokens.u128(),
                validator_info.address.clone(),
            ),
        );
    }

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
