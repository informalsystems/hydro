use cosmwasm_schema::cw_serde;
use cosmwasm_std::{DepsMut, Env, MessageInfo, Response};
use cw2::{get_contract_version, set_contract_version};
// entry_point is being used but for some reason clippy doesn't see that, hence the allow attribute here
#[allow(unused_imports)]
use cosmwasm_std::entry_point;
use neutron_sdk::bindings::msg::NeutronMsg;
use neutron_sdk::bindings::query::NeutronQuery;

use crate::contract::{CONTRACT_NAME, CONTRACT_VERSION};
use crate::error::{new_generic_error, ContractError};
use crate::migration::unreleased::migrate_config;

#[cw_serde]
pub struct MigrateMsg {
    pub control_center_addr: String,
    pub token_info_provider_addr: Option<String>,
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(
    mut deps: DepsMut<NeutronQuery>,
    _env: Env,
    info: MessageInfo,
    msg: MigrateMsg,
) -> Result<Response<NeutronMsg>, ContractError> {
    check_contract_version(deps.storage)?;

    let response = migrate_config(&mut deps, info, msg)?;

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    Ok(response)
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
