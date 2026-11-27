use crate::state::Config;
use cosmwasm_schema::{cw_serde, QueryResponses};

#[allow(unused_imports)]
use cosmwasm_std::Decimal;
use cosmwasm_std::{Decimal256, Uint128, Uint256};
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

    #[returns(Decimal)]
    RatioToBaseToken { denom: String },
}

#[cw_serde]
pub struct ConfigResponse {
    pub config: Config,
}

// Query and response types of the hATOM contract (hatom-contracts, `contracts/hatom/schema/hatom.json`).
#[cw_serde]
#[derive(QueryResponses)]
pub enum HatomQueryMsg {
    #[returns(HatomRateResponse)]
    Rate {},
}

#[cw_serde]
pub struct HatomRateResponse {
    /// Bond denom per hATOM.
    pub exchange_rate: Decimal256,
    pub holders_value: Uint256,
    pub supply: Uint128,
}
