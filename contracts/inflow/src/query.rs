use cosmwasm_schema::{cw_serde, QueryResponses};

use crate::state::Config;

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
