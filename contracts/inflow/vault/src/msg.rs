use cosmwasm_schema::cw_serde;
use cosmwasm_std::Uint128;
use serde::{Deserialize, Serialize};

#[cw_serde]
pub struct InstantiateMsg {
    /// The denom of the token that can be deposited into the vault.
    pub deposit_denom: String,
    /// Inflow vault shares token subdenom. Used to derive the full token denom.
    /// E.g. if the subdenom is "hydro_inflow_uatom" then the full denom will be
    /// "factory/{inflow_contract_address}/hydro_inflow_uatom"
    pub subdenom: String,
    /// Additional metadata to be set for the newly created vault shares token.
    pub token_metadata: DenomMetadata,
    /// Address of the Inflow Control Center contract that will manage this sub-vault.
    pub control_center_contract: String,
    /// Address of the token info provider contract used to obtain the ratio of the
    /// deposit token to the base token. If None, then the deposit token is the base token.
    pub token_info_provider_contract: Option<String>,
    /// List of addresses allowed to execute permissioned actions.
    pub whitelist: Vec<String>,
    /// Maximum number of pending withdrawals per single user.
    pub max_withdrawals_per_user: u64,
}

#[cw_serde]
pub struct DenomMetadata {
    /// Number of decimals used for denom unit other than the base one.
    /// E.g. "uatom" as a base denom unit has 0 decimals, and "atom" would have 6.
    pub exponent: u32,
    /// Lowercase moniker to be displayed in clients, example: "atom"
    /// Also used as a denom for the non-base denom unit.
    pub display: String,
    /// Descriptive token name, example: "Cosmos Hub Atom"
    pub name: String,
    /// Even longer description, example: "The native staking token of the Cosmos Hub"
    pub description: String,
    /// Symbol is the token symbol usually shown on exchanges (eg: ATOM). This can be the same as the display.
    pub symbol: String,
    /// URI to a document (on or off-chain) that contains additional information.
    pub uri: Option<String>,
    /// URIHash is a sha256 hash of a document pointed by URI. It's used to verify that the document didn't change.
    pub uri_hash: Option<String>,
}

#[cw_serde]
pub enum ExecuteMsg {
    Deposit {
        on_behalf_of: Option<String>,
    },
    Withdraw {
        on_behalf_of: Option<String>,
    },
    CancelWithdrawal {
        withdrawal_ids: Vec<u64>,
    },
    FulfillPendingWithdrawals {
        limit: u64,
    },
    ClaimUnbondedWithdrawals {
        withdrawal_ids: Vec<u64>,
    },
    WithdrawForDeployment {
        amount: Uint128,
    },
    SetTokenInfoProviderContract {
        address: Option<String>,
    },
    AddToWhitelist {
        address: String,
    },
    RemoveFromWhitelist {
        address: String,
    },
    UpdateConfig {
        config: UpdateConfigData,
    },

    /// Register a new adapter for protocol integrations
    RegisterAdapter {
        name: String,
        address: String,
        description: Option<String>,
        /// Whether to include this adapter in automated allocation from the start
        auto_allocation: bool,
    },
    /// Unregister an existing adapter
    UnregisterAdapter {
        name: String,
    },
    /// Toggle adapter's automated allocation status
    /// Manual admin operations (DepositToAdapter, WithdrawFromAdapter) still work regardless of this flag
    ToggleAdapterAutoAllocation {
        name: String,
    },
    /// Withdraw funds from an adapter to the vault contract (whitelisted only)
    /// Funds stay in contract until withdraw_for_deployment is called
    WithdrawFromAdapter {
        adapter_name: String,
        amount: Uint128,
    },
    /// Deposit funds from vault contract balance to an adapter (whitelisted only)
    /// Used for manual rebalancing between adapters
    DepositToAdapter {
        adapter_name: String,
        amount: Uint128,
    },
}

#[cw_serde]
pub struct UpdateConfigData {
    pub max_withdrawals_per_user: Option<u64>,
}

#[derive(Serialize, Deserialize)]
pub enum ReplyPayload {
    CreateDenom {
        subdenom: String,
        metadata: DenomMetadata,
    },
}
