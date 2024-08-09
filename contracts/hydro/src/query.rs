use crate::state::{Constants, LockEntry, Proposal, Tranche, Vote};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Timestamp, Uint128};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Constants {},
    Tranches {},
    AllUserLockups {
        address: String,
        start_from: u32,
        limit: u32,
    },
    ExpiredUserLockups {
        address: String,
        start_from: u32,
        limit: u32,
    },
    UserVotingPower {
        address: String,
    },
    UserVote {
        round_id: u64,
        tranche_id: u64,
        address: String,
    },
    CurrentRound {},
    RoundEnd {
        round_id: u64,
    },
    RoundTotalVotingPower {
        round_id: u64,
    },
    RoundProposals {
        round_id: u64,
        tranche_id: u64,
        start_from: u32,
        limit: u32,
    },
    Proposal {
        round_id: u64,
        tranche_id: u64,
        proposal_id: u64,
    },
    TopNProposals {
        round_id: u64,
        tranche_id: u64,
        number_of_proposals: usize,
    },
    Whitelist {},
    WhitelistAdmins {},
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
