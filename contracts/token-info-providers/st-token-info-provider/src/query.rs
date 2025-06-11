use crate::state::{Config, InterchainQueryInfo};
use cosmwasm_schema::{cw_serde, QueryResponses};

use cosmwasm_std::Timestamp;
// When compiling for wasm32 platform, compiler doesn't recognize that this type is used in one of the queries.
#[allow(unused_imports)]
use interface::token_info_provider::DenomInfoResponse;

#[derive(QueryResponses)]
#[cw_serde]
pub enum QueryMsg {
    #[returns(ConfigResponse)]
    Config {},

    #[returns(InterchainQueryInfoResponse)]
    InterchainQueryInfo {},

    #[returns(DenomInfoResponse)]
    DenomInfo { round_id: u64 },
}

#[cw_serde]
pub struct ConfigResponse {
    pub config: Config,
}

#[cw_serde]
pub struct InterchainQueryInfoResponse {
    pub info: Option<InterchainQueryInfo>,
}

// TODO: The following two data structures should be replaced with the ones from the interface package once the
// LSM integration PR that introduces them gets merged.
#[cw_serde]
#[derive(QueryResponses)]
pub enum HydroQueryMsg {
    #[returns(HydroCurrentRoundResponse)]
    CurrentRound {},
}

#[cw_serde]
pub struct HydroCurrentRoundResponse {
    pub round_id: u64,
    pub round_end: Timestamp,
}
