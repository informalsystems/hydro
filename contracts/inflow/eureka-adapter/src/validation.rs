use cosmwasm_std::{Deps, DepsMut, MessageInfo, StdResult};
use neutron_sdk::bindings::query::NeutronQuery;

use crate::error::ContractError;
use crate::state::{
    ChainConfig, Depositor, ADMINS, ALLOWED_DESTINATION_ADDRESSES, ALLOWED_RECOVER_ADDRESSES,
    EXECUTORS, WHITELISTED_DEPOSITORS,
};

/// Validates that the caller is a registered and enabled depositor.
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

/// Validates that the caller is a config admin.
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

/// Validates that the caller is a registered executor.
pub fn validate_executor_caller(
    deps: &DepsMut<NeutronQuery>,
    info: &MessageInfo,
) -> Result<(), ContractError> {
    if !EXECUTORS.has(deps.storage, info.sender.clone()) {
        return Err(ContractError::UnauthorizedExecutor {});
    }
    Ok(())
}

/// Helper to load a depositor from storage (used in queries).
pub fn get_depositor(deps: Deps<NeutronQuery>, depositor_address: String) -> StdResult<Depositor> {
    let addr = deps.api.addr_validate(&depositor_address)?;
    WHITELISTED_DEPOSITORS.load(deps.storage, addr)
}

/// Validate and normalize an EVM address to lowercase without 0x prefix.
pub fn normalize_evm_address(address: &str) -> Result<String, ContractError> {
    let address = address.trim();
    let hex_str = address.strip_prefix("0x").unwrap_or(address);

    if hex_str.len() != 40 {
        return Err(ContractError::InvalidEvmAddress {
            address: address.to_string(),
            reason: format!("expected 40 hex characters, got {}", hex_str.len()),
        });
    }

    if !hex_str.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(ContractError::InvalidEvmAddress {
            address: address.to_string(),
            reason: "contains non-hexadecimal characters".to_string(),
        });
    }

    Ok(hex_str.to_lowercase())
}

/// Look up a destination address from the allowlist, normalizing it first.
pub fn get_destination_address(
    deps: &Deps<NeutronQuery>,
    chain_id: &str,
    address: &str,
) -> Result<String, ContractError> {
    let normalized = normalize_evm_address(address)?;

    ALLOWED_DESTINATION_ADDRESSES
        .may_load(deps.storage, (chain_id.to_string(), normalized.clone()))?
        .ok_or(ContractError::DestinationAddressNotAllowed {
            chain_id: chain_id.to_string(),
            address: normalized.clone(),
        })?;

    Ok(normalized)
}

/// Validate that a recover address is in the allowlist.
pub fn validate_recover_address(
    deps: &Deps<NeutronQuery>,
    address: &str,
) -> Result<(), ContractError> {
    ALLOWED_RECOVER_ADDRESSES
        .may_load(deps.storage, address.to_string())?
        .ok_or(ContractError::RecoverAddressNotAllowed {
            address: address.to_string(),
        })?;

    Ok(())
}

/// Validate a bech32 Cosmos address, returning it trimmed.
/// Does not enforce a specific prefix so any chain's address can be used.
pub fn validate_cosmos_address(address: &str) -> Result<String, ContractError> {
    let address = address.trim().to_string();
    bech32::decode(&address).map_err(|_| ContractError::InvalidCosmosAddress {
        address: address.clone(),
        reason: "invalid bech32 address".to_string(),
    })?;
    Ok(address)
}

impl ChainConfig {
    /// Validates and normalizes the ChainConfig struct.
    pub fn validate_and_normalize(mut self) -> Result<Self, ContractError> {
        self.chain_id = self.chain_id.trim().to_string();
        if self.chain_id.is_empty() {
            return Err(ContractError::InvalidChainConfig {
                reason: "chain_id cannot be empty".to_string(),
            });
        }

        self.eureka_source_channel = self.eureka_source_channel.trim().to_string();
        if self.eureka_source_channel.is_empty() {
            return Err(ContractError::InvalidChainConfig {
                reason: "eureka_source_channel cannot be empty".to_string(),
            });
        }

        // Validate eureka_fee_receiver is a valid bech32 address
        self.eureka_fee_receiver =
            validate_cosmos_address(&self.eureka_fee_receiver).map_err(|_| {
                ContractError::InvalidChainConfig {
                    reason: format!(
                        "eureka_fee_receiver is not a valid bech32 address: {}",
                        self.eureka_fee_receiver
                    ),
                }
            })?;

        if self.min_eureka_fee > self.max_eureka_fee {
            return Err(ContractError::InvalidChainConfig {
                reason: format!(
                    "min_eureka_fee ({}) cannot exceed max_eureka_fee ({})",
                    self.min_eureka_fee, self.max_eureka_fee
                ),
            });
        }

        Ok(self)
    }
}
