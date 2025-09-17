use crate::state::Config;
use cosmwasm_schema::{cw_serde, QueryResponses};

use cosmwasm_std::Addr;
// When compiling for wasm32 platform, compiler doesn't recognize that this type is used in one of the queries.
#[allow(unused_imports)]
use interface::token_info_provider::ValidatorsInfoResponse;

#[derive(QueryResponses)]
#[cw_serde]
pub enum QueryMsg {
    #[returns(ConfigResponse)]
    Config {},

    #[returns(RegisteredValidatorQueriesResponse)]
    RegisteredValidatorQueries {},

    #[returns(AdminsResponse)]
    Admins {},

    #[serde(rename = "icq_managers")]
    #[returns(ICQManagersResponse)]
    ICQManagers {},

    // Token Information Provider Query
    #[returns(ValidatorsInfoResponse)]
    ValidatorsInfo { round_id: u64 },
}

#[cw_serde]
pub struct ConfigResponse {
    pub config: Config,
}

// A vector containing tuples, where each tuple contains a validator address
// and the id of the interchain query associated with that validator.
#[cw_serde]
pub struct RegisteredValidatorQueriesResponse {
    pub query_ids: Vec<(String, u64)>,
}

#[cw_serde]
pub struct AdminsResponse {
    pub admins: Vec<Addr>,
}

#[cw_serde]
pub struct ICQManagersResponse {
    pub managers: Vec<Addr>,
}
