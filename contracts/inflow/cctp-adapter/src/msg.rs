use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::Uint128;

use crate::state::{
    ChainConfig, Config, DepositorCapabilities, DestinationAddress, TransferFundsInstructions,
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
    /// Optional capabilities for this depositor (typed, not Binary)
    /// If not provided, defaults to { can_withdraw: true }
    pub capabilities: Option<DepositorCapabilities>,
}

/// Initial executor configuration for instantiation
#[cw_serde]
pub struct InitialExecutor {
    /// Executor address to register
    pub address: String,
}

/// Initial chain configuration with allowed destination addresses
#[cw_serde]
pub struct InitialChainConfig {
    /// Chain configuration (stored in CHAIN_REGISTRY)
    pub chain_config: ChainConfig,
    /// Initial allowed destination addresses for this chain
    pub initial_allowed_destination_addresses: Vec<DestinationAddress>,
}

/// Message for instantiating the CCTP adapter contract
#[cw_serde]
pub struct InstantiateMsg {
    /// The config admins who can update the config and manage chains/depositors/executors
    pub admins: Vec<String>,
    /// The single token denom this adapter handles (i.e. the USDC token denom)
    pub denom: String,
    /// IBC transfer channel ID from Neutron to Noble
    pub noble_transfer_channel_id: String,
    /// Default IBC timeout in seconds for Noble transfers
    pub ibc_default_timeout_seconds: u64,
    /// Initial depositors to register during instantiation (can be empty array)
    pub initial_depositors: Vec<InitialDepositor>,
    /// Initial chain configurations with allowed destination addresses (can be empty array)
    pub initial_chains: Vec<InitialChainConfig>,
    /// Initial executors who can perform operations (can be empty array)
    pub initial_executors: Vec<InitialExecutor>,
}

/// Top-level execute message wrapper for CCTP adapter
#[cw_serde]
pub enum ExecuteMsg {
    /// Standard adapter interface messages (deposit, withdraw, manage depositors)
    StandardAction(AdapterInterfaceMsg),
    /// CCTP adapter-specific custom messages
    CustomAction(CctpAdapterMsg),
}

/// Data for updating the contract config.
#[cw_serde]
pub struct UpdateConfigData {
    pub denom: Option<String>,
    pub noble_transfer_channel_id: Option<String>,
    pub ibc_default_timeout_seconds: Option<u64>,
}

/// CCTP adapter-specific execute messages (placeholder for future custom actions)
#[cw_serde]
pub enum CctpAdapterMsg {
    /// Update contract configuration (admin only)
    UpdateConfig { update: UpdateConfigData },

    /// Add a new executor (config admin only)
    AddExecutor { executor_address: String },

    /// Remove an executor (config admin only)
    RemoveExecutor { executor_address: String },

    /// Add a new admin (admin only)
    AddAdmin { admin_address: String },

    /// Remove an admin (admin only)
    RemoveAdmin { admin_address: String },

    /// Register or update chain configuration (config admin only)
    RegisterChain { chain_config: ChainConfig },

    /// Update an existing registered chain configuration (config admin only)
    UpdateRegisteredChain { chain_config: ChainConfig },

    /// Unregister a chain (config admin only)
    UnregisterChain { chain_id: String },

    /// Add an allowed destination address for a chain (config admin only)
    AddAllowedDestinationAddress {
        chain_id: String,
        address: String,
        protocol: String,
    },

    /// Remove an allowed destination address for a chain (config admin only)
    RemoveAllowedDestinationAddress { chain_id: String, address: String },

    /// Transfer funds via CCTP to EVM chain
    TransferFunds {
        amount: Uint128,
        instructions: TransferFundsInstructions,
    },
}

/// Top-level query message wrapper for CCTP adapter
#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    /// Standard adapter interface queries
    #[returns(cosmwasm_std::Binary)]
    StandardQuery(AdapterInterfaceQueryMsg),
    /// CCTP adapter-specific custom queries
    #[returns(cosmwasm_std::Binary)]
    CustomQuery(CctpAdapterQueryMsg),
}

/// CCTP adapter-specific query messages
#[cw_serde]
#[derive(QueryResponses)]
pub enum CctpAdapterQueryMsg {
    /// Get chain configuration
    #[returns(ChainConfigResponse)]
    ChainConfig { chain_id: String },

    /// Get all registered chains
    #[returns(AllChainsResponse)]
    AllChains {},

    /// Get list of executors
    #[returns(ExecutorsResponse)]
    Executors {},

    /// Get list of all admins
    #[returns(AdminsResponse)]
    Admins {},

    /// Get depositor capabilities
    #[returns(DepositorCapabilitiesResponse)]
    DepositorCapabilities { depositor_address: String },

    /// Get allowed destination addresses for a chain
    #[returns(AllowedDestinationAddressesResponse)]
    AllowedDestinationAddresses {
        chain_id: String,
        start_after: Option<String>,
        limit: Option<u32>,
    },
}

// Response types for CCTP-specific queries

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
    pub addresses: Vec<DestinationAddress>,
}

#[cw_serde]
pub struct AdminsResponse {
    pub admins: Vec<String>,
}
