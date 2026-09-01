use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::Uint128;

use crate::state::{Config, DepositorCapabilities};

pub use interface::inflow_adapter::{
    AdapterInterfaceMsg, AdapterInterfaceQueryMsg, AdminsResponse, AllPositionsResponse,
    AvailableAmountResponse, DepositorPositionResponse, DepositorPositionsResponse,
    RegisteredDepositorInfo, RegisteredDepositorsResponse, TimeEstimateResponse,
};

/// Message for instantiating the IBC Eureka adapter contract
#[cw_serde]
pub struct InstantiateMsg {
    /// The config admins who can update the config and manage denoms/depositors/executors
    pub admins: Vec<String>,
    /// Address of the Skip swap entry-point contract on Cosmos Hub
    pub skip_swap_entry_point_contract: String,
    /// IBC Eureka source channel ID used for bridging to the destination chain
    pub source_channel: String,
    /// Address on Cosmos Hub that receives the IBC Eureka relayer fee
    pub eureka_fee_receiver: String,
    /// Timeout, in seconds, to be used when building IBC Eureka transfer messages
    pub ibc_transfer_timeout_seconds: u64,
    /// Timeout, in seconds, for the Eureka relayer fees to be received (converted to nanoseconds when building the message)
    pub eureka_fee_timeout_seconds: u64,
    /// Initial depositors to register during instantiation (can be empty array)
    pub initial_depositors: Vec<InitialDepositor>,
    /// Initial allowed token denoms for this adapter instance (can be empty array)
    pub initial_denoms: Vec<String>,
    /// Initial allowed EVM destination addresses (can be empty array)
    pub initial_allowed_destination_addresses: Vec<String>,
    /// Initial executors who can perform operations (can be empty array)
    pub initial_executors: Vec<InitialExecutor>,
}

/// Initial depositor configuration for instantiation
#[cw_serde]
pub struct InitialDepositor {
    /// Depositor address to register
    pub address: String,
    /// Optional capabilities for this depositor
    /// If not provided, defaults to { can_withdraw: true }
    pub capabilities: Option<DepositorCapabilities>,
}

/// Initial executor configuration for instantiation
#[cw_serde]
pub struct InitialExecutor {
    /// Executor address to register
    pub address: String,
}

/// Top-level execute message wrapper for the IBC Eureka adapter
#[cw_serde]
pub enum ExecuteMsg {
    /// Standard adapter interface messages (deposit, withdraw, manage depositors)
    StandardAction(AdapterInterfaceMsg),
    /// IBC Eureka adapter-specific custom messages
    CustomAction(IbcEurekaAdapterMsg),
}

/// Data for updating the contract config.
#[cw_serde]
pub struct UpdateConfigData {
    pub skip_swap_entry_point_contract: Option<String>,
    pub source_channel: Option<String>,
    pub eureka_fee_receiver: Option<String>,
    pub encoding: Option<String>,
    pub ibc_transfer_timeout_seconds: Option<u64>,
    pub eureka_fee_timeout_seconds: Option<u64>,
}

/// IBC Eureka adapter-specific execute messages
#[cw_serde]
pub enum IbcEurekaAdapterMsg {
    /// Update contract configuration (admin only)
    UpdateConfig { update: UpdateConfigData },

    /// Add a new executor (config admin only)
    AddExecutor { executor_address: String },

    /// Remove an executor (config admin only)
    RemoveExecutor { executor_address: String },

    /// Add an allowed token denom for this adapter instance (config admin only)
    AddAllowedDenom { denom: String },

    /// Remove an allowed token denom for this adapter instance (config admin only)
    RemoveAllowedDenom { denom: String },

    /// Add an allowed EVM destination address (config admin only)
    AddAllowedDestinationAddress { address: String },

    /// Remove an allowed EVM destination address (config admin only)
    RemoveAllowedDestinationAddress { address: String },

    /// Transfer funds via IBC Eureka to the destination EVM chain.
    /// The executor must attach the Eureka relayer fee via `info.funds`, in the
    /// same denom as `denom` - it is not drawn from the adapter's own balance.
    TransferFunds {
        /// Token denom to bridge; must be a registered allowed denom
        denom: String,
        /// Amount to bridge, excluding the Eureka relayer fee
        amount: Uint128,
        /// Destination EVM address; must be in the allowed destination address list
        recipient: String,
    },
}

/// Top-level query message wrapper for the IBC Eureka adapter
#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    /// Standard adapter interface queries
    #[returns(cosmwasm_std::Binary)]
    StandardQuery(AdapterInterfaceQueryMsg),
    /// IBC Eureka adapter-specific custom queries
    #[returns(cosmwasm_std::Binary)]
    CustomQuery(IbcEurekaAdapterQueryMsg),
}

/// IBC Eureka adapter-specific query messages
#[cw_serde]
#[derive(QueryResponses)]
pub enum IbcEurekaAdapterQueryMsg {
    /// Get all allowed token denoms
    #[returns(AllowedDenomsResponse)]
    AllowedDenoms {},

    /// Get list of executors
    #[returns(ExecutorsResponse)]
    Executors {},

    /// Get depositor capabilities
    #[returns(DepositorCapabilitiesResponse)]
    DepositorCapabilities { depositor_address: String },

    /// Get allowed EVM destination addresses
    #[returns(AllowedDestinationAddressesResponse)]
    AllowedDestinationAddresses {
        start_after: Option<String>,
        limit: Option<u32>,
    },
}

// Response types for IBC Eureka-specific queries

#[cw_serde]
pub struct ConfigResponse {
    pub config: Config,
}

#[cw_serde]
pub struct AllowedDenomsResponse {
    pub denoms: Vec<String>,
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
