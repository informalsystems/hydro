use std::collections::HashMap;

use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::Decimal;

use crate::lsm::ValidatorInfo;

// This Query enum will be expanded with additional variants as new
// token info provider implementations emerge.
#[derive(QueryResponses)]
#[cw_serde]
pub enum TokenInfoProviderQueryMsg {
    // Implemented by the standard derivative token info providers (e.g. for stATOM, dATOM, ...)
    #[returns(DenomInfoResponse)]
    DenomInfo { round_id: u64 },

    // Implemented by the LSM token info provider
    #[returns(ValidatorsInfoResponse)]
    ValidatorsInfo { round_id: u64 },

    // Implemented by the token info providers that are compatible with Inflow smart contracts.
    #[returns(Decimal)]
    RatioToBaseToken { denom: String },
}

#[cw_serde]
pub struct DenomInfoResponse {
    pub denom: String,
    pub token_group_id: String,
    pub ratio: Decimal,
}

#[cw_serde]
pub struct ValidatorsInfoResponse {
    pub round_id: u64,
    pub validators: HashMap<String, ValidatorInfo>,
}
