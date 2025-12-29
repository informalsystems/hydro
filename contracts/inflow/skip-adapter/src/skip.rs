// Skip Protocol Integration Module
//
// This module contains message types and helper functions for interacting with Skip Protocol
// swap contract on Neutron.

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{to_json_binary, Addr, Coin, StdResult, Uint128, WasmMsg};

use crate::state::SwapOperation;

/// Skip Protocol Execute Messages
#[cw_serde]
pub enum SkipExecuteMsg {
    /// Execute a swap with optional post-swap action
    SwapAndAction {
        /// User swap specification
        user_swap: Swap,
        /// Minimum output asset (slippage protection)
        min_asset: SkipAsset,
        /// Post-swap action (transfer, IBC transfer, etc.)
        post_swap_action: SkipAction,
        /// Timeout timestamp in nanoseconds
        timeout_timestamp: u64,
        /// Affiliate fees (empty for now)
        affiliates: Vec<Affiliate>,
    },
}

/// Swap type enum
#[cw_serde]
pub enum Swap {
    /// Swap exact amount in
    SwapExactAssetIn(SwapExactAssetIn),
}

/// Swap exact asset in parameters
#[cw_serde]
pub struct SwapExactAssetIn {
    /// Swap operations (route hops)
    pub operations: Vec<SwapOperation>,
    /// Swap venue name (e.g., "neutron-astroport", "osmosis-poolmanager")
    pub swap_venue_name: String,
}

/// Asset type for Skip protocol - native tokens only
#[cw_serde]
pub enum SkipAsset {
    Native(Coin),
}

/// Action type for Skip protocol
#[cw_serde]
pub enum SkipAction {
    Transfer { to_address: String },
}

/// Affiliate fee structure
#[cw_serde]
pub struct Affiliate {
    pub address: String,
    pub basis_points_fee: Uint128,
}

/// Helper function to create Skip SwapAndAction message for Neutron swaps
#[allow(clippy::too_many_arguments)]
pub fn create_swap_and_action_msg(
    skip_contract: Addr,
    coin_in: Coin,
    operations: Vec<SwapOperation>,
    swap_venue_name: String,
    min_denom_out: String,
    min_amount_out: Uint128,
    recipient: String,
    timeout_timestamp: u64,
) -> StdResult<WasmMsg> {
    let msg = SkipExecuteMsg::SwapAndAction {
        user_swap: Swap::SwapExactAssetIn(SwapExactAssetIn {
            operations,
            swap_venue_name,
        }),
        min_asset: SkipAsset::Native(Coin {
            denom: min_denom_out,
            amount: min_amount_out,
        }),
        post_swap_action: SkipAction::Transfer {
            to_address: recipient,
        },
        timeout_timestamp,
        affiliates: vec![],
    };

    Ok(WasmMsg::Execute {
        contract_addr: skip_contract.to_string(),
        msg: to_json_binary(&msg)?,
        funds: vec![coin_in],
    })
}
