use cosmwasm_std::{DepsMut, MessageInfo};

use crate::error::ContractError;
use crate::state::{Depositor, UnifiedRoute, ADMINS, EXECUTORS, WHITELISTED_DEPOSITORS};

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

/// Validates that the caller is a config admin
pub fn validate_config_admin(deps: &DepsMut, info: &MessageInfo) -> Result<(), ContractError> {
    let admins = ADMINS.load(deps.storage)?;

    if !admins.contains(&info.sender) {
        return Err(ContractError::UnauthorizedAdmin {});
    }

    Ok(())
}

/// Validates that the caller is either a config admin OR an executor
pub fn validate_admin_or_executor(deps: &DepsMut, info: &MessageInfo) -> Result<(), ContractError> {
    let admins = ADMINS.load(deps.storage)?;
    if admins.contains(&info.sender) {
        return Ok(());
    }

    let executors = EXECUTORS.load(deps.storage)?;
    if executors.contains(&info.sender) {
        return Ok(());
    }

    Err(ContractError::UnauthorizedExecutor {})
}

/// Validates a route configuration
pub fn validate_route_config(route: &UnifiedRoute) -> Result<(), ContractError> {
    // Must have at least one operation
    if route.operations.is_empty() {
        return Err(ContractError::InvalidRoute {
            reason: "Route must have at least one operation".to_string(),
        });
    }

    // Validate operations form a continuous path
    for i in 0..route.operations.len() - 1 {
        if route.operations[i].denom_out != route.operations[i + 1].denom_in {
            return Err(ContractError::InvalidRoute {
                reason: format!(
                    "Operation[{}].denom_out ({}) does not match Operation[{}].denom_in ({})",
                    i,
                    route.operations[i].denom_out,
                    i + 1,
                    route.operations[i + 1].denom_in
                ),
            });
        }
    }

    // On Cosmos Hub, all venues are cross-chain — must have non-empty forward_path
    if route.forward_path.is_empty() {
        return Err(ContractError::InvalidForwardPath {
            reason: "Cross-chain routes must specify forward_path".to_string(),
        });
    }

    // Validate each hop has chain_id, channel and receiver
    for (idx, hop) in route.forward_path.iter().enumerate() {
        if hop.chain_id.is_empty() {
            return Err(ContractError::InvalidForwardPath {
                reason: format!("Forward hop {} has empty chain_id", idx),
            });
        }
        if hop.channel.is_empty() {
            return Err(ContractError::InvalidForwardPath {
                reason: format!("Forward hop {} has empty channel", idx),
            });
        }
        if hop.receiver.is_empty() {
            return Err(ContractError::InvalidForwardPath {
                reason: format!("Forward hop {} has empty receiver", idx),
            });
        }
    }

    Ok(())
}
