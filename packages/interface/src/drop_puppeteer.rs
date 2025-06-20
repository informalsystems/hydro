use cosmwasm_schema::{cw_serde, serde::Deserialize};
use cosmwasm_std::{Addr, Decimal256, Timestamp};

#[cw_serde]
pub struct DelegationsResponse {
    pub delegations: Delegations,
    pub remote_height: u64,
    pub local_height: u64,
    pub timestamp: Timestamp,
}

#[cw_serde]
pub struct Delegations {
    pub delegations: Vec<DropDelegation>,
}

#[cw_serde]
pub struct DropDelegation {
    pub delegator: Addr,
    /// A validator address (e.g. cosmosvaloper1...)
    pub validator: String,
    /// How much we have locked in the delegation
    pub amount: cosmwasm_std::Coin,
    /// How many shares the delegator has in the validator
    pub share_ratio: Decimal256,
}

#[cw_serde]
pub enum QueryExtMsg {
    Delegations {},
}

#[cw_serde]
pub enum PuppeteerQueryMsg {
    Extension { msg: QueryExtMsg },
    // You can add more query types here if needed
}
