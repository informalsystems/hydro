use crate::contract::{CONTRACT_NAME, CONTRACT_VERSION};
use crate::error::ContractError;
// entry_point is being used but for some reason clippy doesn't see that, hence the allow attribute here
#[allow(unused_imports)]
use cosmwasm_std::{entry_point, DepsMut, Env, Response, StdError};
use cw2::{get_contract_version, set_contract_version};
use neutron_sdk::bindings::msg::NeutronMsg;
use neutron_sdk::bindings::query::NeutronQuery;

use super::v2_0_1::{migrate_v1_1_0_to_v2_0_1, MigrateMsgV2_0_1};

pub const CONTRACT_VERSION_V1_1_0: &str = "1.1.0";
pub const CONTRACT_VERSION_V2_0_1: &str = "2.0.1";

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(
    mut deps: DepsMut<NeutronQuery>,
    _env: Env,
    msg: MigrateMsgV2_0_1,
) -> Result<Response<NeutronMsg>, ContractError> {
    let contract_version = get_contract_version(deps.storage)?;

    if contract_version.version == CONTRACT_VERSION {
        return Err(ContractError::Std(StdError::generic_err(
            "Contract is already migrated to the newest version.",
        )));
    }

    if contract_version.version == CONTRACT_VERSION_V1_1_0 {
        migrate_v1_1_0_to_v2_0_1(&mut deps, msg)?;
    }

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    Ok(Response::default())
}
