use cosmwasm_std::{Binary, StdResult, Uint128};
use serde_json::json;

use crate::state::BridgingConfig;

/// Construct Noble Orbiter memo for CCTP forwarding to EVM chain
///
/// Format follows Noble Orbiter v2 spec:
/// - Pre-actions: ACTION_FEE with fee recipient and amount
/// - Forwarding: PROTOCOL_CCTP with destination domain and EVM addresses
pub fn construct_noble_cctp_memo(
    bridging_config: &BridgingConfig,
    evm_mint_recipient: &str,
    fee_amount: Uint128,
) -> StdResult<String> {
    // Convert hex EVM addresses to base64 (32-byte padded format)
    let mint_recipient_base64 = evm_address_to_padded_base64(evm_mint_recipient)?;
    let destination_caller_base64 =
        evm_address_to_padded_base64(&bridging_config.evm_destination_caller)?;

    let memo = json!({
        "orbiter": {
            "pre_actions": [{
                "id": "ACTION_FEE",
                "attributes": {
                    "@type": "/noble.orbiter.controller.action.v2.FeeAttributes",
                    "fees_info": [{
                        "recipient": bridging_config.noble_fee_recipient,
                        "amount": {
                            "value": fee_amount.to_string()
                        }
                    }]
                }
            }],
            "forwarding": {
                "protocol_id": "PROTOCOL_CCTP",
                "attributes": {
                    "@type": "/noble.orbiter.controller.forwarding.v1.CCTPAttributes",
                    "destination_domain": bridging_config.destination_domain,
                    "mint_recipient": mint_recipient_base64,
                    "destination_caller": destination_caller_base64
                },
                "passthrough_payload": ""
            }
        }
    });

    serde_json::to_string(&memo).map_err(|e| {
        cosmwasm_std::StdError::generic_err(format!("Failed to serialize memo: {}", e))
    })
}

/// Convert hex EVM address to base64 with 32-byte padding
///
/// EVM addresses are 20 bytes but CCTP expects 32-byte values.
fn evm_address_to_padded_base64(hex_str: &str) -> StdResult<String> {
    // Remove 0x prefix if present
    let hex_str = hex_str.strip_prefix("0x").unwrap_or(hex_str);

    // Validate length (should be 40 chars for 20 bytes)
    if hex_str.len() != 40 {
        return Err(cosmwasm_std::StdError::generic_err(format!(
            "Invalid EVM address length: expected 40 hex chars, got {}",
            hex_str.len()
        )));
    }

    // Decode hex to 20-byte address
    let address_bytes = hex::decode(hex_str)
        .map_err(|e| cosmwasm_std::StdError::generic_err(format!("Invalid hex string: {}", e)))?;

    // Left-pad to 32 bytes: [12 zero bytes][20 address bytes]
    let mut padded = vec![0u8; 12];
    padded.extend_from_slice(&address_bytes);

    // Encode to base64
    Ok(Binary::from(padded).to_base64())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hex_to_padded_base64() {
        // Test with example from user's JSON
        let hex = "c0a7e8fee8ff0b35e752345772c46ba1db36a6ef";
        let result = evm_address_to_padded_base64(hex).unwrap();

        // Decode to verify structure
        use cosmwasm_std::Binary;
        let decoded = Binary::from_base64(&result).unwrap();
        assert_eq!(decoded.len(), 32);
        assert_eq!(&decoded[0..12], &[0u8; 12]); // First 12 bytes are zeros
    }

    #[test]
    fn test_construct_memo_structure() {
        let config = BridgingConfig {
            noble_receiver: "noble15xt7kx5mles58vkkfxvf0lq78sw04jajvfgd4d".to_string(),
            noble_fee_recipient: "noble1dyw0geqa2cy0ppdjcxfpzusjpwmq85r5a35hqe".to_string(),
            destination_domain: 0, // Ethereum mainnet
            evm_destination_caller: "fc05ad74c6fe2e7046e091d6ad4f660d2a159762".to_string(),
        };

        let evm_mint_recipient = "c0a7e8fee8ff0b35e752345772c46ba1db36a6ef";

        let memo =
            construct_noble_cctp_memo(&config, evm_mint_recipient, Uint128::new(1227130)).unwrap();

        // Verify structure
        assert!(memo.contains("orbiter"));
        assert!(memo.contains("ACTION_FEE"));
        assert!(memo.contains("PROTOCOL_CCTP"));
        assert!(memo.contains("1227130"));
    }

    #[test]
    fn test_invalid_hex_length() {
        let result = evm_address_to_padded_base64("abc"); // Too short
        assert!(result.is_err());
    }

    #[test]
    fn test_with_0x_prefix() {
        let hex = "0xc0a7e8fee8ff0b35e752345772c46ba1db36a6ef";
        let result = evm_address_to_padded_base64(hex);
        assert!(result.is_ok());
    }
}
