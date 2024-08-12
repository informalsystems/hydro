use cosmwasm_std::Timestamp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    pub round_length: u64,
    pub lock_epoch_length: u64,
    pub tranches: Vec<TrancheInfo>,
    pub first_round_start: Timestamp,
    pub max_locked_tokens: u128,
    pub whitelist_admins: Vec<String>,
    pub initial_whitelist: Vec<String>,
    pub max_validator_shares_participating: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct TrancheInfo {
    pub name: String,
    pub metadata: String,
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
        title: String,
        description: String,
    },
    Vote {
        tranche_id: u64,
        proposal_id: u64,
    },
    AddAccountToWhitelist {
        address: String,
    },
    RemoveAccountFromWhitelist {
        address: String,
    },
    UpdateMaxLockedTokens {
        max_locked_tokens: u128,
    },
    Pause {},
    AddTranche {
        tranche: TrancheInfo,
    },
    EditTranche {
        tranche_id: u64,
        tranche_name: Option<String>,
        tranche_metadata: Option<String>,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct MigrateMsg {}
