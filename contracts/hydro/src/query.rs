use crate::{
    msg::LiquidityDeployment,
    state::{Constants, LockEntry, Proposal, Tranche, Vote, VoteWithPower},
    token_manager::TokenInfoProvider,
};
use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Timestamp, Uint128};

#[cw_serde]
#[derive(QueryResponses, cw_orch::QueryFns)]
pub enum QueryMsg {
    #[returns(ConstantsResponse)]
    Constants {},

    #[returns(TokenInfoProvidersResponse)]
    TokenInfoProviders {},

    #[returns(TranchesResponse)]
    Tranches {},

    #[returns(AllUserLockupsResponse)]
    AllUserLockups {
        address: String,
        start_from: u32,
        limit: u32,
    },

    #[returns(SpecificUserLockupsResponse)]
    SpecificUserLockups { address: String, lock_ids: Vec<u64> },

    // a version of the AllUserLockups query where additional information
    // is returned
    #[returns(AllUserLockupsWithTrancheInfosResponse)]
    AllUserLockupsWithTrancheInfos {
        address: String,
        start_from: u32,
        limit: u32,
    },

    #[returns(SpecificUserLockupsWithTrancheInfosResponse)]
    SpecificUserLockupsWithTrancheInfos { address: String, lock_ids: Vec<u64> },

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

    #[returns(AllVotesResponse)]
    AllVotes { start_from: u32, limit: u32 },

    #[returns(AllVotesRoundTrancheResponse)]
    AllVotesRoundTranche {
        round_id: u64,
        tranche_id: u64,
        start_from: u32,
        limit: u32,
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

    #[returns(CanLockDenomResponse)]
    CanLockDenom { token_denom: String },

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
    #[returns(TotalPowerAtHeightResponse)]
    TotalPowerAtHeight { height: Option<u64> },
    #[returns(VotingPowerAtHeightResponse)]
    VotingPowerAtHeight {
        address: String,
        height: Option<u64>,
    },
}

#[cw_serde]
pub struct ConstantsResponse {
    pub constants: Constants,
}

#[cw_serde]
pub struct TokenInfoProvidersResponse {
    pub providers: Vec<TokenInfoProvider>,
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

// PerTrancheLockupInfo is used to store the lockup information for a specific tranche.
#[cw_serde]
pub struct PerTrancheLockupInfo {
    pub tranche_id: u64,
    // If this number is less or equal to the current round, it means the lockup can vote in the current round.
    pub next_round_lockup_can_vote: u64,
    // This is the proposal that the lockup is voting for in the current round, if any.
    // In particular, if the lockup is blocked from voting in the current round (because it voted for a
    // proposal with a long deployment duration in a previous round), this will be None.
    pub current_voted_on_proposal: Option<u64>,

    // This is the id of the proposal that the lockup is tied to because it has voted for a proposal with a long deployment duration.
    // In case the lockup can currently vote (and is not tied to a proposal), this will be None.
    // Note that None will also be returned if the lockup voted for a proposal that received a deployment with zero funds.
    pub tied_to_proposal: Option<u64>,
}

// LockupWithPerTrancheInfo is used to store the lockup information for a specific lockup,
// together with lockup-specific information for each tranche.
#[cw_serde]
pub struct LockupWithPerTrancheInfo {
    pub lock_with_power: LockEntryWithPower,
    pub per_tranche_info: Vec<PerTrancheLockupInfo>,
}

#[cw_serde]
pub struct AllUserLockupsResponse {
    pub lockups: Vec<LockEntryWithPower>,
}

// This is necessary because otherwise, cosmwasm-ts-codegen does not generate SpecificUserLockupsResponse
// pub type SpecificUserLockupsResponse = AllUserLockupsResponse; does not seem to work
#[cw_serde]
pub struct SpecificUserLockupsResponse {
    pub lockups: Vec<LockEntryWithPower>,
}

// A version of AllUserLockupsResponse that includes the per-tranche information for each lockup.
#[cw_serde]
pub struct AllUserLockupsWithTrancheInfosResponse {
    pub lockups_with_per_tranche_infos: Vec<LockupWithPerTrancheInfo>,
}

// This is necessary because otherwise, cosmwasm-ts-codegen does not generate SpecificUserLockupsWithTrancheInfosResponse
// pub type SpecificUserLockupsWithTrancheInfosResponse = AllUserLockupsWithTrancheInfosResponse; does not seem to work
#[cw_serde]
pub struct SpecificUserLockupsWithTrancheInfosResponse {
    pub lockups_with_per_tranche_infos: Vec<LockupWithPerTrancheInfo>,
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
pub struct VoteEntry {
    pub round_id: u64,
    pub tranche_id: u64,
    pub sender_addr: Addr,
    pub lock_id: u64,
    pub vote: Vote,
}

#[cw_serde]
pub struct AllVotesResponse {
    pub votes: Vec<VoteEntry>,
}

#[cw_serde]
pub struct AllVotesRoundTrancheResponse {
    pub votes: Vec<VoteEntry>,
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
pub struct CanLockDenomResponse {
    pub denom: String,
    pub can_be_locked: bool,
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

//TotalPowerAtHeightResponse and VotingPowerAtHeightResponse conform to the DAODAO interface for a voting power module:
// https://github.com/DA0-DA0/dao-contracts/blob/development/packages/dao-interface/src/voting.rs
// TotalPowerAtHeightResponse and VotingPowerAtHeightResponse are defined instead of using the ones from dao-interface
// so that we can use them in tests and other places without having to convert Uint128 from v1 to v2 and other way round
// because DAO DAO currently uses CosmWasm 1.5, and we are on version 2.
#[cw_serde]
pub struct TotalPowerAtHeightResponse {
    pub power: Uint128,
    pub height: u64,
}

#[cw_serde]
pub struct VotingPowerAtHeightResponse {
    pub power: Uint128,
    pub height: u64,
}
