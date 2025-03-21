use crate::contract::{CONTRACT_NAME, CONTRACT_VERSION};
use crate::error::{new_generic_error, ContractError};
// entry_point is being used but for some reason clippy doesn't see that, hence the allow attribute here
#[allow(unused_imports)]
use cosmwasm_std::{entry_point, DepsMut, Env, Response, StdError};
use cw2::{get_contract_version, set_contract_version};
use neutron_sdk::bindings::msg::NeutronMsg;
use neutron_sdk::bindings::query::NeutronQuery;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::unreleased::migrate_v3_1_1_to_unreleased;

pub const CONTRACT_VERSION_V1_1_0: &str = "1.1.0";
pub const CONTRACT_VERSION_V2_0_1: &str = "2.0.1";
pub const CONTRACT_VERSION_V2_0_2: &str = "2.0.2";
pub const CONTRACT_VERSION_V2_1_0: &str = "2.1.0";
pub const CONTRACT_VERSION_V3_0_0: &str = "3.0.0";
pub const CONTRACT_VERSION_V3_1_0: &str = "3.1.0";
pub const CONTRACT_VERSION_V3_1_1: &str = "3.1.1";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct MigrateMsgV3_2_0 {}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(
    mut deps: DepsMut<NeutronQuery>,
    _env: Env,
    _msg: MigrateMsgV3_2_0,
) -> Result<Response<NeutronMsg>, ContractError> {
    let contract_version = get_contract_version(deps.storage)?;

    if contract_version.version == CONTRACT_VERSION {
        return Err(ContractError::Std(StdError::generic_err(
            "Contract is already migrated to the newest version.",
        )));
    }

    if contract_version.version != CONTRACT_VERSION_V3_1_1 {
        return Err(new_generic_error(format!(
            "In order to migrate the contract to the newest version, its current version must be {}.",
            CONTRACT_VERSION_V3_1_1,
        )));
    }

    migrate_v3_1_1_to_unreleased(&mut deps)?;

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    Ok(Response::default())
}
