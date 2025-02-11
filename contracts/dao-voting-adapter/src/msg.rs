use cosmwasm_schema::cw_serde;

#[cw_serde]
pub struct InstantiateMsg {
    pub hydro_contract: String,
}

#[cw_serde]
pub enum ExecuteMsg {}
