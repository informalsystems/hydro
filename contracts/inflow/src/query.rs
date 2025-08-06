use cosmwasm_schema::{cw_serde, QueryResponses};
// When compiling for wasm32 platform, compiler doesn't recognize that this type is used in one of the queries.
#[allow(unused_imports)]
use cosmwasm_std::Uint128;

use crate::state::Config;

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(ConfigResponse)]
    Config {},

    #[returns(Uint128)]
    TotalSharesIssued {},

    #[returns(TotalPoolValueResponse)]
    TotalPoolValue {},
}

#[cw_serde]
pub struct ConfigResponse {
    pub config: Config,
}

#[cw_serde]
pub struct TotalPoolValueResponse {
    pub total: Uint128,
}
