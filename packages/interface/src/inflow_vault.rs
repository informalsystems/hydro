use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::Uint128;

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(PoolInfoResponse)]
    PoolInfo {},
}

#[cw_serde]
pub struct PoolInfoResponse {
    pub shares_issued: Uint128,
    pub balance_base_tokens: Uint128,
    pub adapter_deposits_base_tokens: Uint128,
    pub withdrawal_queue_base_tokens: Uint128,
}
