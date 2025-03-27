use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::Uint128;

#[cw_serde]
pub struct InstantiateMsg {}

#[cw_serde]
pub struct CreatePositionMsg {
    pub pool_id: u64,
    pub lower_tick: i64,
    pub upper_tick: i64,
    pub token0_amount: Uint128,
    pub token1_amount: Uint128,
}

#[cw_serde]
pub enum ExecuteMsg {
    CreatePosition(CreatePositionMsg),
    WithdrawPosition {},
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(StateResponse)]
    GetState {},
}

#[cw_serde]
pub struct StateResponse {
    pub owner: String,
    pub pool_id: u64,
    pub position_id: Option<u64>,
    pub token0_denom: String,
    pub token1_denom: String,
    pub initial_token0_amount: Uint128,
}
