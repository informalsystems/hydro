use cosmwasm_std::{Deps, DepsMut, MessageInfo, StdResult};
use neutron_sdk::bindings::query::NeutronQuery;

use crate::error::ContractError;
use crate::state::{
    ChainConfig, Depositor, ADMINS, ALLOWED_DESTINATION_ADDRESSES, EXECUTORS,
    WHITELISTED_DEPOSITORS,
};

const NOBLE_HRP: &str = "noble";

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

/// Validates that the caller is an executor
pub fn validate_executor_caller(
    deps: &DepsMut<NeutronQuery>,
    info: &MessageInfo,
) -> Result<(), ContractError> {
    if !EXECUTORS.has(deps.storage, info.sender.clone()) {
        return Err(ContractError::UnauthorizedExecutor {});
    }

    Ok(())
}

/// Helper function to get depositor from storage (used in queries)
pub fn get_depositor(deps: Deps<NeutronQuery>, depositor_address: String) -> StdResult<Depositor> {
    let addr = deps.api.addr_validate(&depositor_address)?;
    WHITELISTED_DEPOSITORS.load(deps.storage, addr)
}

/// Helper function to check if executor exists (returns bool since no capabilities)
#[allow(dead_code)]
pub fn get_executor_exists(deps: Deps<NeutronQuery>, executor_address: String) -> StdResult<bool> {
    let addr = deps.api.addr_validate(&executor_address)?;
    Ok(EXECUTORS.has(deps.storage, addr))
}

/// Validate and normalize EVM address to lowercase without 0x prefix.
/// Accepts addresses with or without the 0x prefix; the prefix is stripped.
/// Validates that the address is exactly 40 hex characters (excluding 0x prefix).
pub fn normalize_evm_address(address: &str) -> Result<String, ContractError> {
    let address = address.trim();
    let hex_str = address.strip_prefix("0x").unwrap_or(address);

    // Validate length (should be 40 chars for 20 bytes)
    if hex_str.len() != 40 {
        return Err(ContractError::InvalidEvmAddress {
            address: address.to_string(),
            reason: format!("expected 40 hex characters, got {}", hex_str.len()),
        });
    }

    // Validate hex characters
    if !hex_str.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(ContractError::InvalidEvmAddress {
            address: address.to_string(),
            reason: "contains non-hexadecimal characters".to_string(),
        });
    }

    Ok(hex_str.to_lowercase())
}

/// Get destination address from storage, normalizing the address first.
/// Returns the normalized address string if it exists in the allowlist.
pub fn get_destination_address(
    deps: &Deps<NeutronQuery>,
    chain_id: &str,
    address: &str,
) -> Result<String, ContractError> {
    let normalized_address = normalize_evm_address(address)?;

    ALLOWED_DESTINATION_ADDRESSES
        .may_load(
            deps.storage,
            (chain_id.to_string(), normalized_address.clone()),
        )?
        .ok_or(ContractError::DestinationAddressNotAllowed {
            chain_id: chain_id.to_string(),
            address: normalized_address.clone(),
        })?;

    Ok(normalized_address)
}

impl ChainConfig {
    /// Validates and normalizes the ChainConfig struct
    pub fn validate_and_normalize(mut self) -> Result<Self, ContractError> {
        // Trim whitespace from chain_id and ensure it's not empty
        self.chain_id = self.chain_id.trim().to_string();

        // Check that chain_id isn't empty
        if self.chain_id.is_empty() {
            return Err(ContractError::InvalidChainConfig {
                reason: "chain_id cannot be empty".to_string(),
            });
        }

        // Check that both noble_receiver and noble_fee_recipient are valid noble addresses
        match bech32::decode(&self.bridging_config.noble_receiver) {
            Err(_) => {
                return Err(ContractError::InvalidNobleAddress {
                    address: self.bridging_config.noble_receiver.clone(),
                    reason: "invalid bech32 address".to_string(),
                });
            }
            Ok((prefix, _)) => {
                if prefix.as_str() != NOBLE_HRP {
                    return Err(ContractError::InvalidNobleAddress {
                        address: self.bridging_config.noble_receiver.clone(),
                        reason: format!(
                            "noble_receiver must have '{}' prefix, got '{}'",
                            NOBLE_HRP, prefix
                        ),
                    });
                }
            }
        }

        match bech32::decode(&self.bridging_config.noble_fee_recipient) {
            Err(_) => {
                return Err(ContractError::InvalidNobleAddress {
                    address: self.bridging_config.noble_fee_recipient.clone(),
                    reason: "invalid bech32 address".to_string(),
                });
            }
            Ok((prefix, _)) => {
                if prefix.as_str() != NOBLE_HRP {
                    return Err(ContractError::InvalidNobleAddress {
                        address: self.bridging_config.noble_fee_recipient.clone(),
                        reason: format!(
                            "noble_fee_recipient must have '{}' prefix, got '{}'",
                            NOBLE_HRP, prefix
                        ),
                    });
                }
            }
        }

        // Validate and normalize evm_destination_caller address
        self.bridging_config.evm_destination_caller =
            normalize_evm_address(&self.bridging_config.evm_destination_caller)?;

        Ok(self)
    }
}
