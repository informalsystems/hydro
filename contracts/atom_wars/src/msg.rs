use cosmwasm_std::{Addr, Timestamp};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::state::{CovenantParams, Tranche};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    pub denom: String,
    pub round_length: u64,
    pub lock_epoch_length: u64,
    pub tranches: Vec<Tranche>,
    pub first_round_start: Timestamp,
    pub whitelist_admins: Vec<Addr>,
    pub initial_whitelist: Vec<CovenantParams>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    LockTokens {
        lock_duration: u64,
    },
    RefreshLockDuration {
        lock_id: u64,
        lock_duration: u64,
    },
    UnlockTokens {},
    CreateProposal {
        tranche_id: u64,
        covenant_params: CovenantParams,
    },
    Vote {
        tranche_id: u64,
        proposal_id: u64,
    },
    AddToWhitelist {
        covenant_params: CovenantParams,
    },
    RemoveFromWhitelist {
        covenant_params: CovenantParams,
    },
    // ExecuteProposal { proposal_id: u64 },
}
