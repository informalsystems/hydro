use std::collections::HashMap;

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Decimal, Timestamp, Uint128};

#[cw_serde]
pub struct ConstantsV1_1_0 {
    pub round_length: u64,
    pub lock_epoch_length: u64,
    pub first_round_start: Timestamp,
    pub max_locked_tokens: u128,
    pub max_validator_shares_participating: u64,
    pub hub_connection_id: String,
    pub hub_transfer_channel_id: String,
    pub icq_update_period: u64,
    pub paused: bool,
    pub is_in_pilot_mode: bool,
}

#[cw_serde]
pub struct ProposalV1_1_0 {
    pub round_id: u64,
    pub tranche_id: u64,
    pub proposal_id: u64,
    pub title: String,
    pub description: String,
    pub power: Uint128,
    pub percentage: Uint128,
}

#[cw_serde]
pub struct VoteV1_1_0 {
    pub prop_id: u64,
    pub time_weighted_shares: HashMap<String, Decimal>,
}
