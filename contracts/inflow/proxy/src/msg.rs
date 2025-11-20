use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::Uint128;

use crate::state::State;

#[cw_serde]
pub struct InstantiateMsg {
    pub admins: Vec<String>,
}

#[cw_serde]
pub enum ExecuteMsg {
    ForwardToInflow {},
    WithdrawReceiptTokens { address: String, amount: Uint128 },
    WithdrawFunds { address: String, amount: Uint128 },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(StateResponse)]
    State {},
}

#[cw_serde]
pub struct StateResponse {
    pub state: State,
}
