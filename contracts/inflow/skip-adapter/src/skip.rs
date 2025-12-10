// Skip Protocol Integration Module
//
// This module contains message types and helper functions for interacting with Skip Protocol
// swap contract on Neutron.

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{to_json_binary, Addr, Binary, Coin, StdResult, Uint128, WasmMsg};

use crate::msg::{Asset, PostSwapAction, SwapOperation};
use crate::state::RouteConfig;

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
        /// Optional: sent asset specification
        sent_asset: Option<SkipAsset>,
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
    pub operations: Vec<SkipSwapOperation>,
    /// Swap venue name (e.g., "neutron-duality", "neutron-astroport")
    pub swap_venue_name: String,
}

/// Skip swap operation (single hop)
/// Matches Skip Protocol schema exactly
#[cw_serde]
pub struct SkipSwapOperation {
    /// Input denom
    pub denom_in: String,
    /// Output denom
    pub denom_out: String,
    /// Pool identifier
    pub pool: String,
    /// Optional interface specification
    pub interface: Option<Binary>,
}

/// Asset type for Skip protocol (matches schema)
#[cw_serde]
pub enum SkipAsset {
    Native(Coin),
    Cw20(Cw20Coin),
}

/// CW20 coin structure
#[cw_serde]
pub struct Cw20Coin {
    pub address: String,
    pub amount: Uint128,
}

/// Action type for Skip protocol (matches schema)
#[cw_serde]
pub enum SkipAction {
    Transfer { to_address: String },
    // Future: IbcTransfer, ContractCall, HplTransfer, etc.
}

/// Affiliate fee structure
#[cw_serde]
pub struct Affiliate {
    pub address: String,
    pub basis_points_fee: Uint128,
}

/// Helper function to create Skip SwapAndAction message
pub fn create_swap_and_action_msg(
    skip_contract: Addr,
    coin_in: Coin,
    operations: Vec<SwapOperation>,
    swap_venue_name: String,
    min_asset: Asset,
    post_swap_action: Option<PostSwapAction>,
    timeout_timestamp: u64,
) -> StdResult<WasmMsg> {
    // Convert operations to Skip format
    let skip_operations: Vec<SkipSwapOperation> = operations
        .into_iter()
        .map(|op| SkipSwapOperation {
            denom_in: op.denom_in,
            denom_out: op.denom_out,
            pool: op.pool,
            interface: op.interface,
        })
        .collect();

    // Convert min_asset to Skip format
    let skip_min_asset = match min_asset {
        Asset::Native { denom, amount } => SkipAsset::Native(Coin { denom, amount }),
        Asset::Cw20 { address, amount } => SkipAsset::Cw20(Cw20Coin { address, amount }),
    };

    // Convert post_swap_action to Skip format
    // Default to transfer back to adapter if no action specified
    let skip_post_swap_action = match post_swap_action {
        Some(PostSwapAction::Transfer { to_address }) => SkipAction::Transfer { to_address },
        None => SkipAction::Transfer {
            to_address: skip_contract.to_string(), // Keep funds in adapter
        },
    };

    let msg = SkipExecuteMsg::SwapAndAction {
        user_swap: Swap::SwapExactAssetIn(SwapExactAssetIn {
            operations: skip_operations,
            swap_venue_name,
        }),
        min_asset: skip_min_asset,
        post_swap_action: skip_post_swap_action,
        timeout_timestamp,
        affiliates: vec![], // Empty for now
        sent_asset: None,   // Optional field, not needed for our use case
    };

    Ok(WasmMsg::Execute {
        contract_addr: skip_contract.to_string(),
        msg: to_json_binary(&msg)?,
        funds: vec![coin_in],
    })
}

/// Validates that coin_in denom matches route's denom_in
pub fn validate_coin_in_matches_route(
    coin_in: &Coin,
    route_config: &RouteConfig,
) -> Result<(), String> {
    if coin_in.denom != route_config.denom_in {
        return Err(format!(
            "Coin denom ({}) does not match route denom_in ({})",
            coin_in.denom, route_config.denom_in
        ));
    }
    Ok(())
}
