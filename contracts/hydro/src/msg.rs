use cosmwasm_std::{Coin, Decimal, Timestamp, Uint128};
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
    pub max_deployment_duration: u64,
    // A vector of tuples, where each tuple contains a round number and the power scaling factor
    // that a lockup has when it has this many rounds left at the end of the round.
    // The vector should be sorted by round number in ascending order.
    // It will always be implicit that 0 rounds lock left corresponds to 0 voting power.
    // Otherwise, it implicitly assumes that between two entries, the larger entries power is used.
    // For example, if the vector is [(1, 1), (2, 1.25), (3, 1.5), (6, 2), (12, 4)],
    // then the power scaling factors are
    // 0x if lockup has expires before the end of the round
    // 1x if lockup has between 0 and 1 epochs left at the end of the round
    // 1.25x if lockup has between 1 and 2 epochs left at the end of the round
    // 1.5x if lockup has between 2 and 3 epochs left at the end of the round
    // 2x if lockup has between 3 and 6 epochs left at the end of the round
    // 4x if lockup has between 6 and 12 epochs left at the end of the round
    pub round_lock_power_schedule: Vec<(u64, Decimal)>,
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
        lock_ids: Vec<u64>,
        lock_duration: u64,
    },
    UnlockTokens {},
    CreateProposal {
        tranche_id: u64,
        title: String,
        description: String,
        deployment_duration: u64,
        minimum_atom_liquidity_request: Uint128,
    },
    Vote {
        tranche_id: u64,
        proposals_votes: Vec<ProposalToLockups>,
    },
    AddAccountToWhitelist {
        address: String,
    },
    RemoveAccountFromWhitelist {
        address: String,
    },
    UpdateConfig {
        max_locked_tokens: Option<u128>,
        max_deployment_duration: Option<u64>,
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

    AddLiquidityDeployment {
        round_id: u64,
        tranche_id: u64,
        proposal_id: u64,
        destinations: Vec<String>,
        deployed_funds: Vec<Coin>,
        funds_before_deployment: Vec<Coin>,
        total_rounds: u64,
        remaining_rounds: u64,
    },

    RemoveLiquidityDeployment {
        round_id: u64,
        tranche_id: u64,
        proposal_id: u64,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ProposalToLockups {
    pub proposal_id: u64,
    pub lock_ids: Vec<u64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct LiquidityDeployment {
    pub round_id: u64,
    pub tranche_id: u64,
    pub proposal_id: u64,
    pub destinations: Vec<String>,
    // allocation assigned to the proposal to be deployed during the specified round
    pub deployed_funds: Vec<Coin>,
    // allocation at the end of the last round for the proposal. this is 0 if no equivalent proposal existed in the last round.
    // if this is a "repeating" proposal (where proposals in subsequent rounds are for the same underlying liqudiity deployment),
    // it's the allocation prior to any potential clawback or increase
    pub funds_before_deployment: Vec<Coin>,
    // how many rounds this proposal has been in effect for if the proposal has a non-zero duration
    pub total_rounds: u64,
    // how many rounds are left for this proposal to be in effect
    // if this is a "repeating" proposal
    pub remaining_rounds: u64,
}
