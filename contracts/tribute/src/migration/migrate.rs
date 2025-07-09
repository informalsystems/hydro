use cosmwasm_schema::cw_serde;
use cosmwasm_std::{entry_point, DepsMut, Env, Response, StdError};
use cw2::{get_contract_version, set_contract_version};

use crate::{
    contract::{CONTRACT_NAME, CONTRACT_VERSION},
    error::ContractError,
};

pub const CONTRACT_VERSION_V1_1_1: &str = "1.1.1";
pub const CONTRACT_VERSION_V2_0_1: &str = "2.0.1";
pub const CONTRACT_VERSION_V2_0_2: &str = "2.0.2";
pub const CONTRACT_VERSION_V3_0_0: &str = "3.0.0";
pub const CONTRACT_VERSION_V3_1_1: &str = "3.1.1";
pub const CONTRACT_VERSION_V3_2_0: &str = "3.2.0";
pub const CONTRACT_VERSION_V3_4_1: &str = "3.4.1";
pub const CONTRACT_VERSION_V3_4_2: &str = "3.4.2";

#[cw_serde]
pub struct MigrateMsg {}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    check_contract_version(deps.storage, CONTRACT_VERSION_V3_4_1)?;

    // No state migrations needed from v3.4.1 to v3.5.0

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    Ok(Response::new())
}

fn check_contract_version(
    storage: &dyn cosmwasm_std::Storage,
    expected_version: &str,
) -> Result<(), ContractError> {
    let contract_version = get_contract_version(storage)?;

    if contract_version.version == CONTRACT_VERSION {
        return Err(ContractError::Std(StdError::generic_err(
            "Contract is already migrated to the newest version.",
        )));
    }

    if contract_version.version != expected_version {
        return Err(ContractError::Std(StdError::generic_err(format!(
            "In order to migrate the contract to the newest version, its current version must be {}, got {}.",
            expected_version, contract_version.version
        ))));
    }

    Ok(())
}
