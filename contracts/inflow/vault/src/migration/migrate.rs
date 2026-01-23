use cosmwasm_schema::cw_serde;
use cosmwasm_std::{DepsMut, Env, Response};
use cw2::{get_contract_version, set_contract_version};
// entry_point is being used but for some reason clippy doesn't see that, hence the allow attribute here
#[allow(unused_imports)]
use cosmwasm_std::entry_point;
use neutron_sdk::bindings::msg::NeutronMsg;
use neutron_sdk::bindings::query::NeutronQuery;

use crate::contract::{CONTRACT_NAME, CONTRACT_VERSION};
use crate::error::{new_generic_error, ContractError};

#[cw_serde]
pub struct MigrateMsg {}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(
    deps: DepsMut<NeutronQuery>,
    _env: Env,
    _msg: MigrateMsg,
) -> Result<Response<NeutronMsg>, ContractError> {
    check_contract_version(deps.storage)?;

    // No state migrations needed from v3.6.6 to v3.6.7

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    Ok(Response::new())
}

fn check_contract_version(storage: &dyn cosmwasm_std::Storage) -> Result<(), ContractError> {
    let contract_version = get_contract_version(storage)?;

    if contract_version.version == CONTRACT_VERSION {
        return Err(new_generic_error(
            "Contract is already migrated to the newest version.",
        ));
    }

    Ok(())
}
