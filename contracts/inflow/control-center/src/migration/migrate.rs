use cosmwasm_schema::cw_serde;
use cosmwasm_std::entry_point;
use cosmwasm_std::{Addr, Decimal, DepsMut, Env, Response, StdError, StdResult, Storage};
use cw2::{get_contract_version, set_contract_version};
use cw_storage_plus::Item;
use interface::inflow_control_center::FeeConfig;

use crate::{
    contract::{CONTRACT_NAME, CONTRACT_VERSION},
    error::ContractError,
    state::FEE_CONFIG,
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

    migrate_fee_config(deps.storage)?;

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    Ok(Response::new().add_attribute("action", "migrate"))
}

/// Migrate FeeConfig: convert fee_recipient from Addr to Option<Addr>.
/// Empty string sentinel becomes None; any real address becomes Some(addr).
pub fn migrate_fee_config(storage: &mut dyn Storage) -> StdResult<()> {
    let old = FEE_CONFIG_V1.load(storage)?;
    let fee_recipient = if old.fee_recipient.as_str().is_empty() {
        None
    } else {
        Some(old.fee_recipient)
    };
    FEE_CONFIG.save(
        storage,
        &FeeConfig {
            fee_rate: old.fee_rate,
            fee_recipient,
        },
    )
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
