use crate::state::Config;
use cosmwasm_schema::{cw_serde, QueryResponses};

// When compiling for wasm32 platform, compiler doesn't recognize that this type is used in one of the queries.
#[allow(unused_imports)]
use interface::token_info_provider::DenomInfoResponse;

#[derive(QueryResponses)]
#[cw_serde]
pub enum QueryMsg {
    #[returns(ConfigResponse)]
    Config {},

    #[returns(DenomInfoResponse)]
    DenomInfo { round_id: u64 },
}

#[cw_serde]
pub struct ConfigResponse {
    pub config: Config,
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum DropQueryMsg {
    // https://github.com/hadronlabs-org/drop-contracts/blob/442801e54a51d7b21dbff94b3256a1ec34df08c5/packages/base/src/msg/core.rs#L64
    #[returns(cosmwasm_std::Decimal)]
    ExchangeRate {},
}
