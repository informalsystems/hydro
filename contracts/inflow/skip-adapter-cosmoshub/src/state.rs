use std::collections::BTreeMap;

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Binary, StdError, StdResult};
use cw_storage_plus::{Item, Map};

// ============================================================================
// Configuration
// ============================================================================

/// Contract configuration for Cosmos Hub — cross-chain venues only
#[cw_serde]
pub struct Config {
    /// Skip contract addresses by chain (e.g., "osmosis" -> "osmo1...")
    /// Each chain has one Skip entry-point contract
    pub skip_contracts: BTreeMap<String, String>,
    /// Default timeout in nanoseconds (e.g., 1800000000000 = 30 min)
    pub default_timeout_nanos: u64,
    /// Maximum slippage in basis points (e.g., 100 = 1%)
    pub max_slippage_bps: u64,
}

impl Config {
    /// Get the Skip contract address for a given swap venue name
    /// Extracts the chain from venue name (e.g., "osmosis" from "osmosis-poolmanager") and looks it up
    pub fn get_skip_contract(&self, swap_venue_name: &str) -> StdResult<&String> {
        // Extract chain from venue name (e.g., "osmosis-poolmanager" -> "osmosis")
        let chain = swap_venue_name.split('-').next().unwrap_or(swap_venue_name);

        self.skip_contracts.get(chain).ok_or_else(|| {
            StdError::generic_err(format!(
                "No Skip contract configured for chain '{}' (from venue: {})",
                chain, swap_venue_name
            ))
        })
    }
}

// ============================================================================
// Unified Route System
// ============================================================================

/// Swap venue — on Cosmos Hub only cross-chain venues are supported
#[cw_serde]
pub enum SwapVenue {
    /// Swap on Osmosis (via PFM + wasm hook)
    Osmosis,
}

impl SwapVenue {
    /// Returns true if this venue executes swaps locally — always false on Cosmos Hub
    pub fn is_local(&self) -> bool {
        false
    }

    /// Returns true if this venue executes swaps cross-chain — always true on Cosmos Hub
    pub fn is_cross_chain(&self) -> bool {
        true
    }
}

/// A single swap operation (hop in the swap path)
/// Matches Skip Protocol's SwapOperation schema
#[cw_serde]
pub struct SwapOperation {
    /// Input denom for this hop
    pub denom_in: String,
    /// Output denom for this hop
    pub denom_out: String,
    /// Pool identifier
    pub pool: String,
    /// Optional interface specification returned by Coinhall API.
    /// Not generated internally; passed through when present in API responses.
    /// See: https://github.com/skip-mev/skip-go-cosmwasm-contracts/pull/108#issuecomment-2099460954
    /// And: https://github.com/skip-mev/skip-go-cosmwasm-contracts/blob/main/contracts/adapters/swap/hallswap/src/contract.rs#L364-L367
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interface: Option<Binary>,
}

/// A single hop in an IBC path (used for both forward and return paths)
/// Used for multi-chain transfers (e.g., Cosmos Hub → Osmosis or Osmosis → Cosmos Hub)
#[cw_serde]
pub struct PathHop {
    /// Chain ID of the destination for this hop (e.g., "cosmoshub-4", "osmosis-1")
    pub chain_id: String,
    /// IBC channel to use for this hop
    pub channel: String,
    /// Receiver address on the next chain (intermediary or final)
    pub receiver: String,
}

/// Unified route configuration — cross-chain (Osmosis) only on Cosmos Hub
#[cw_serde]
pub struct UnifiedRoute {
    /// Where the swap executes
    pub venue: SwapVenue,

    /// Input token denom on Cosmos Hub (what the adapter holds)
    pub denom_in: String,

    /// Output token denom on Cosmos Hub (what we expect back)
    pub denom_out: String,

    /// Swap operations (multi-hop path)
    /// Denoms are AS THEY APPEAR ON THE SWAP VENUE (e.g., Osmosis)
    pub operations: Vec<SwapOperation>,

    /// Swap venue name (e.g., "osmosis-poolmanager")
    pub swap_venue_name: String,

    /// Forward path from Cosmos Hub to the swap venue
    /// Empty for local routes
    /// Example: [Cosmos Hub → Osmosis]
    pub forward_path: Vec<PathHop>,

    /// Return path back to Cosmos Hub
    /// Empty for local routes
    /// Example: [Osmosis → Cosmos Hub]
    pub return_path: Vec<PathHop>,

    /// Recovery address on the swap venue (e.g., osmo1...). Will be used as final destination if return_path is empty on cross-chain routes.
    pub recover_address: Option<String>,

    /// Whether route is enabled
    pub enabled: bool,
}

// ============================================================================
// Depositors
// ============================================================================

/// Depositor information
#[cw_serde]
pub struct Depositor {
    /// Whether this depositor is currently enabled
    pub enabled: bool,
}

// ============================================================================
// Storage Items
// ============================================================================

/// Configuration storage
pub const CONFIG: Item<Config> = Item::new("config");

/// List of admin addresses (config management)
pub const ADMINS: Item<Vec<cosmwasm_std::Addr>> = Item::new("admins");

/// List of executor addresses (can execute swaps)
pub const EXECUTORS: Item<Vec<cosmwasm_std::Addr>> = Item::new("executors");

/// Maps depositor address to their info
pub const WHITELISTED_DEPOSITORS: Map<cosmwasm_std::Addr, Depositor> =
    Map::new("whitelisted_depositors");

/// Maps route identifier to unified route configuration
/// Key: route_id (e.g., "atom_to_statom_osmosis")
pub const ROUTES: Map<String, UnifiedRoute> = Map::new("routes");
