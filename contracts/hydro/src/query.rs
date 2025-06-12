use crate::{
    msg::{CollectionInfo, LiquidityDeployment},
    state::{Approval, Constants, LockEntryV2, Proposal, Tranche, Vote, VoteWithPower},
    token_manager::TokenInfoProvider,
};
use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Decimal, Timestamp, Uint128};

#[cw_serde]
#[derive(QueryResponses, cw_orch::QueryFns)]
pub enum QueryMsg {
    #[returns(ConstantsResponse)]
    Constants {},

    #[returns(TokenInfoProvidersResponse)]
    TokenInfoProviders {},

    #[returns(GatekeeperResponse)]
    Gatekeeper {},

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

    #[returns(UserVotedLocksResponse)]
    UserVotedLocks {
        user_address: String,
        round_id: u64,
        tranche_id: u64,
        proposal_id: Option<u64>,
    },

    #[returns(LockVotesHistoryResponse)]
    LockVotesHistory {
        lock_id: u64,
        start_from_round_id: Option<u64>,
        stop_at_round_id: Option<u64>,
        tranche_id: Option<u64>,
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

    /// Returns the owner of the given token, as well as anyone with approval on this particular token.
    /// If the token is unknown, returns an error.
    /// If include_expired is set (to true), shows expired approvals in the results, otherwise, ignore them.
    #[returns(OwnerOfResponse)]
    OwnerOf {
        token_id: String,
        include_expired: Option<bool>,
    },
    /// Returns an approval of spender about the given token_id.
    /// If include_expired is set (to true), shows expired approvals in the results, otherwise, ignore them.
    #[returns(ApprovalResponse)]
    Approval {
        token_id: String,
        spender: String,
        include_expired: Option<bool>,
    },
    /// Return all approvals that apply on the given token_id.
    /// If include_expired is set (to true), show expired approvals in the results, otherwise, ignore them.
    #[returns(ApprovalsResponse)]
    Approvals {
        token_id: String,
        include_expired: Option<bool>,
    },
    /// List operators that can access all of the owner's tokens.
    #[returns(OperatorsResponse)]
    AllOperators {
        owner: String,
        include_expired: Option<bool>,
        start_after: Option<String>,
        limit: Option<u32>,
    },
    /// Total number of tokens (lockups) issued so far
    #[returns(NumTokensResponse)]
    NumTokens {},

    #[returns(CollectionInfoResponse)]
    CollectionInfo {},
    /// Returns metadata about one particular token (as LockupWithPerTrancheInfo).
    #[returns(NftInfoResponse)]
    NftInfo { token_id: String },
    /// Returns the result of both `NftInfo` and `OwnerOf` as one query as an optimization for clients
    /// If include_expired is set (to true), shows expired approvals in the results, otherwise, ignore them.
    #[returns(AllNftInfoResponse)]
    AllNftInfo {
        token_id: String,
        /// unset or false will filter out expired approvals, you must set to true to see them
        include_expired: Option<bool>,
    },

    /// Lists token_ids owned by a given owner, [] if no tokens.
    #[returns(TokensResponse)]
    Tokens {
        owner: String,
        start_after: Option<String>,
        limit: Option<u32>,
    },
    /// Lists token_ids controlled by the contract.
    #[returns(TokensResponse)]
    AllTokens {
        start_after: Option<String>,
        limit: Option<u32>,
    },
}

#[cw_serde]
pub struct TokensResponse {
    /// Contains all token_ids in lexicographical ordering
    /// If there are more than `limit`, use `start_after` in future queries
    /// to achieve pagination.
    pub tokens: Vec<String>,
}

#[cw_serde]
pub struct NumTokensResponse {
    pub count: u64,
}

#[cw_serde]
pub struct ApprovalResponse {
    pub approval: Approval,
}

#[cw_serde]
pub struct ApprovalsResponse {
    pub approvals: Vec<Approval>,
}

#[cw_serde]
pub struct OperatorsResponse {
    pub operators: Vec<Approval>,
}

#[cw_serde]
pub struct NftInfoResponse {
    /// Universal resource identifier for this NFT
    /// Should point to a JSON file that conforms to the ERC721
    /// Metadata JSON Schema
    pub token_uri: Option<String>,
    /// You can add any custom metadata here when you extend cw721-base
    pub extension: LockupWithPerTrancheInfo,
}

#[cw_serde]
pub struct AllNftInfoResponse {
    /// Who can transfer the token
    pub access: OwnerOfResponse,
    /// Data on the token itself,
    pub info: NftInfoResponse,
}
#[cw_serde]
pub struct OwnerOfResponse {
    /// Owner of the token
    pub owner: String,
    /// If set this address is approved to transfer/send the token as well
    pub approvals: Vec<Approval>,
}

pub type CollectionInfoResponse = CollectionInfo;

#[cw_serde]
pub struct ConstantsResponse {
    pub constants: Constants,
}

#[cw_serde]
pub struct TokenInfoProvidersResponse {
    pub providers: Vec<TokenInfoProvider>,
}

#[cw_serde]
pub struct GatekeeperResponse {
    pub gatekeeper: String,
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
    pub lock_entry: LockEntryV2,
    pub current_voting_power: Uint128,
}

#[cw_serde]
pub struct RoundWithBid {
    pub round_id: u64,
    pub proposal_id: u64,
    pub round_end: Timestamp,
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

    /// This is the list of proposals that the lockup has been used to vote for in the past.
    /// It is used to show the history of the lockup upon transfer / selling on Marketplace.
    /// Note that this does not include the current voted on proposal, which is found in the current_voted_on_proposal field.
    pub historic_voted_on_proposals: Vec<RoundWithBid>,
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
    pub lockups: Vec<LockEntryV2>,
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
pub struct VotedLockInfo {
    pub lock_id: u64,
    pub vote_power: Decimal,
}

#[cw_serde]
pub struct UserVotedLocksResponse {
    // Maps proposal_id to a list of locks that voted for it with their voting power
    // The first item in each tuple is the proposal_id
    pub voted_locks: Vec<(u64, Vec<VotedLockInfo>)>,
}

#[cw_serde]
pub struct LockVotesHistoryEntry {
    pub round_id: u64,
    pub tranche_id: u64,
    pub proposal_id: u64,
    pub vote_power: Decimal,
}

#[cw_serde]
pub struct LockVotesHistoryResponse {
    pub vote_history: Vec<LockVotesHistoryEntry>,
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
