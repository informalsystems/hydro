use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Coin, Uint128};

/// Standard execute messages that all protocol adapters must implement
#[cw_serde]
pub enum AdapterExecuteMsg {
    /// Deposit tokens into the protocol
    /// Only callable by whitelisted Inflow contracts
    /// The coin is sent in info.funds
    Deposit {},

    /// Withdraw tokens from the protocol
    /// Only callable by whitelisted Inflow contracts
    Withdraw {
        /// Coin (denom + amount) to withdraw
        coin: Coin,
    },

    /// Register new Inflow address (admin only)
    RegisterInflow {
        /// New Inflow contract address to register
        inflow_address: String,
    },

    /// Unregister Inflow address (admin only)
    UnregisterInflow {
        /// Inflow contract address to unregister.
        /// The account should not have any funds deployed.
        inflow_address: String,
    },

    /// Toggle Inflow address as enabled/disabled (admin only)
    ToggleInflowEnabled {
        /// Inflow contract address to enable or disable
        inflow_address: String,
        enabled: bool,
    },

    /// Update adapter configuration (admin only)
    UpdateConfig {
        /// New protocol contract address
        protocol_address: Option<String>,
        /// Updated list of supported denoms (the whole list needs to be provided)
        supported_denoms: Option<Vec<String>>,
    },
}

/// Standard query messages that all protocol adapters should support
#[cw_serde]
#[derive(QueryResponses)]
pub enum AdapterQueryMsg {
    /// Returns the maximum amount that can be deposited
    /// Accounts for protocol deposit caps and other limitations
    #[returns(AvailableAmountResponse)]
    AvailableForDeposit {
        inflow_address: String,
        denom: String,
    },

    /// Returns the amount available for immediate withdrawal for a specific denom
    #[returns(AvailableAmountResponse)]
    AvailableForWithdraw {
        inflow_address: String,
        denom: String,
    },

    /// Returns estimated blocks/time required for withdrawal
    /// Returns 0 for instant withdrawals (like Mars lending)
    #[returns(TimeEstimateResponse)]
    TimeToWithdraw { inflow_address: String, coin: Coin },

    /// Returns adapter configuration
    #[returns(AdapterConfigResponse)]
    Config {},

    /// Returns total amounts deposited across all Inflow contracts for all denoms
    #[returns(TotalDepositedResponse)]
    TotalDeposited {},

    /// Returns list of registered Inflow contracts with their enabled status
    /// Optionally filter by enabled status (Some(true), Some(false), or None for all)
    #[returns(RegisteredInflowsResponse)]
    RegisteredInflows { enabled: Option<bool> },

    /// Returns the total amount deposited by a specific Inflow contract for a specific denom
    #[returns(InflowDepositResponse)]
    InflowDeposit {
        inflow_address: String,
        denom: String,
    },

    /// Returns all deposits by a specific Inflow contract across all denoms
    #[returns(InflowDepositsResponse)]
    InflowDeposits { inflow_address: String },
}

// Response Types

#[cw_serde]
pub struct AvailableAmountResponse {
    pub amount: Uint128,
}

#[cw_serde]
pub struct InflowDepositResponse {
    pub amount: Uint128,
}

#[cw_serde]
pub struct InflowDepositsResponse {
    pub deposits: Vec<Coin>,
}

#[cw_serde]
pub struct TotalDepositedResponse {
    pub deposits: Vec<Coin>,
}

#[cw_serde]
pub struct TimeEstimateResponse {
    pub blocks: u64,
    pub seconds: u64,
}

#[cw_serde]
pub struct AdapterConfigResponse {
    pub protocol_address: String,
    pub supported_denoms: Vec<String>,
}

#[cw_serde]
pub struct RegisteredInflowInfo {
    pub inflow_address: String,
    pub enabled: bool,
}

#[cw_serde]
pub struct RegisteredInflowsResponse {
    pub inflows: Vec<RegisteredInflowInfo>,
}
