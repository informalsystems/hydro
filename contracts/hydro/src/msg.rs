use cosmwasm_std::{Timestamp, Uint128};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    pub round_length: u64,
    pub lock_epoch_length: u64,
    pub tranches: Vec<TrancheInfo>,
    pub first_round_start: Timestamp,
    pub max_locked_tokens: Uint128,
    pub whitelist_admins: Vec<String>,
    pub initial_whitelist: Vec<String>,
    pub max_validator_shares_participating: u64,
    pub hub_connection_id: String,
    pub hub_transfer_channel_id: String,
    pub icq_update_period: u64,
    // Anyone can permissionlessly create ICQs, but addresses in this list can attempt
    // to create ICQs without paying, which will then implicitly be paid for by the contract;
    // and they can also withdraw funds in the *native token denom* from the contract;
    // they can however not withdraw user funds that were locked for voting.
    pub icq_managers: Vec<String>,
    pub is_in_pilot_mode: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct TrancheInfo {
    pub name: String,
    pub metadata: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema, cw_orch::ExecuteFns)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    #[cw_orch(payable)]
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
    #[serde(rename = "create_icqs_for_validators")]
    #[cw_orch(payable)]
    CreateICQsForValidators {
        validators: Vec<String>,
    },

    AddICQManager {
        address: String,
    },

    RemoveICQManager {
        address: String,
    },

    WithdrawICQFunds {
        amount: Uint128,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct MigrateMsg {}
