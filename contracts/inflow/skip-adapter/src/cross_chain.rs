use cosmwasm_std::{to_json_binary, Coin, Env, StdResult, Uint128, WasmMsg};
use serde_json::json;

use crate::state::{Config, PathHop, UnifiedRoute};

// Re-export IBC adapter types for use in tests and external contracts
pub use ibc_adapter::msg::{ExecuteMsg as IbcAdapterExecuteMsg, IbcAdapterMsg};
pub use ibc_adapter::state::TransferFundsInstructions;

// ============================================================================
// Public Functions
// ============================================================================

/// Build typed IBC adapter message for cross-chain swap with forward path
pub fn build_cross_chain_swap_ibc_adapter_msg(
    ibc_adapter_addr: String,
    coin: Coin,
    forward_path: &[PathHop],
    wasm_hook_memo: String,
    timeout_nanos: u64,
) -> StdResult<WasmMsg> {
    if forward_path.is_empty() {
        return Err(cosmwasm_std::StdError::generic_err(
            "Forward path cannot be empty for cross-chain swaps",
        ));
    }

    // Build PFM memo for hops AFTER the first one
    // The first hop is already specified in the IBC transfer itself (destination_chain, recipient)
    // PFM memo tells the receiving chain where to forward NEXT
    let pfm_memo = if forward_path.len() > 1 {
        // Multiple hops: build PFM forward for remaining hops (excluding first)
        build_pfm_forward_memo_with_payload(&forward_path[1..], &wasm_hook_memo, timeout_nanos)?
    } else {
        // Single hop (direct to swap chain): just use the wasm hook as memo
        wasm_hook_memo.clone()
    };

    // First hop determines the initial IBC transfer destination
    // IBC adapter sends to this chain, then PFM handles the forwarding
    let first_hop = &forward_path[0];

    let msg = IbcAdapterExecuteMsg::CustomAction(IbcAdapterMsg::TransferFunds {
        coin: coin.clone(),
        instructions: TransferFundsInstructions {
            destination_chain: first_hop.chain_id.clone(),
            recipient: first_hop.receiver.clone(),
            timeout_seconds: None, // Use IBC adapter's default
            memo: Some(pfm_memo),
        },
    });

    Ok(WasmMsg::Execute {
        contract_addr: ibc_adapter_addr,
        msg: to_json_binary(&msg)?,
        funds: vec![],
    })
}

/// Construct wasm hook memo for cross-chain swap
/// Based on real Skip entry-point format from examples-swaps.txt
pub fn construct_cross_chain_wasm_hook_memo(
    config: &Config,
    route: &UnifiedRoute,
    min_amount_out: &Uint128,
    env: &Env,
) -> StdResult<String> {
    let timeout = env.block.time.nanos() + config.default_timeout_nanos;

    // Get Skip contract for this venue
    let skip_contract = config.get_skip_contract(&route.swap_venue_name)?;

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
            "contract": skip_contract,
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
            "Cross chain route must have return path",
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

/// Build nested PFM forward memo for multi-hop paths (both forward and return)
///
/// This recursively builds nested PFM forward messages. For the final hop, you can optionally
/// provide a payload (e.g., wasm hook for forward path, or empty string for return path).
fn build_pfm_forward_memo(remaining_hops: &[PathHop], timeout: u64) -> StdResult<String> {
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

/// Build nested PFM forward memo with a final payload (e.g., wasm hook)
/// Used for forward paths where the final destination needs a wasm execution
fn build_pfm_forward_memo_with_payload(
    path_hops: &[PathHop],
    final_payload: &str,
    timeout: u64,
) -> StdResult<String> {
    if path_hops.is_empty() {
        return Err(cosmwasm_std::StdError::generic_err("Path cannot be empty"));
    }

    // Parse the payload as JSON
    let payload_json: serde_json::Value = serde_json::from_str(final_payload)
        .map_err(|e| cosmwasm_std::StdError::generic_err(format!("Invalid payload JSON: {}", e)))?;

    // Build from the end backwards to build nested structure
    let last_idx = path_hops.len() - 1;

    // Start with the final hop that contains the payload
    let mut current_memo = json!({
        "forward": {
            "channel": path_hops[last_idx].channel,
            "port": "transfer",
            "receiver": path_hops[last_idx].receiver,
            "retries": 2,
            "timeout": timeout,
            "next": payload_json
        }
    });

    // Build backwards from second-to-last hop to first
    for i in (0..last_idx).rev() {
        let hop = &path_hops[i];
        current_memo = json!({
            "forward": {
                "channel": hop.channel,
                "port": "transfer",
                "receiver": hop.receiver,
                "retries": 2,
                "timeout": timeout,
                "next": current_memo
            }
        });
    }

    serde_json::to_string(&current_memo)
        .map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))
}
