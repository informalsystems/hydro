use crate::state::{Constants, LockEntry, Proposal, Tranche, Vote};
use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Timestamp, Uint128};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(
    Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema, QueryResponses, cw_orch::QueryFns,
)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    #[returns(ConstantsResponse)]
    Constants {},

    #[returns(TranchesResponse)]
    Tranches {},

    #[returns(AllUserLockupsResponse)]
    AllUserLockups {
        address: String,
        start_from: u32,
        limit: u32,
    },

    #[returns(ExpiredUserLockupsResponse)]
    ExpiredUserLockups {
        address: String,
        start_from: u32,
        limit: u32,
    },

    #[returns(UserVotingPowerResponse)]
    UserVotingPower { address: String },

    #[returns(UserVoteResponse)]
    UserVote {
        round_id: u64,
        tranche_id: u64,
        address: String,
    },

    #[returns(CurrentRoundResponse)]
    CurrentRound {},

    #[returns(RoundEndResponse)]
    RoundEnd { round_id: u64 },

    #[returns(RoundTotalVotingPowerResponse)]
    RoundTotalVotingPower { round_id: u64 },

    #[returns(RoundProposalsResponse)]
    RoundProposals {
        round_id: u64,
        tranche_id: u64,
        start_from: u32,
        limit: u32,
    },

    #[returns(ProposalResponse)]
    Proposal {
        round_id: u64,
        tranche_id: u64,
        proposal_id: u64,
    },

    #[returns(TopNProposalsResponse)]
    TopNProposals {
        round_id: u64,
        tranche_id: u64,
        number_of_proposals: usize,
    },

    #[returns(WhitelistResponse)]
    Whitelist {},

    #[returns(WhitelistAdminsResponse)]
    WhitelistAdmins {},

    #[returns(TotalLockedTokensResponse)]
    TotalLockedTokens {},
}

#[cw_serde]
pub struct ConstantsResponse {
    pub constants: Constants,
}

#[cw_serde]
pub struct TranchesResponse {
    pub tranches: Vec<Tranche>,
}

#[cw_serde]
pub struct AllUserLockupsResponse {
    pub lockups: Vec<LockEntry>,
}

#[cw_serde]
pub struct ExpiredUserLockupsResponse {
    pub lockups: Vec<LockEntry>,
}

#[cw_serde]
pub struct UserVotingPowerResponse {
    pub voting_power: u128,
}

#[cw_serde]
pub struct UserVoteResponse {
    pub vote: Vote,
}

#[cw_serde]
pub struct CurrentRoundResponse {
    pub round_id: u64,
}

#[cw_serde]
pub struct RoundEndResponse {
    pub round_end: Timestamp,
}

#[cw_serde]
pub struct RoundTotalVotingPowerResponse {
    pub total_voting_power: Uint128,
}

#[cw_serde]
pub struct ProposalResponse {
    pub proposal: Proposal,
}

#[cw_serde]
pub struct TopNProposalsResponse {
    pub proposals: Vec<Proposal>,
}
#[cw_serde]
pub struct WhitelistResponse {
    pub whitelist: Vec<Addr>,
}

#[cw_serde]
pub struct WhitelistAdminsResponse {
    pub admins: Vec<Addr>,
}

#[cw_serde]
pub struct TotalLockedTokensResponse {
    pub total_locked_tokens: u128,
}

#[cw_serde]
pub struct RoundProposalsResponse {
    pub proposals: Vec<Proposal>,
}
