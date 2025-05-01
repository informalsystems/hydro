use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::Decimal;

// This Query enum will be expanded with additional variants as new
// token info provider implementations emerge.
#[derive(QueryResponses)]
#[cw_serde]
pub enum TokenInfoProviderQueryMsg {
    // Implemented by the standard derivative token info providers (e.g. for stATOM, dATOM, ...)
    #[returns(DenomInfoResponse)]
    DenomInfo { round_id: u64 },
}

#[cw_serde]
pub struct DenomInfoResponse {
    pub denom: String,
    pub token_group_id: String,
    pub ratio: Decimal,
}
