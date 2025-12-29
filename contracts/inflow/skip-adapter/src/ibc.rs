use cosmwasm_std::{Deps, StdResult};
use neutron_sdk::bindings::query::NeutronQuery;
use sha2::{Digest, Sha256};

use crate::state::{CHANNEL_REGISTRY, TOKEN_REGISTRY};

/// Calculate IBC denom hash
/// Formula: "ibc/" + uppercase(sha256("transfer/channel-X/native_denom"))
///
/// # Arguments
/// * `channel` - IBC channel ID (e.g., "channel-0")
/// * `native_denom` - Native denom on the source chain (e.g., "uatom")
///
/// # Returns
/// IBC denom string (e.g., "ibc/27394FB092D2ECCD56123C74F36E4C1F926001CEADA9CA97EA622B25F41E5EB2")
pub fn calculate_ibc_denom(channel: &str, native_denom: &str) -> String {
    let path = format!("transfer/{}/{}", channel, native_denom);
    let mut hasher = Sha256::new();
    hasher.update(path.as_bytes());
    let hash = hasher.finalize();
    format!("ibc/{}", hex::encode_upper(hash))
}

/// Get IBC denom for a token on a specific chain
///
/// # Arguments
/// * `deps` - Cosmos dependencies
/// * `token_symbol` - Token symbol (e.g., "ATOM", "stATOM")
/// * `target_chain` - Target chain ID (e.g., "neutron-1", "osmosis-1")
///
/// # Returns
/// Denom on the target chain (either native denom or IBC denom)
///
/// # Logic
/// - If target is the token's native chain, returns native_denom
/// - Otherwise, calculates IBC denom through channel path from native chain to target chain
/// - Currently assumes 1-hop (direct channel from native chain to target)
pub fn get_token_denom_on_chain(
    deps: &Deps<NeutronQuery>,
    token_symbol: &str,
    target_chain: &str,
) -> StdResult<String> {
    // Load token info
    let token = TOKEN_REGISTRY.load(deps.storage, token_symbol.to_string())?;

    // If target is native chain, return native denom
    if token.native_chain == target_chain {
        return Ok(token.native_denom);
    }

    // Otherwise, calculate IBC denom through channel path
    // For now: assume 1-hop (token native chain → target chain)
    let channel = CHANNEL_REGISTRY.load(
        deps.storage,
        (token.native_chain.clone(), target_chain.to_string()),
    )?;

    Ok(calculate_ibc_denom(&channel.channel_id, &token.native_denom))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_ibc_denom() {
        // Test with known IBC denom
        // channel-0 + uatom should produce a specific hash
        let result = calculate_ibc_denom("channel-0", "uatom");

        // Verify format
        assert!(result.starts_with("ibc/"));
        assert_eq!(result.len(), 68); // "ibc/" (4) + 64 hex chars

        // Verify deterministic (same input = same output)
        let result2 = calculate_ibc_denom("channel-0", "uatom");
        assert_eq!(result, result2);

        // Different inputs produce different results
        let different = calculate_ibc_denom("channel-1", "uatom");
        assert_ne!(result, different);
    }

    #[test]
    fn test_ibc_denom_uppercase() {
        let result = calculate_ibc_denom("channel-0", "uatom");

        // Extract hash part (after "ibc/")
        let hash = &result[4..];

        // Verify all uppercase
        assert_eq!(hash, hash.to_uppercase());

        // Verify valid hex
        assert!(hex::decode(hash).is_ok());
    }
}
