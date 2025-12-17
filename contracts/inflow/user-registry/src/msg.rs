use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::Addr;

#[cw_serde]
pub struct InstantiateMsg {
    pub admins: Vec<String>,
}

#[cw_serde]
pub enum ExecuteMsg {
    RegisterUser {
        user_id: String,
        proxy_address: String,
    },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(ProxyAddressResponse)]
    ProxyAddress { user_id: String },
}

#[cw_serde]
pub struct ProxyAddressResponse {
    pub address: Addr,
}
