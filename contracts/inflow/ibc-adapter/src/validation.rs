use cosmwasm_std::{from_json, Binary, Deps, DepsMut, MessageInfo, StdResult};
use neutron_sdk::bindings::query::NeutronQuery;

use crate::error::ContractError;
use crate::state::{
    ChainConfig, Depositor, DepositorCapabilities, Executor, ADMINS, EXECUTORS,
    WHITELISTED_DEPOSITORS,
};

/// Validates that the caller is a registered and enabled depositor
/// Returns the Depositor struct if valid
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

/// Validates that the caller is a config admin
pub fn validate_admin_caller(
    deps: &Deps<NeutronQuery>,
    info: &MessageInfo,
) -> Result<(), ContractError> {
    let admins = ADMINS.load(deps.storage)?;

    if !admins.contains(&info.sender) {
        return Err(ContractError::UnauthorizedAdmin {});
    }

    Ok(())
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

/// Validates that the caller is an executor
pub fn validate_executor(
    deps: &DepsMut<NeutronQuery>,
    info: &MessageInfo,
) -> Result<(), ContractError> {
    if !EXECUTORS.has(deps.storage, info.sender.clone()) {
        return Err(ContractError::UnauthorizedExecutor {});
    }

    Ok(())
}

/// Validates that the caller is either a config admin OR an executor
/// Returns true if caller is config admin, false if executor
pub fn validate_admin_or_executor(
    deps: &DepsMut<NeutronQuery>,
    info: &MessageInfo,
) -> Result<bool, ContractError> {
    let admins = ADMINS.load(deps.storage)?;
    if admins.contains(&info.sender) {
        return Ok(true); // Is config admin
    }

    if EXECUTORS.has(deps.storage, info.sender.clone()) {
        return Ok(false); // Is executor (not admin)
    }

    Err(ContractError::Unauthorized {})
}

/// Validates recipient against chain-level allowed recipients
pub fn validate_recipient_for_chain(
    chain_config: &ChainConfig,
    recipient: &str,
) -> Result<(), ContractError> {
    // If allowed_recipients is empty, all recipients are allowed
    if chain_config.allowed_recipients.is_empty() {
        return Ok(());
    }

    // Check if recipient is in the allowed list
    if !chain_config
        .allowed_recipients
        .iter()
        .any(|r| r == recipient)
    {
        return Err(ContractError::RecipientNotAllowed {
            recipient: recipient.to_string(),
            chain_id: chain_config.chain_id.clone(),
        });
    }

    Ok(())
}

/// Parses and validates depositor capabilities from Binary format
pub fn validate_capabilities_binary(
    capabilities_binary: &Binary,
) -> Result<DepositorCapabilities, ContractError> {
    let capabilities: DepositorCapabilities =
        from_json(capabilities_binary).map_err(|_| ContractError::InvalidCapabilities {})?;

    Ok(capabilities)
}

/// Helper function to get depositor from storage (used in queries)
pub fn get_depositor(deps: Deps<NeutronQuery>, depositor_address: String) -> StdResult<Depositor> {
    let addr = deps.api.addr_validate(&depositor_address)?;
    WHITELISTED_DEPOSITORS.load(deps.storage, addr)
}

/// Helper function to get executor from storage (used in queries)
pub fn get_executor(deps: Deps<NeutronQuery>, executor_address: String) -> StdResult<Executor> {
    let addr = deps.api.addr_validate(&executor_address)?;
    EXECUTORS.load(deps.storage, addr)
}
