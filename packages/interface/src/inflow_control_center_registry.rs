use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::Addr;

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(ConfigResponse)]
    Config {},
    #[returns(ControlCentersResponse)]
    ControlCenters {},
}

#[cw_serde]
pub struct ConfigResponse {
    pub admin: Addr,
    pub pending_admin: Option<Addr>,
}

#[cw_serde]
pub struct ControlCentersResponse {
    pub control_centers: Vec<Addr>,
}
