use crate::{
    msg::LiquidityDeployment,
    state::{Constants, LockEntry, Proposal, Tranche, VoteWithPower},
};
use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Decimal, Timestamp, Uint128};
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

    #[returns(UserVotesResponse)]
    UserVotes {
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

    #[returns(ICQManagersResponse)]
    ICQManagers {},

    #[returns(TotalLockedTokensResponse)]
    TotalLockedTokens {},

    #[returns(RegisteredValidatorQueriesResponse)]
    RegisteredValidatorQueries {},

    #[returns(ValidatorPowerRatioResponse)]
    ValidatorPowerRatio { validator: String, round_id: u64 },

    #[returns(LiquidityDeploymentResponse)]
    LiquidityDeployment {
        round_id: u64,
        tranche_id: u64,
        proposal_id: u64,
    },

    #[returns(RoundTrancheLiquidityDeploymentsResponse)]
    RoundTrancheLiquidityDeployments {
        round_id: u64,
        tranche_id: u64,
        start_from: u64,
        limit: u64,
    },
}

#[cw_serde]
pub struct ConstantsResponse {
    pub constants: Constants,
}

#[cw_serde]
pub struct TranchesResponse {
    pub tranches: Vec<Tranche>,
}

// LockEntryWithPower is a LockEntry with the current voting power of the sender
// attached. It is used to enrich query responses where the
// lockups are returned with the current voting power of the lockup.
#[cw_serde]
pub struct LockEntryWithPower {
    pub lock_entry: LockEntry,
    pub current_voting_power: Uint128,
}

#[cw_serde]
pub struct AllUserLockupsResponse {
    pub lockups: Vec<LockEntryWithPower>,
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
pub struct UserVotesResponse {
    pub votes: Vec<VoteWithPower>,
}

#[cw_serde]
pub struct CurrentRoundResponse {
    pub round_id: u64,
    pub round_end: Timestamp,
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

// A vector containing tuples, where each tuple contains a validator address
// and the id of the interchain query associated with that validator.
#[cw_serde]
pub struct RegisteredValidatorQueriesResponse {
    pub query_ids: Vec<(String, u64)>,
}

#[cw_serde]
pub struct ValidatorPowerRatioResponse {
    pub ratio: Decimal,
}

#[cw_serde]
pub struct ICQManagersResponse {
    pub managers: Vec<Addr>,
}

#[cw_serde]
pub struct LiquidityDeploymentResponse {
    pub liquidity_deployment: LiquidityDeployment,
}

#[cw_serde]
pub struct RoundTrancheLiquidityDeploymentsResponse {
    pub liquidity_deployments: Vec<LiquidityDeployment>,
}
