use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Binary};
use cw_storage_plus::{Item, Map};

// ============================================================================
// Configuration
// ============================================================================

/// Contract configuration - unified for both Neutron and Osmosis venues
#[cw_serde]
pub struct Config {
    /// Skip contract address on Neutron
    pub neutron_skip_contract: Addr,
    /// Skip contract address on Osmosis (for wasm hook)
    pub osmosis_skip_contract: String,
    /// IBC adapter contract address on Neutron
    pub ibc_adapter: Addr,
    /// Default timeout in nanoseconds (e.g., 1800000000000 = 30 min)
    pub default_timeout_nanos: u64,
    /// Maximum slippage in basis points (e.g., 100 = 1%)
    pub max_slippage_bps: u64,
}

// ============================================================================
// Unified Route System
// ============================================================================

/// Swap venue - where the swap executes
#[cw_serde]
pub enum SwapVenue {
    /// Swap on Neutron (Astroport via Skip)
    NeutronAstroport,
    /// Swap on Osmosis (via PFM + wasm hook)
    Osmosis,
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
    /// Optional interface specification
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interface: Option<Binary>,
}

/// A single hop in an IBC path (used for both forward and return paths)
/// Used for multi-chain transfers (e.g., Neutron → Cosmos → Osmosis or Osmosis → Stride → Neutron)
#[cw_serde]
pub struct PathHop {
    /// IBC channel to use for this hop
    pub channel: String,
    /// Receiver address on the next chain (intermediary or final)
    pub receiver: String,
}

/// Unified route configuration - works for both Neutron and Osmosis swaps
#[cw_serde]
pub struct UnifiedRoute {
    /// Where the swap executes
    pub venue: SwapVenue,

    /// Input token denom on Neutron (what the adapter holds)
    pub denom_in: String,

    /// Output token denom on Neutron (what we expect back)
    pub denom_out: String,

    /// Swap operations (multi-hop path)
    /// Denoms are AS THEY APPEAR ON THE SWAP VENUE (Neutron or Osmosis)
    pub operations: Vec<SwapOperation>,

    /// Swap venue name (e.g., "neutron-astroport", "osmosis-poolmanager")
    pub swap_venue_name: String,

    /// For Osmosis routes: forward path from Neutron to Osmosis
    /// Empty for Neutron routes
    /// Example: [Neutron → Cosmos Hub → Osmosis]
    pub forward_path: Vec<PathHop>,

    /// For Osmosis routes: return path back to Neutron
    /// Empty for Neutron routes
    /// Example: [Osmosis → Stride → Neutron]
    pub return_path: Vec<PathHop>,

    /// Recovery address on the swap venue (for Osmosis: osmo1..., for Neutron: not needed)
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
pub const ADMINS: Item<Vec<Addr>> = Item::new("admins");

/// List of executor addresses (can execute swaps)
pub const EXECUTORS: Item<Vec<Addr>> = Item::new("executors");

/// Maps depositor address to their info
pub const WHITELISTED_DEPOSITORS: Map<Addr, Depositor> = Map::new("whitelisted_depositors");

/// Maps route identifier to unified route configuration
/// Key: route_id (e.g., "astro_to_ntrn", "ntrn_to_scrt_osmosis")
pub const ROUTES: Map<String, UnifiedRoute> = Map::new("routes");
