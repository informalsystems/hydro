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

    #[returns(AdminsResponse)]
    Admins {},

    // Token Information Provider Query
    #[returns(ValidatorsInfoResponse)]
    ValidatorsInfo { round_id: u64 },
}

#[cw_serde]
pub struct ConfigResponse {
    pub config: Config,
}

#[cw_serde]
pub struct AdminsResponse {
    pub admins: Vec<Addr>,
}
