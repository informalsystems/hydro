use cosmwasm_std::{to_json_binary, Coin, Env, StdResult, Uint128, WasmMsg};
use serde_json::json;

use crate::state::{Config, UnifiedRoute};

// Re-export IBC adapter types for use in tests and external contracts
pub use ibc_adapter::msg::{ExecuteMsg as IbcAdapterExecuteMsg, IbcAdapterMsg};
pub use ibc_adapter::state::TransferFundsInstructions;

const OSMOSIS_CHAIN_ID: &str = "osmosis-1";

// ============================================================================
// Public Functions
// ============================================================================

/// Build typed IBC adapter message for Osmosis swap
pub fn build_osmosis_swap_ibc_adapter_msg(
    ibc_adapter_addr: String,
    coin: Coin,
    osmosis_recipient: String,
    memo: String,
) -> StdResult<WasmMsg> {
    let msg = IbcAdapterExecuteMsg::CustomAction(IbcAdapterMsg::TransferFunds {
        coin: coin.clone(),
        instructions: TransferFundsInstructions {
            destination_chain: OSMOSIS_CHAIN_ID.to_string(),
            recipient: osmosis_recipient,
            timeout_seconds: None, // Use IBC adapter's default
            memo: Some(memo),
        },
    });

    Ok(WasmMsg::Execute {
        contract_addr: ibc_adapter_addr,
        msg: to_json_binary(&msg)?,
        funds: vec![],
    })
}

/// Construct wasm hook memo for Osmosis swap
/// Based on real Skip entry-point format from examples-swaps.txt
pub fn construct_osmosis_wasm_hook_memo(
    config: &Config,
    route: &UnifiedRoute,
    min_amount_out: &Uint128,
    env: &Env,
) -> StdResult<String> {
    let timeout = env.block.time.nanos() + config.default_timeout_nanos;

    // Get output denom from last operation
    let output_denom = route
        .operations
        .last()
        .map(|op| op.denom_out.clone())
        .ok_or_else(|| cosmwasm_std::StdError::generic_err("Route has no operations"))?;

    // Build post_swap_action based on return path
    let post_swap_action = build_ibc_return_action(route, timeout)?;

    // Build Skip swap message
    let skip_msg = json!({
        "swap_and_action": {
            "user_swap": {
                "swap_exact_asset_in": {
                    "swap_venue_name": route.swap_venue_name,
                    "operations": route.operations,
                }
            },
            "min_asset": {
                "native": {
                    "denom": output_denom,
                    "amount": min_amount_out.to_string()
                }
            },
            "timeout_timestamp": timeout,
            "post_swap_action": post_swap_action,
            "affiliates": []
        }
    });

    // Wrap in wasm hook
    let wasm_hook = json!({
        "wasm": {
            "contract": config.osmosis_skip_contract,
            "msg": skip_msg
        }
    });

    serde_json::to_string(&wasm_hook)
        .map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))
}

// ============================================================================
// Internal Functions
// ============================================================================

/// Build ibc_transfer post_swap_action with potential PFM forward
fn build_ibc_return_action(route: &UnifiedRoute, timeout: u64) -> StdResult<serde_json::Value> {
    if route.return_path.is_empty() {
        return Err(cosmwasm_std::StdError::generic_err(
            "Osmosis route must have return path",
        ));
    }

    let first_hop = &route.return_path[0];

    // Build nested PFM forward memo for remaining hops
    let forward_memo = if route.return_path.len() > 1 {
        build_pfm_forward_memo(&route.return_path[1..], timeout)?
    } else {
        "".to_string()
    };

    Ok(json!({
        "ibc_transfer": {
            "ibc_info": {
                "source_channel": first_hop.channel,
                "receiver": first_hop.receiver,
                "memo": forward_memo,
                "recover_address": route.recover_address.as_ref().unwrap_or(&"".to_string())
            }
        }
    }))
}

/// Build nested PFM forward memo for multi-hop return
fn build_pfm_forward_memo(
    remaining_hops: &[crate::state::ReturnHop],
    timeout: u64,
) -> StdResult<String> {
    if remaining_hops.is_empty() {
        return Ok("".to_string());
    }

    let hop = &remaining_hops[0];
    let next_memo = if remaining_hops.len() > 1 {
        build_pfm_forward_memo(&remaining_hops[1..], timeout)?
    } else {
        "".to_string()
    };

    let mut forward = json!({
        "forward": {
            "channel": hop.channel,
            "port": "transfer",
            "receiver": hop.receiver,
            "retries": 2,
            "timeout": timeout
        }
    });

    // If there are more hops, nest the memo
    if !next_memo.is_empty() {
        if let Some(forward_obj) = forward.get_mut("forward") {
            if let Some(obj) = forward_obj.as_object_mut() {
                obj.insert("next".to_string(), serde_json::Value::String(next_memo));
            }
        }
    }

    serde_json::to_string(&forward).map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))
}
