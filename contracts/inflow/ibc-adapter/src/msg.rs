use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Binary, Coin};

use crate::state::{
    ChainConfig, DepositorCapabilities, ExecutorCapabilities, TokenConfig,
    TransferFundsInstructions,
};

// Re-export adapter interface types and response types
pub use interface::inflow_adapter::{
    AdapterInterfaceMsg, AdapterInterfaceQueryMsg, AllPositionsResponse, AvailableAmountResponse,
    DepositorPositionResponse, DepositorPositionsResponse, RegisteredDepositorInfo,
    RegisteredDepositorsResponse, TimeEstimateResponse,
};

/// Initial depositor configuration for instantiation
#[cw_serde]
pub struct InitialDepositor {
    /// Depositor address to register
    pub address: String,
    /// Optional capabilities for this depositor (serialized as Binary)
    /// If not provided, defaults to { can_withdraw: true }
    pub capabilities: Option<Binary>,
}

/// Initial executor configuration for instantiation
#[cw_serde]
pub struct InitialExecutor {
    /// Executor address to register
    pub address: String,
    /// Optional capabilities for this executor
    /// If not provided, defaults to { can_set_memo: false }
    pub capabilities: Option<ExecutorCapabilities>,
}

/// Message for instantiating the IBC adapter contract
#[cw_serde]
pub struct InstantiateMsg {
    /// The config admins who can update the config and manage chains/depositors/executors
    pub admins: Vec<String>,
    /// Initial depositors to register during instantiation (can be empty array)
    pub initial_depositors: Vec<InitialDepositor>,
    /// Default IBC timeout in seconds
    pub default_timeout_seconds: u64,
    /// Initial chain configurations to register (can be empty array)
    pub initial_chains: Vec<ChainConfig>,
    /// Initial token configurations to register (can be empty array)
    pub initial_tokens: Vec<TokenConfig>,
    /// Initial executors who can call TransferFunds (can be empty array)
    pub initial_executors: Vec<InitialExecutor>,
}

/// Top-level execute message wrapper for IBC adapter
#[cw_serde]
pub enum ExecuteMsg {
    /// Standard adapter interface messages (deposit, withdraw, manage depositors)
    StandardAction(AdapterInterfaceMsg),
    /// IBC adapter-specific custom messages
    CustomAction(IbcAdapterMsg),
}

/// IBC adapter-specific execute messages
#[cw_serde]
pub enum IbcAdapterMsg {
    /// Transfer funds via IBC (admin or executor, executed after deposit)
    /// This routes deposited funds to destination chains
    TransferFunds {
        coin: Coin,
        instructions: TransferFundsInstructions,
    },

    /// Add a new executor (config admin only)
    AddExecutor {
        executor_address: String,
        capabilities: Option<ExecutorCapabilities>,
    },

    /// Remove an executor (config admin only)
    RemoveExecutor { executor_address: String },

    /// Set executor capabilities (config admin only)
    SetExecutorCapabilities {
        executor_address: String,
        capabilities: ExecutorCapabilities,
    },

    /// Register or update chain configuration (config admin only)
    RegisterChain {
        chain_id: String,
        channel_from_neutron: String,
        allowed_recipients: Vec<String>,
    },

    /// Unregister a chain (config admin only)
    UnregisterChain { chain_id: String },

    /// Register a token with its source chain (config admin only)
    RegisterToken {
        denom: String,
        source_chain_id: String,
    },

    /// Unregister a token (config admin only)
    UnregisterToken { denom: String },

    /// Update contract configuration (config admin only)
    UpdateConfig { default_timeout_seconds: u64 },
}

/// Top-level query message wrapper for IBC adapter
#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    /// Standard adapter interface queries
    #[returns(cosmwasm_std::Binary)]
    StandardQuery(AdapterInterfaceQueryMsg),
    /// IBC adapter-specific custom queries
    #[returns(cosmwasm_std::Binary)]
    CustomQuery(IbcAdapterQueryMsg),
}

/// IBC adapter-specific query messages
#[cw_serde]
#[derive(QueryResponses)]
pub enum IbcAdapterQueryMsg {
    /// Get chain configuration
    #[returns(ChainConfigResponse)]
    ChainConfig { chain_id: String },

    /// Get all registered chains
    #[returns(AllChainsResponse)]
    AllChains {},

    /// Get token configuration
    #[returns(TokenConfigResponse)]
    TokenConfig { denom: String },

    /// Get all registered tokens
    #[returns(AllTokensResponse)]
    AllTokens {},

    /// Get list of executors
    #[returns(ExecutorsResponse)]
    Executors {},

    /// Get executor capabilities
    #[returns(ExecutorCapabilitiesResponse)]
    ExecutorCapabilities { executor_address: String },

    /// Get depositor capabilities
    #[returns(DepositorCapabilitiesResponse)]
    DepositorCapabilities { depositor_address: String },
}

// Response types for IBC-specific queries

#[cw_serde]
pub struct IbcConfigResponse {
    pub admins: Vec<String>,
    pub default_timeout_seconds: u64,
}

#[cw_serde]
pub struct ChainConfigResponse {
    pub chain_config: ChainConfig,
}

#[cw_serde]
pub struct AllChainsResponse {
    pub chains: Vec<ChainConfig>,
}

#[cw_serde]
pub struct TokenConfigResponse {
    pub token_config: TokenConfig,
}

#[cw_serde]
pub struct AllTokensResponse {
    pub tokens: Vec<TokenConfig>,
}

#[cw_serde]
pub struct ExecutorInfo {
    pub executor_address: String,
    pub capabilities: ExecutorCapabilities,
}

#[cw_serde]
pub struct ExecutorsResponse {
    pub executors: Vec<ExecutorInfo>,
}

#[cw_serde]
pub struct ExecutorCapabilitiesResponse {
    pub capabilities: ExecutorCapabilities,
}

#[cw_serde]
pub struct DepositorCapabilitiesResponse {
    pub capabilities: DepositorCapabilities,
}
