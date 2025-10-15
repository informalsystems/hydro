use cosmwasm_schema::cw_serde;

// Re-export adapter interface message types
pub use adapter_interface::{
    AdapterConfigResponse, AdapterExecuteMsg, AdapterQueryMsg, AvailableAmountResponse,
    InflowDepositResponse, InflowDepositsResponse, RegisteredInflowInfo, RegisteredInflowsResponse,
    TimeEstimateResponse, TotalDepositedResponse,
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
    /// Optional: single inflow address to whitelist during instantiation
    pub inflow_address: Option<String>,
}
