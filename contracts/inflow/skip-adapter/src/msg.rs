use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Binary, Uint128};

use crate::state::{SwapVenue, UnifiedRoute};

// Re-export adapter interface types
pub use interface::inflow_adapter::{
    AdapterInterfaceMsg, AdapterInterfaceQueryMsg, AllPositionsResponse, AvailableAmountResponse,
    DepositorPositionResponse, DepositorPositionsResponse, RegisteredDepositorInfo,
    RegisteredDepositorsResponse, TimeEstimateResponse,
};

// Re-export SwapOperation from state for convenience
pub use crate::state::SwapOperation;

// ============================================================================
// Instantiate
// ============================================================================

/// Message for instantiating the Skip adapter contract
#[cw_serde]
pub struct InstantiateMsg {
    /// The config admins who can update config and manage routes/executors
    pub admins: Vec<String>,
    /// Skip contract address on Neutron
    pub neutron_skip_contract: String,
    /// Skip contract address on Osmosis
    pub osmosis_skip_contract: String,
    /// IBC adapter contract address on Neutron
    pub ibc_adapter: String,
    /// IBC channel from Neutron to Osmosis
    pub osmosis_channel: String,
    /// Default timeout in nanoseconds (e.g., 1800000000000 for 30 minutes)
    pub default_timeout_nanos: u64,
    /// Maximum allowed slippage in basis points (e.g., 100 = 1%)
    pub max_slippage_bps: u64,
    /// Initial executors who can call ExecuteSwap (can be empty array)
    pub executors: Vec<String>,
    /// Initial route configurations to register (can be empty array)
    pub initial_routes: Vec<(String, UnifiedRoute)>,
    /// Initial depositor addresses to register during instantiation (can be empty array)
    pub initial_depositors: Vec<String>,
}

// ============================================================================
// Execute Messages
// ============================================================================

/// Top-level execute message wrapper for Skip adapter
#[cw_serde]
pub enum ExecuteMsg {
    /// Standard adapter interface messages (deposit, withdraw, manage depositors)
    StandardAction(AdapterInterfaceMsg),
    /// Skip adapter-specific custom messages
    CustomAction(SkipAdapterMsg),
}

/// Simplified swap parameters - unified for both Neutron and Osmosis
#[cw_serde]
pub struct SwapParams {
    /// Route ID (must be registered)
    pub route_id: String,
    /// Amount to swap
    pub amount_in: Uint128,
    /// Minimum output (slippage protection)
    pub min_amount_out: Uint128,
}

/// Skip adapter-specific execute messages
#[cw_serde]
pub enum SkipAdapterMsg {
    // =========================================================================
    // SWAP EXECUTION
    // =========================================================================
    /// Execute a swap (admin or executor only)
    /// Works for both Neutron and Osmosis venues - dispatches based on route config
    ExecuteSwap { params: SwapParams },

    // =========================================================================
    // ROUTE MANAGEMENT
    // =========================================================================
    /// Register a new route (admin only)
    RegisterRoute {
        route_id: String,
        route: UnifiedRoute,
    },

    /// Unregister a swap route (admin only)
    UnregisterRoute { route_id: String },

    /// Enable/disable a specific route (admin only)
    SetRouteEnabled { route_id: String, enabled: bool },

    // =========================================================================
    // EXECUTOR MANAGEMENT
    // =========================================================================
    /// Add a new executor (admin only)
    AddExecutor { executor_address: String },

    /// Remove an executor (admin only)
    RemoveExecutor { executor_address: String },

    // =========================================================================
    // CONFIG
    // =========================================================================
    /// Update contract configuration (admin only)
    UpdateConfig {
        neutron_skip_contract: Option<String>,
        osmosis_skip_contract: Option<String>,
        ibc_adapter: Option<String>,
        osmosis_channel: Option<String>,
        default_timeout_nanos: Option<u64>,
        max_slippage_bps: Option<u64>,
    },
}

// ============================================================================
// Query Messages
// ============================================================================

/// Top-level query message wrapper for Skip adapter
#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    /// Standard adapter interface queries
    #[returns(Binary)]
    StandardQuery(AdapterInterfaceQueryMsg),
    /// Skip adapter-specific custom queries
    #[returns(Binary)]
    CustomQuery(SkipAdapterQueryMsg),
}

/// Skip adapter-specific query messages
#[cw_serde]
#[derive(QueryResponses)]
pub enum SkipAdapterQueryMsg {
    /// Get contract configuration
    #[returns(SkipConfigResponse)]
    Config {},

    /// Get a specific route by ID
    #[returns(RouteResponse)]
    Route { route_id: String },

    /// Get all registered routes (with optional venue filter)
    #[returns(AllRoutesResponse)]
    AllRoutes { venue: Option<SwapVenue> },

    /// Get list of executors
    #[returns(ExecutorsResponse)]
    Executors {},
}

// ============================================================================
// Response Types
// ============================================================================

#[cw_serde]
pub struct SkipConfigResponse {
    pub admins: Vec<String>,
    pub neutron_skip_contract: String,
    pub osmosis_skip_contract: String,
    pub ibc_adapter: String,
    pub osmosis_channel: String,
    pub default_timeout_nanos: u64,
    pub max_slippage_bps: u64,
}

#[cw_serde]
pub struct RouteResponse {
    pub route_id: String,
    pub route: UnifiedRoute,
}

#[cw_serde]
pub struct AllRoutesResponse {
    pub routes: Vec<(String, UnifiedRoute)>,
}

#[cw_serde]
pub struct ExecutorsResponse {
    pub executors: Vec<String>,
}
