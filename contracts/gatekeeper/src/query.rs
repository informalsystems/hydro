use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Uint128};

use crate::state::Config;

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(ConfigResponse)]
    Config {},
    #[returns(AdminsResponse)]
    GetAdmins {},
}

#[cw_serde]
pub struct ConfigResponse {
    pub config: Config,
}
#[cw_serde]
pub struct AdminsResponse {
    pub admins: Vec<String>,
}

#[cw_serde]
pub struct CanLockResponse {
    pub can_lock: bool,
    pub available_amount: Option<Uint128>,
}
