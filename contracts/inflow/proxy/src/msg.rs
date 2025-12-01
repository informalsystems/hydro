use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::Coin;

use crate::state::Config;

#[cw_serde]
pub struct InstantiateMsg {
    pub admins: Vec<String>,
    // List of Control Center contract addresses used to discover and interact with Inflow vaults
    pub control_centers: Vec<String>,
}

#[cw_serde]
pub enum ExecuteMsg {
    ForwardToInflow {},
    WithdrawReceiptTokens { address: String, amount: Coin },
    WithdrawFunds { address: String, amount: Coin },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(ConfigResponse)]
    Config {},
}

#[cw_serde]
pub struct ConfigResponse {
    pub config: Config,
}

#[cw_serde]
pub enum ReplyPayload {
    DepositToInflow {
        vault_address: String,
        deposit: Coin,
    },
}
