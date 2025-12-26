use cosmwasm_std::{Deps, DepsMut, MessageInfo, StdResult};
use neutron_sdk::bindings::query::NeutronQuery;

use crate::error::ContractError;
use crate::msg::SwapOperation;
use crate::state::{
    Depositor, RecipientConfig, RouteConfig, ADMINS, EXECUTORS, RECIPIENT_REGISTRY,
    WHITELISTED_DEPOSITORS,
};

/// Validates that the caller is a registered and enabled depositor
pub fn validate_depositor_caller(
    deps: &DepsMut<NeutronQuery>,
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

/// Validates that the caller is a config admin (mutable version)
pub fn validate_config_admin(
    deps: &DepsMut<NeutronQuery>,
    info: &MessageInfo,
) -> Result<(), ContractError> {
    let admins = ADMINS.load(deps.storage)?;

    if !admins.contains(&info.sender) {
        return Err(ContractError::UnauthorizedAdmin {});
    }

    Ok(())
}

/// Validates that the caller is either a config admin OR an executor
pub fn validate_admin_or_executor(
    deps: &DepsMut<NeutronQuery>,
    info: &MessageInfo,
) -> Result<(), ContractError> {
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
pub fn validate_route_config(route_config: &RouteConfig) -> Result<(), ContractError> {
    // Must have at least 2 denoms (in + out)
    if route_config.denoms_path.len() < 2 {
        return Err(ContractError::InvalidDenomPath {});
    }

    // First denom must match denom_in
    if route_config.denoms_path[0] != route_config.denom_in {
        return Err(ContractError::InvalidRoute {
            reason: format!(
                "First denom in path ({}) does not match denom_in ({})",
                route_config.denoms_path[0], route_config.denom_in
            ),
        });
    }

    // Last denom must match denom_out
    let last_idx = route_config.denoms_path.len() - 1;
    if route_config.denoms_path[last_idx] != route_config.denom_out {
        return Err(ContractError::InvalidRoute {
            reason: format!(
                "Last denom in path ({}) does not match denom_out ({})",
                route_config.denoms_path[last_idx], route_config.denom_out
            ),
        });
    }

    Ok(())
}

/// Validates that operations match the route's denom path
pub fn validate_operations_match_route(
    route_config: &RouteConfig,
    operations: &[SwapOperation],
) -> Result<(), ContractError> {
    // Number of operations should be len(denoms_path) - 1
    let expected_ops = route_config.denoms_path.len() - 1;
    if operations.len() != expected_ops {
        return Err(ContractError::OperationsCountMismatch {
            route_hops: expected_ops,
            operations_count: operations.len(),
        });
    }

    // Validate each operation matches the expected denom transition
    for (idx, operation) in operations.iter().enumerate() {
        let expected_denom_in = &route_config.denoms_path[idx];
        let expected_denom_out = &route_config.denoms_path[idx + 1];

        if &operation.denom_in != expected_denom_in {
            return Err(ContractError::RoutePathMismatch {
                expected: format!("operation[{}].denom_in = {}", idx, expected_denom_in),
                actual: format!("operation[{}].denom_in = {}", idx, operation.denom_in),
            });
        }

        if &operation.denom_out != expected_denom_out {
            return Err(ContractError::RoutePathMismatch {
                expected: format!("operation[{}].denom_out = {}", idx, expected_denom_out),
                actual: format!("operation[{}].denom_out = {}", idx, operation.denom_out),
            });
        }
    }

    Ok(())
}

/// Validates a recipient for post-swap transfer
pub fn validate_recipient(
    deps: &DepsMut<NeutronQuery>,
    recipient_address: &str,
) -> Result<RecipientConfig, ContractError> {
    let recipient_addr = deps.api.addr_validate(recipient_address)?;

    let recipient_config = RECIPIENT_REGISTRY
        .may_load(deps.storage, recipient_addr)?
        .ok_or(ContractError::RecipientNotRegistered {
            recipient: recipient_address.to_string(),
        })?;

    if !recipient_config.enabled {
        return Err(ContractError::RecipientDisabled {
            recipient: recipient_address.to_string(),
        });
    }

    Ok(recipient_config)
}

/// Helper function to get depositor from storage (used in queries)
pub fn get_depositor(deps: Deps<NeutronQuery>, depositor_address: String) -> StdResult<Depositor> {
    let addr = deps.api.addr_validate(&depositor_address)?;
    WHITELISTED_DEPOSITORS.load(deps.storage, addr)
}
