use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Coin, Uint128};

/// Standard execute messages that all protocol adapters must implement
#[cw_serde]
pub enum AdapterExecuteMsg {
    /// Deposit tokens into the protocol
    /// Only callable by whitelisted depositors
    /// The coin is sent in info.funds
    Deposit {},

    /// Withdraw tokens from the protocol
    /// Only callable by whitelisted depositors
    Withdraw {
        /// Coin (denom + amount) to withdraw
        coin: Coin,
    },

    /// Register new depositor address (admin only)
    RegisterDepositor {
        /// New depositor contract address to register
        depositor_address: String,
    },

    /// Unregister depositor address (admin only)
    UnregisterDepositor {
        /// Depositor contract address to unregister.
        /// The account should not have any funds deployed.
        depositor_address: String,
    },

    /// Toggle depositor address as enabled/disabled (admin only)
    ToggleDepositorEnabled {
        /// Depositor contract address to enable or disable
        depositor_address: String,
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
        depositor_address: String,
        denom: String,
    },

    /// Returns the amount available for immediate withdrawal for a specific denom
    #[returns(AvailableAmountResponse)]
    AvailableForWithdraw {
        depositor_address: String,
        denom: String,
    },

    /// Returns estimated blocks/time required for withdrawal
    /// Returns 0 for instant withdrawals (like Mars lending)
    #[returns(TimeEstimateResponse)]
    TimeToWithdraw {
        depositor_address: String,
        coin: Coin,
    },

    /// Returns adapter configuration
    #[returns(AdapterConfigResponse)]
    Config {},

    /// Returns total positions across all depositor contracts for all denoms
    #[returns(TotalDepositedResponse)]
    TotalDeposited {},

    /// Returns list of registered depositor contracts with their enabled status
    /// Optionally filter by enabled status (Some(true), Some(false), or None for all)
    #[returns(RegisteredDepositorsResponse)]
    RegisteredDepositors { enabled: Option<bool> },

    /// Returns the current position for a specific depositor contract and denom
    #[returns(DepositorPositionResponse)]
    DepositorPosition {
        depositor_address: String,
        denom: String,
    },

    /// Returns all positions for a specific depositor contract across all denoms
    #[returns(DepositorPositionsResponse)]
    DepositorPositions { depositor_address: String },
}

// Response Types

#[cw_serde]
pub struct AvailableAmountResponse {
    pub amount: Uint128,
}

#[cw_serde]
pub struct DepositorPositionResponse {
    pub amount: Uint128,
}

#[cw_serde]
pub struct DepositorPositionsResponse {
    pub positions: Vec<Coin>,
}

#[cw_serde]
pub struct TotalDepositedResponse {
    pub positions: Vec<Coin>,
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
pub struct RegisteredDepositorInfo {
    pub depositor_address: String,
    pub enabled: bool,
}

#[cw_serde]
pub struct RegisteredDepositorsResponse {
    pub depositors: Vec<RegisteredDepositorInfo>,
}
