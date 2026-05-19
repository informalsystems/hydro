use cosmwasm_schema::{cw_serde, QueryResponses};

use crate::state::{
    ChainConfig, Config, DepositorCapabilities, TokenConfig, TransferFundsInstructions,
};

// Re-export adapter interface types
pub use interface::inflow_adapter::{
    AdapterInterfaceMsg, AdapterInterfaceQueryMsg, AdminsResponse, AllPositionsResponse,
    AvailableAmountResponse, DepositorPositionResponse, DepositorPositionsResponse,
    RegisteredDepositorInfo, RegisteredDepositorsResponse, TimeEstimateResponse,
};

/// Initial depositor configuration for instantiation
#[cw_serde]
pub struct InitialDepositor {
    pub address: String,
    /// If not provided, defaults to { can_withdraw: true }
    pub capabilities: Option<DepositorCapabilities>,
}

/// Initial executor configuration for instantiation
#[cw_serde]
pub struct InitialExecutor {
    pub address: String,
}

/// Initial chain configuration with allowed destination addresses
#[cw_serde]
pub struct InitialChainConfig {
    pub chain_config: ChainConfig,
    pub initial_allowed_destination_addresses: Vec<String>,
}

/// Message for instantiating the Eureka adapter contract
#[cw_serde]
pub struct InstantiateMsg {
    /// Config admins who can manage chains, tokens, depositors, and executors
    pub admins: Vec<String>,
    /// Skip swap entry point contract on Cosmos Hub
    pub skip_entry_point: String,
    /// Skip swap IBC adapter contract on Cosmos Hub
    pub skip_ibc_adapter: String,
    /// IBC channel from Neutron to Cosmos Hub (e.g. "channel-1")
    pub neutron_to_hub_channel: String,
    /// Default IBC timeout in seconds
    pub ibc_default_timeout_seconds: u64,
    /// Initial depositors (can be empty)
    pub initial_depositors: Vec<InitialDepositor>,
    /// Initial chain configurations with allowed EVM destination addresses (can be empty)
    pub initial_chains: Vec<InitialChainConfig>,
    /// Initial tokens supported for bridging (can be empty)
    pub initial_tokens: Vec<TokenConfig>,
    /// Initial executors who can perform TransferFunds (can be empty)
    pub initial_executors: Vec<InitialExecutor>,
    /// Initial allowed recover addresses on Cosmos Hub (can be empty)
    pub initial_recover_addresses: Vec<String>,
}

/// Top-level execute message wrapper for Eureka adapter
#[cw_serde]
pub enum ExecuteMsg {
    StandardAction(AdapterInterfaceMsg),
    CustomAction(EurekaAdapterMsg),
}

/// Data for updating the global contract config
#[cw_serde]
pub struct UpdateConfigData {
    pub skip_entry_point: Option<String>,
    pub skip_ibc_adapter: Option<String>,
    pub neutron_to_hub_channel: Option<String>,
    pub ibc_default_timeout_seconds: Option<u64>,
}

/// Eureka adapter-specific execute messages
#[cw_serde]
pub enum EurekaAdapterMsg {
    /// Transfer funds via IBC Eureka to an EVM chain (executor only)
    TransferFunds {
        instructions: TransferFundsInstructions,
    },

    /// Update global contract configuration (admin only)
    UpdateConfig { update: UpdateConfigData },

    /// Add a new executor (admin only)
    AddExecutor { executor_address: String },

    /// Remove an executor (admin only)
    RemoveExecutor { executor_address: String },

    /// Register a new EVM chain configuration (admin only)
    RegisterChain { chain_config: ChainConfig },

    /// Update an existing chain configuration (admin only)
    UpdateRegisteredChain { chain_config: ChainConfig },

    /// Unregister a chain (admin only)
    UnregisterChain { chain_id: String },

    /// Register a token for bridging (admin only)
    RegisterToken { denom: String, hub_denom: String },

    /// Unregister a token (admin only)
    UnregisterToken { denom: String },

    /// Add an allowed EVM destination address for a chain (admin only)
    AddAllowedDestinationAddress { chain_id: String, address: String },

    /// Remove an allowed EVM destination address (admin only)
    RemoveAllowedDestinationAddress { chain_id: String, address: String },

    /// Add an allowed Cosmos Hub recover address (admin only)
    AddAllowedRecoverAddress { address: String },

    /// Remove an allowed recover address (admin only)
    RemoveAllowedRecoverAddress { address: String },
}

/// Top-level query message wrapper for Eureka adapter
#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(cosmwasm_std::Binary)]
    StandardQuery(AdapterInterfaceQueryMsg),
    #[returns(cosmwasm_std::Binary)]
    CustomQuery(EurekaAdapterQueryMsg),
}

/// Eureka adapter-specific query messages
#[cw_serde]
#[derive(QueryResponses)]
pub enum EurekaAdapterQueryMsg {
    #[returns(ConfigResponse)]
    Config {},

    #[returns(ChainConfigResponse)]
    ChainConfig { chain_id: String },

    #[returns(AllChainsResponse)]
    AllChains {},

    #[returns(TokenConfigResponse)]
    TokenConfig { denom: String },

    #[returns(AllTokensResponse)]
    AllTokens {},

    #[returns(ExecutorsResponse)]
    Executors {},

    #[returns(DepositorCapabilitiesResponse)]
    DepositorCapabilities { depositor_address: String },

    #[returns(AllowedDestinationAddressesResponse)]
    AllowedDestinationAddresses {
        chain_id: String,
        start_after: Option<String>,
        limit: Option<u32>,
    },

    #[returns(AllowedRecoverAddressesResponse)]
    AllowedRecoverAddresses {
        start_after: Option<String>,
        limit: Option<u32>,
    },
}

// Response types

#[cw_serde]
pub struct ConfigResponse {
    pub config: Config,
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
}

#[cw_serde]
pub struct ExecutorsResponse {
    pub executors: Vec<ExecutorInfo>,
}

#[cw_serde]
pub struct DepositorCapabilitiesResponse {
    pub capabilities: DepositorCapabilities,
}

#[cw_serde]
pub struct AllowedDestinationAddressesResponse {
    pub addresses: Vec<String>,
}

#[cw_serde]
pub struct AllowedRecoverAddressesResponse {
    pub addresses: Vec<String>,
}
