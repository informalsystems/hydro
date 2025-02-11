use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::Addr;
use dao_dao_macros::voting_module_query;

use crate::state::Config;

#[voting_module_query]
#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(ConfigResponse)]
    Config {},
}

#[cw_serde]
pub struct ConfigResponse {
    pub config: Config,
}

#[cw_serde]
pub struct DaoResponse(Addr);
