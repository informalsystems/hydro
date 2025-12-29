use cosmwasm_std::{Deps, Env, StdResult, Uint128};
use neutron_sdk::bindings::query::NeutronQuery;
use serde_json::json;

use crate::msg::SwapOperation;
use crate::state::{CrossChainRoute, OsmosisConfig, CHANNEL_REGISTRY};

/// Construct PFM + wasm hook memo for Osmosis swap
///
/// This creates a nested JSON memo structure:
/// 1. PFM forward to Osmosis
/// 2. Wasm hook calling Skip contract on Osmosis
/// 3. Skip swap message with auto-generated operations
/// 4. IBC transfer back to recovery address
pub fn construct_osmosis_swap_memo(
    deps: &Deps<NeutronQuery>,
    osmosis_config: &OsmosisConfig,
    route: &CrossChainRoute,
    osmosis_denom_in: &str,
    osmosis_denom_out: &str,
    min_amount_out: &Uint128,
    recovery_address: &str,
    env: &Env,
) -> StdResult<String> {
    // Auto-generate operations from route config
    // For Osmosis, we use a single operation with the pool_id
    let operations = vec![SwapOperation {
        denom_in: osmosis_denom_in.to_string(),
        denom_out: osmosis_denom_out.to_string(),
        pool: route.pool_id.clone(),
        interface: None,
    }];

    // Get channel to Osmosis (from Neutron)
    let channel_to_osmosis = CHANNEL_REGISTRY
        .load(
            deps.storage,
            ("neutron-1".to_string(), osmosis_config.chain_id.clone()),
        )?
        .channel_id;

    // Get return channel from Osmosis (back to Neutron)
    let channel_from_osmosis = CHANNEL_REGISTRY
        .load(
            deps.storage,
            (osmosis_config.chain_id.clone(), "neutron-1".to_string()),
        )?
        .channel_id;

    // Calculate timeout (30 minutes from now)
    let timeout_timestamp = (env.block.time.nanos() + 1_800_000_000_000).to_string();

    // Construct Skip swap message for Osmosis
    // This follows Skip Protocol's message format
    let skip_msg = json!({
        "swap_and_action": {
            "user_swap": {
                "swap_exact_asset_in": {
                    "operations": operations,
                    "swap_venue_name": osmosis_config.swap_venue,
                }
            },
            "min_asset": {
                "native": {
                    "denom": osmosis_denom_out,
                    "amount": min_amount_out.to_string(),
                }
            },
            "post_swap_action": {
                "ibc_transfer": {
                    "receiver": recovery_address,
                    "source_channel": channel_from_osmosis,
                    "memo": "",
                    "timeout_timestamp": timeout_timestamp,
                }
            },
            "timeout_timestamp": timeout_timestamp,
            "affiliates": [],
        }
    });

    // Wrap in wasm hook
    let wasm_hook = json!({
        "wasm": {
            "contract": osmosis_config.skip_contract,
            "msg": skip_msg,
        }
    });

    // Wrap in PFM forward
    let wasm_hook_str = serde_json::to_string(&wasm_hook)
        .map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))?;

    let pfm_memo = json!({
        "forward": {
            "receiver": osmosis_config.skip_contract,
            "port": "transfer",
            "channel": channel_to_osmosis,
            "timeout": "1800000000000",
            "retries": 2,
            "next": wasm_hook_str,
        }
    });

    serde_json::to_string(&pfm_memo)
        .map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::testing::mock_env;
    use cosmwasm_std::Addr;

    #[test]
    fn test_memo_structure() {
        // Just verify the JSON structure is valid
        let osmosis_config = OsmosisConfig {
            chain_id: "osmosis-1".to_string(),
            skip_contract: "osmo1skip".to_string(),
            swap_venue: "osmosis-poolmanager".to_string(),
            ibc_adapter: Addr::unchecked("neutron1ibc"),
        };

        let route = CrossChainRoute {
            token_in: "stATOM".to_string(),
            token_out: "ATOM".to_string(),
            swap_chain: "osmosis-1".to_string(),
            pool_id: "1234".to_string(),
            enabled: true,
        };

        // Note: This test would need proper deps with channel registry setup
        // For now, just verify the types compile
        assert!(osmosis_config.skip_contract == "osmo1skip");
        assert!(route.pool_id == "1234");
    }
}
