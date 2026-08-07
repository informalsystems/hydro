use cosmwasm_schema::cw_serde;
use cosmwasm_std::entry_point;
use cosmwasm_std::{Addr, Decimal, DepsMut, Env, Response, StdError};
use cw2::{get_contract_version, set_contract_version};
use cw_storage_plus::Item;

use crate::{
    contract::{CONTRACT_NAME, CONTRACT_VERSION},
    error::ContractError,
};

#[cw_serde]
pub struct MigrateMsg {}

/// Pre-migration FeeConfig with fee_recipient as plain Addr (not Option).
#[cw_serde]
pub struct FeeConfigV1 {
    pub fee_rate: Decimal,
    pub fee_recipient: Addr,
}

pub const FEE_CONFIG_V1: Item<FeeConfigV1> = Item::new("fee_config");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    check_contract_version(deps.storage)?;

    // No state migrations needed

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    Ok(Response::new().add_attribute("action", "migrate"))
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
