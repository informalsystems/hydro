use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Timestamp, Uint128};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::migration::v1_1_0::{ConstantsV1_1_0, ProposalV1_1_0};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct MigrateMsgV2_0_1 {
    pub max_deployment_duration: u64,
}

#[cw_serde]
pub struct ConstantsV2_0_1 {
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
    pub max_deployment_duration: u64,
}

impl ConstantsV2_0_1 {
    pub fn from(old_constants: ConstantsV1_1_0, max_deployment_duration: u64) -> Self {
        Self {
            round_length: old_constants.round_length,
            lock_epoch_length: old_constants.lock_epoch_length,
            first_round_start: old_constants.first_round_start,
            max_locked_tokens: old_constants.max_locked_tokens,
            max_validator_shares_participating: old_constants.max_validator_shares_participating,
            hub_connection_id: old_constants.hub_connection_id,
            hub_transfer_channel_id: old_constants.hub_transfer_channel_id,
            icq_update_period: old_constants.icq_update_period,
            paused: old_constants.paused,
            is_in_pilot_mode: old_constants.is_in_pilot_mode,
            // Set max_deployment_duration to the value specified in the migrate message
            max_deployment_duration,
        }
    }
}

#[cw_serde]
pub struct ProposalV2_0_1 {
    pub round_id: u64,
    pub tranche_id: u64,
    pub proposal_id: u64,
    pub title: String,
    pub description: String,
    pub power: Uint128,
    pub percentage: Uint128,
    pub deployment_duration: u64,
    pub minimum_atom_liquidity_request: Uint128,
}

impl ProposalV2_0_1 {
    pub fn from(old_proposal: ProposalV1_1_0) -> Self {
        Self {
            round_id: old_proposal.round_id,
            tranche_id: old_proposal.tranche_id,
            proposal_id: old_proposal.proposal_id,
            power: old_proposal.power,
            percentage: old_proposal.percentage,
            title: old_proposal.title,
            description: old_proposal.description,
            // Existing proposals are getting the liquidity deployed for only one round
            deployment_duration: 1,
            minimum_atom_liquidity_request: Uint128::zero(),
        }
    }
}
