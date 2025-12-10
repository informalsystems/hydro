use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Binary, Coin, Uint128};

use crate::state::{RecipientConfig, RouteConfig};

// Re-export adapter interface types
pub use interface::inflow_adapter::{
    AdapterInterfaceMsg, AdapterInterfaceQueryMsg, AllPositionsResponse, AvailableAmountResponse,
    DepositorPositionResponse, DepositorPositionsResponse, RegisteredDepositorInfo,
    RegisteredDepositorsResponse, TimeEstimateResponse,
};

/// Message for instantiating the Skip adapter contract
#[cw_serde]
pub struct InstantiateMsg {
    /// The config admins who can update config and manage routes/recipients/executors
    pub admins: Vec<String>,
    /// Skip contract address on Neutron
    pub skip_contract: String,
    /// Default timeout in nanoseconds (e.g., 1800000000000 for 30 minutes)
    pub default_timeout_nanos: u64,
    /// Optional: initial executors who can call ExecuteSwap
    pub executors: Option<Vec<String>>,
    /// Optional: initial route configurations to register
    pub initial_routes: Option<Vec<(String, RouteConfig)>>,
    /// Optional: initial recipient configurations to register
    pub initial_recipients: Option<Vec<(String, RecipientConfig)>>,
    /// Optional: single depositor address to register during instantiation
    pub depositor_address: Option<String>,
}

/// Top-level execute message wrapper for Skip adapter
#[cw_serde]
pub enum ExecuteMsg {
    /// Standard adapter interface messages (deposit, withdraw, manage depositors)
    StandardAction(AdapterInterfaceMsg),
    /// Skip adapter-specific custom messages
    CustomAction(SkipAdapterMsg),
}

/// Swap execution parameters provided by executor
#[cw_serde]
pub struct SwapExecutionParams {
    /// Route identifier (must match a registered route)
    pub route_id: String,
    /// Input coin to swap (must be deposited in adapter)
    pub coin_in: Coin,
    /// Minimum output asset specification (executor calculates)
    pub min_asset: Asset,
    /// Swap operations (pool IDs and venues)
    pub operations: Vec<SwapOperation>,
    /// Swap venue name (e.g., "neutron-duality", "neutron-astroport")
    pub swap_venue_name: String,
    /// Optional: post-swap action
    pub post_swap_action: Option<PostSwapAction>,
    /// Optional: timeout override (nanoseconds)
    pub timeout_nanos: Option<u64>,
}

/// Swap operation details (pool + denoms)
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
    pub interface: Option<Binary>,
}

/// Asset specification (matches Skip Protocol's Asset type)
#[cw_serde]
pub enum Asset {
    Native { denom: String, amount: Uint128 },
    Cw20 { address: String, amount: Uint128 },
}

/// Post-swap action options
#[cw_serde]
pub enum PostSwapAction {
    /// Transfer to a recipient (must be registered)
    Transfer { to_address: String },
    // Future: could add IbcTransfer, ContractCall, etc.
}

/// Skip adapter-specific execute messages
#[cw_serde]
pub enum SkipAdapterMsg {
    /// Execute a swap (admin or executor only)
    ExecuteSwap { params: SwapExecutionParams },

    /// Add a new executor (config admin only)
    AddExecutor { executor_address: String },

    /// Remove an executor (config admin only)
    RemoveExecutor { executor_address: String },

    /// Register or update a swap route (config admin only)
    RegisterRoute {
        route_id: String,
        route_config: RouteConfig,
    },

    /// Unregister a swap route (config admin only)
    UnregisterRoute { route_id: String },

    /// Enable/disable a specific route (config admin only)
    SetRouteEnabled { route_id: String, enabled: bool },

    /// Register or update a recipient (config admin only)
    RegisterRecipient {
        recipient_address: String,
        description: Option<String>,
    },

    /// Unregister a recipient (config admin only)
    UnregisterRecipient { recipient_address: String },

    /// Enable/disable a specific recipient (config admin only)
    SetRecipientEnabled {
        recipient_address: String,
        enabled: bool,
    },

    /// Update contract configuration (config admin only)
    UpdateConfig {
        skip_contract: Option<String>,
        default_timeout_nanos: Option<u64>,
    },
}

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

    /// Get route configuration
    #[returns(RouteConfigResponse)]
    RouteConfig { route_id: String },

    /// Get all registered routes
    #[returns(AllRoutesResponse)]
    AllRoutes {},

    /// Get recipient configuration
    #[returns(RecipientConfigResponse)]
    RecipientConfig { recipient_address: String },

    /// Get all registered recipients
    #[returns(AllRecipientsResponse)]
    AllRecipients {},

    /// Get list of executors
    #[returns(ExecutorsResponse)]
    Executors {},
}

// Response types

#[cw_serde]
pub struct SkipConfigResponse {
    pub admins: Vec<String>,
    pub skip_contract: String,
    pub default_timeout_nanos: u64,
}

#[cw_serde]
pub struct RouteConfigResponse {
    pub route_id: String,
    pub route_config: RouteConfig,
}

#[cw_serde]
pub struct AllRoutesResponse {
    pub routes: Vec<(String, RouteConfig)>,
}

#[cw_serde]
pub struct RecipientConfigResponse {
    pub recipient_config: RecipientConfig,
}

#[cw_serde]
pub struct AllRecipientsResponse {
    pub recipients: Vec<(String, RecipientConfig)>,
}

#[cw_serde]
pub struct ExecutorsResponse {
    pub executors: Vec<String>,
}
