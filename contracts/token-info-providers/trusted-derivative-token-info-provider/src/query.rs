use crate::state::Config;
use cosmwasm_schema::{cw_serde, QueryResponses};

#[allow(unused_imports)]
use interface::token_info_provider::DenomInfoResponse;

#[derive(QueryResponses)]
#[cw_serde]
pub enum QueryMsg {
    #[returns(ConfigResponse)]
    Config {},

    #[returns(WhitelistResponse)]
    Whitelist {},

    #[returns(DenomInfoResponse)]
    DenomInfo { round_id: u64 },
}

#[cw_serde]
pub struct ConfigResponse {
    pub config: Config,
}

#[cw_serde]
pub struct WhitelistResponse {
    pub addresses: Vec<String>,
}
