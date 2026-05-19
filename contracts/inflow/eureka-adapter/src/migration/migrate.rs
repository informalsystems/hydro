use cosmwasm_schema::cw_serde;
#[allow(unused_imports)]
use cosmwasm_std::entry_point;
use cosmwasm_std::{DepsMut, Env, Response, StdError};
use neutron_sdk::bindings::msg::NeutronMsg;
use neutron_sdk::bindings::query::NeutronQuery;

use crate::contract::{CONTRACT_NAME, CONTRACT_VERSION};
use crate::error::ContractError;

#[cw_serde]
pub struct MigrateMsg {}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(
    deps: DepsMut<NeutronQuery>,
    _env: Env,
    _msg: MigrateMsg,
) -> Result<Response<NeutronMsg>, ContractError> {
    check_contract_version(deps.storage)?;
    cw2::set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    Ok(Response::new())
}

fn check_contract_version(storage: &dyn cosmwasm_std::Storage) -> Result<(), ContractError> {
    let contract_version = cw2::get_contract_version(storage)?;
    if contract_version.version == CONTRACT_VERSION {
        return Err(ContractError::Std(StdError::generic_err(
            "Contract is already migrated to the newest version.",
        )));
    }
    Ok(())
}
