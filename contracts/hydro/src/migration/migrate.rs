use crate::contract::{CONTRACT_NAME, CONTRACT_VERSION};
use crate::error::ContractError;
use cosmwasm_schema::cw_serde;
// entry_point is being used but for some reason clippy doesn't see that, hence the allow attribute here
#[allow(unused_imports)]
use cosmwasm_std::entry_point;
use cosmwasm_std::{DepsMut, Env, Response, StdError};
use cw2::{get_contract_version, set_contract_version};
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

#[cw_serde]
pub struct MigrateMsg {}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(
    deps: DepsMut<NeutronQuery>,
    _env: Env,
    _msg: MigrateMsg,
) -> Result<Response<NeutronMsg>, ContractError> {
    check_contract_version(deps.storage)?;

    // No migration needed for v3.5.1 -> v3.5.2

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
