use cosmwasm_schema::{cw_serde, QueryResponses};

// Re-export adapter interface types and response types
pub use interface::inflow_adapter::{
    AdapterInterfaceMsg, AdapterInterfaceQueryMsg, AllPositionsResponse, AvailableAmountResponse,
    DepositorPositionResponse, DepositorPositionsResponse, RegisteredDepositorInfo,
    RegisteredDepositorsResponse, TimeEstimateResponse,
};

/// Message for instantiating the Mars adapter contract
#[cw_serde]
pub struct InstantiateMsg {
    /// The admins who can update the config
    pub admins: Vec<String>,
    /// Mars credit manager contract address
    pub mars_contract: String,
    /// List of supported token denoms (e.g., USDC IBC denom)
    pub supported_denoms: Vec<String>,
    /// Optional: single depositor address to whitelist during instantiation
    pub depositor_address: Option<String>,
}

/// Top-level execute message wrapper for Mars adapter
#[cw_serde]
pub enum ExecuteMsg {
    /// Standard adapter interface messages (deposit, withdraw, manage depositors)
    StandardAction(AdapterInterfaceMsg),
    /// Mars adapter-specific custom messages
    CustomAction(MarsAdapterMsg),
}

/// Mars adapter-specific execute messages
/// Currently minimal as Mars operations are handled via interface
#[cw_serde]
pub enum MarsAdapterMsg {
    /// Update Mars adapter configuration (admin-only)
    UpdateConfig {
        mars_contract: Option<String>,
        supported_denoms: Option<Vec<String>>,
    },
}

/// Top-level query message wrapper for Mars adapter
#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    /// Standard adapter interface queries
    #[returns(cosmwasm_std::Binary)]
    StandardQuery(AdapterInterfaceQueryMsg),
    /// Mars adapter-specific custom queries (none currently)
    #[returns(cosmwasm_std::Binary)]
    CustomQuery(MarsAdapterQueryMsg),
}

/// Mars adapter-specific query messages
#[cw_serde]
#[derive(QueryResponses)]
pub enum MarsAdapterQueryMsg {
    // Placeholder for future Mars-specific queries
}

// Response types for Mars-specific queries

#[cw_serde]
pub struct MarsConfigResponse {
    pub admins: Vec<String>,
    pub mars_contract: String,
    pub supported_denoms: Vec<String>,
}
