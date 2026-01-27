use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::Uint128;
pub use interface::inflow_adapter::{AdapterInterfaceMsg, AdapterInterfaceQueryMsg};

/// Token registration info used during instantiation
#[cw_serde]
pub struct TokenRegistration {
    /// Human-readable key, e.g., "datom", "dntrn"
    pub symbol: String,
    /// Full denom, e.g., "factory/.../dATOM"
    pub denom: String,
    pub drop_staking_core: String,
    pub drop_voucher: String,
    pub drop_withdrawal_manager: String,
    /// Output denom, e.g., "ibc/.../uatom"
    pub base_asset_denom: String,
}

#[cw_serde]
pub struct InstantiateMsg {
    pub initial_admins: Vec<String>,
    pub initial_executors: Vec<String>,
    /// Initial depositors (e.g., vault addresses)
    pub initial_depositors: Vec<String>,
    /// Optional initial tokens to register
    #[serde(default)]
    pub initial_tokens: Vec<TokenRegistration>,
}

#[cw_serde]
pub enum ExecuteMsg {
    /// Adapter interface entrypoint (Deposit, Withdraw, depositor management)
    StandardAction(AdapterInterfaceMsg),

    /// DAsset-specific logic
    CustomAction(DAssetAdapterMsg),
}

#[cw_serde]
pub enum DAssetAdapterMsg {
    /// Executor-only: Initiate unbonding of dAsset tokens
    UnbondInDrop {
        /// Token symbol (e.g., "datom") to look up Drop contracts
        symbol: String,
        /// Amount to unbond. None = unbond all available balance
        amount: Option<Uint128>,
    },

    /// Executor-only: Withdraw base asset from Drop Protocol using NFT voucher
    WithdrawFromDrop {
        /// Token symbol (e.g., "datom") to look up Drop contracts
        symbol: String,
        /// NFT voucher token ID from unbonding
        token_id: String,
    },

    /// Admin-only: Register a new dAsset token with its Drop Protocol contracts
    RegisterToken {
        /// Human-readable key, e.g., "datom"
        symbol: String,
        /// Full denom, e.g., "factory/.../dATOM"
        denom: String,
        drop_staking_core: String,
        drop_voucher: String,
        drop_withdrawal_manager: String,
        /// Output denom, e.g., "ibc/.../uatom"
        base_asset_denom: String,
    },

    /// Admin-only: Unregister a dAsset token
    UnregisterToken { symbol: String },

    /// Admin-only: Enable or disable a registered token
    SetTokenEnabled { symbol: String, enabled: bool },

    /// Admin-only: Add a single executor
    AddExecutor { executor_address: String },

    /// Admin-only: Remove a single executor
    RemoveExecutor { executor_address: String },
}

#[cw_serde]
pub struct ConfigResponse {
    pub admins: Vec<String>,
}

#[cw_serde]
pub struct ExecutorsResponse {
    pub executors: Vec<String>,
}

#[cw_serde]
pub struct TokenConfigResponse {
    pub symbol: String,
    pub enabled: bool,
    pub denom: String,
    pub drop_staking_core: String,
    pub drop_voucher: String,
    pub drop_withdrawal_manager: String,
    pub base_asset_denom: String,
}

#[cw_serde]
pub struct TokensResponse {
    pub tokens: Vec<TokenConfigResponse>,
}

/// Top-level query message wrapper for dAsset adapter
#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    /// Standard adapter interface queries
    #[returns(cosmwasm_std::Binary)]
    StandardQuery(AdapterInterfaceQueryMsg),

    /// DAsset adapter-specific custom queries
    #[returns(cosmwasm_std::Binary)]
    CustomQuery(DAssetAdapterQueryMsg),
}

/// DAsset adapter-specific query messages
#[cw_serde]
#[derive(QueryResponses)]
pub enum DAssetAdapterQueryMsg {
    /// Query a specific token's configuration by symbol
    #[returns(TokenConfigResponse)]
    TokenConfig { symbol: String },

    /// Query all registered tokens
    #[returns(TokensResponse)]
    AllTokens {},

    /// Query all executors
    #[returns(ExecutorsResponse)]
    Executors {},
}
