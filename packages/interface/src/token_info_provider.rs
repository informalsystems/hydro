use std::collections::HashMap;

use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Decimal, Uint128};

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

#[cw_serde]
pub struct ValidatorsInfoResponse {
    pub round_id: u64,
    pub validators: HashMap<String, ValidatorInfo>,
}

// We will switch to this struct everywhere once the LSM decoupling PR is merged.
#[cw_serde]
#[derive(Default)]
pub struct ValidatorInfo {
    pub address: String,
    pub delegated_tokens: Uint128,
    pub power_ratio: Decimal,
}
