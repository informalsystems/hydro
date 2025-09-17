use crate::state::{Config, InterchainQueryInfo};
use cosmwasm_schema::{cw_serde, QueryResponses};

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
