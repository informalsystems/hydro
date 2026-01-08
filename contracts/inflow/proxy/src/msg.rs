use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::Coin;

use crate::state::{Config, State};

#[cw_serde]
pub struct InstantiateMsg {
    pub admins: Vec<String>,
    // List of Control Center contract addresses used to discover and interact with Inflow vaults
    pub control_centers: Vec<String>,
}

#[cw_serde]
pub enum ExecuteMsg {
    ForwardToInflow {},
    WithdrawReceiptTokens { address: String, coin: Coin },
    WithdrawFunds { address: String, coin: Coin },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(ConfigResponse)]
    Config {},
    #[returns(StateResponse)]
    State {},
}

#[cw_serde]
pub struct ConfigResponse {
    pub config: Config,
}

#[cw_serde]
pub struct StateResponse {
    pub state: State,
}

#[cw_serde]
pub enum ReplyPayload {
    DepositToInflow {
        vault_address: String,
        deposit: Coin,
    },
}
