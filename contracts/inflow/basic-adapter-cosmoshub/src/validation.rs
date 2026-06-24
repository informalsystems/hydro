use cosmwasm_std::{Deps, DepsMut, MessageInfo};

use crate::error::ContractError;
use crate::state::{Depositor, ADMINS, WHITELISTED_DEPOSITORS};

/// Validates that the caller is a registered and enabled depositor
pub fn validate_depositor_caller(
    deps: &DepsMut,
    info: &MessageInfo,
) -> Result<Depositor, ContractError> {
    let depositor = WHITELISTED_DEPOSITORS
        .may_load(deps.storage, info.sender.clone())?
        .ok_or(ContractError::DepositorNotRegistered {
            depositor_address: info.sender.to_string(),
        })?;

    if !depositor.enabled {
        return Err(ContractError::Unauthorized {});
    }

    Ok(depositor)
}

/// Validates that the caller is an admin
pub fn validate_admin_caller(deps: &Deps, info: &MessageInfo) -> Result<(), ContractError> {
    let admins = ADMINS.load(deps.storage)?;
    if !admins.contains(&info.sender) {
        return Err(ContractError::UnauthorizedAdmin {});
    }
    Ok(())
}
