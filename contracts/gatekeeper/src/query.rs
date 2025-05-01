use crate::state::{Config, StageData};
use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Uint128};

#[derive(QueryResponses)]
#[cw_serde]
pub enum QueryMsg {
    #[returns(ConfigResponse)]
    Config {},
    #[returns(CurrentStageResponse)]
    CurrentStage {},
    #[returns(AdminsResponse)]
    Admins {},
    #[returns(CurrentEpochUserLockedResponse)]
    CurrentEpochUserLocked { user_address: String },
}

#[cw_serde]
pub struct ConfigResponse {
    pub config: Config,
}

#[cw_serde]
pub struct CurrentStageResponse {
    pub stage: StageData,
}

#[cw_serde]
pub struct AdminsResponse {
    pub admins: Vec<Addr>,
}

#[cw_serde]
pub struct CurrentEpochUserLockedResponse {
    pub currently_locked: Uint128,
}
