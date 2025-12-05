use cosmwasm_schema::cw_serde;
use cosmwasm_std::{from_json, to_json_binary, Binary, Coin, StdResult, Uint128};
use serde::{Deserialize, Serialize};

/// Standard execute messages that all protocol adapters must implement
#[cw_serde]
pub enum AdapterInterfaceMsg {
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
        /// New depositor address to register
        depositor_address: String,
        /// Optional adapter-specific metadata (e.g., capabilities)
        /// Serialized as Binary for flexibility
        metadata: Option<Binary>,
    },

    /// Unregister depositor address (admin only)
    UnregisterDepositor {
        /// Depositor address to unregister.
        /// The account should not have any funds deployed.
        depositor_address: String,
    },

    /// Toggle depositor address as enabled/disabled (admin only)
    SetDepositorEnabled {
        /// Depositor address to enable or disable
        depositor_address: String,
        enabled: bool,
    },
}

/// Standard query messages that all protocol adapters should support
/// Note: QueryResponses derive is handled by each adapter's QueryMsg wrapper
#[cw_serde]
pub enum AdapterInterfaceQueryMsg {
    /// Returns the maximum amount that can be deposited
    /// Accounts for protocol deposit caps and other limitations
    AvailableForDeposit {
        depositor_address: String,
        denom: String,
    },

    /// Returns the amount available for immediate withdrawal for a specific denom
    AvailableForWithdraw {
        depositor_address: String,
        denom: String,
    },

    /// Returns estimated blocks/time required for withdrawal
    /// Returns 0 for instant withdrawals (like Mars lending)
    TimeToWithdraw {
        depositor_address: String,
        coin: Coin,
    },

    /// Returns adapter configuration (adapter-specific response type)
    Config {},

    /// Returns total positions across all depositors for all denoms
    AllPositions {},

    /// Returns list of registered depositors with their enabled status
    /// Optionally filter by enabled status (Some(true), Some(false), or None for all)
    RegisteredDepositors { enabled: Option<bool> },

    /// Returns the current position for a specific depositor and denom
    /// When working with Inflow vaults, the position should not be included in the "deployed" amount
    /// as it will be included, together with the balance, in the total pool value calculation
    DepositorPosition {
        depositor_address: String,
        denom: String,
    },

    /// Returns all positions for a specific depositor across all denoms
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
pub struct AllPositionsResponse {
    pub positions: Vec<Coin>,
}

#[cw_serde]
pub struct TimeEstimateResponse {
    pub blocks: u64,
    pub seconds: u64,
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

// ========== SERIALIZATION HELPERS FOR CALLING ADAPTERS ==========

/// Helper to serialize AdapterInterfaceMsg for calling adapters from external contracts
/// Wraps the message in the structure adapters expect: {"standard_action": {...}}
/// Used with WasmMsg::Execute which requires Binary
pub fn serialize_adapter_interface_msg(msg: &AdapterInterfaceMsg) -> StdResult<Binary> {
    #[derive(Serialize)]
    struct Wrapper<'a> {
        standard_action: &'a AdapterInterfaceMsg,
    }

    to_json_binary(&Wrapper {
        standard_action: msg,
    })
}

/// Wrapper for querying adapters from external contracts
/// Used with query_wasm_smart which serializes internally
/// Example: deps.querier.query_wasm_smart(addr, &AdapterInterfaceQuery { standard_query: &msg })
#[derive(Serialize)]
pub struct AdapterInterfaceQuery<'a> {
    pub standard_query: &'a AdapterInterfaceQueryMsg,
}

// ========== DESERIALIZATION HELPERS FOR TESTS ==========

/// Helper to deserialize AdapterInterfaceMsg from Binary (for tests)
/// Unwraps the {"standard_action": {...}} structure
pub fn deserialize_adapter_interface_msg(binary: &Binary) -> StdResult<AdapterInterfaceMsg> {
    #[derive(Deserialize)]
    struct Wrapper {
        standard_action: AdapterInterfaceMsg,
    }

    let wrapper: Wrapper = from_json(binary)?;
    Ok(wrapper.standard_action)
}

/// Helper to deserialize AdapterInterfaceQueryMsg from Binary (for tests)
/// Unwraps the {"standard_query": {...}} structure
pub fn deserialize_adapter_interface_query_msg(
    binary: &Binary,
) -> StdResult<AdapterInterfaceQueryMsg> {
    #[derive(Deserialize)]
    struct Wrapper {
        standard_query: AdapterInterfaceQueryMsg,
    }

    let wrapper: Wrapper = from_json(binary)?;
    Ok(wrapper.standard_query)
}
