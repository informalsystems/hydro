use crate::state::{LockEntry, Proposal};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Constants {},
    AllUserLockups {
        address: String,
    },
    ExpiredUserLockups {
        address: String,
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
    RoundProposals {
        round_id: u64,
        tranche_id: u64,
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
}

#[derive(Serialize, Deserialize, Clone, PartialEq, JsonSchema, Debug, Default)]
pub struct UserLockupsResponse {
    pub lockups: Vec<LockEntry>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, JsonSchema, Debug, Default)]
pub struct RoundProposalsResponse {
    pub proposals: Vec<Proposal>,
}
