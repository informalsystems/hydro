use cosmwasm_schema::{cw_serde, QueryResponses};

#[cw_serde]
pub struct InstantiateMsg {
    pub target_address: String,
    pub denom: String,
    pub inflow_contract: String,
    pub channel_id: String,
    pub ibc_timeout_seconds: u64,
}

#[cw_serde]
pub enum ExecuteMsg {
    ForwardToInflow {},
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(ConfigResponse)]
    Config {},
}

#[cw_serde]
pub struct ConfigResponse {
    pub target_address: String,
    pub denom: String,
    pub inflow_contract: String,
    pub channel_id: String,
    pub ibc_timeout_seconds: u64,
}
