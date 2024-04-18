use cosmwasm_std::{Timestamp, Uint128};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::state::Tranche;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    pub denom: String,
    pub round_length: u64,
    pub total_pool: Uint128,
    pub tranches: Vec<Tranche>,
    pub first_round_start: Timestamp,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    LockTokens {
        lock_duration: u64,
    },
    UnlockTokens {},
    CreateProposal {
        tranche_id: u64,
        covenant_params: String,
    },
    Vote {
        tranche_id: u64,
        proposal_id: u64,
    },
    // ExecuteProposal { proposal_id: u64 },
}
