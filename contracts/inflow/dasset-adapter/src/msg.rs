use cosmwasm_schema::{cw_serde, QueryResponses};
pub use interface::inflow_adapter::{AdapterInterfaceMsg, AdapterInterfaceQueryMsg};

#[cw_serde]
pub struct InstantiateMsg {
    pub admins: Vec<String>,
    pub executors: Vec<String>,

    pub drop_staking_core: String,
    pub drop_voucher: String,
    pub drop_withdrawal_manager: String,

    pub vault_contract: String,

    pub liquid_asset_denom: String,
    pub base_asset_denom: String,
}

#[cw_serde]
pub enum ExecuteMsg {
    /// Adapter interface entrypoint
    StandardAction(AdapterInterfaceMsg),

    /// DAsset-specific logic
    CustomAction(DAssetAdapterMsg),
}

#[cw_serde]
pub enum DAssetAdapterMsg {
    /// executor-only
    Unbond {},

    /// executor-only
    Withdraw { token_id: String },

    /// admin-only
    UpdateConfig {
        drop_staking_core: Option<String>,
        drop_voucher: Option<String>,
        drop_withdrawal_manager: Option<String>,
        vault_contract: Option<String>,
    },

    /// admin-only
    UpdateExecutors { executors: Vec<String> },
}

#[cw_serde]
pub struct ConfigResponse {
    pub admins: Vec<String>,
    pub executors: Vec<String>,
    pub drop_staking_core: String,
    pub drop_voucher: String,
    pub drop_withdrawal_manager: String,
    pub vault_contract: String,
    pub liquid_asset_denom: String,
    pub base_asset_denom: String,
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(cosmwasm_std::Binary)]
    StandardQuery(AdapterInterfaceQueryMsg),

    #[returns(ConfigResponse)]
    Config {},
}
