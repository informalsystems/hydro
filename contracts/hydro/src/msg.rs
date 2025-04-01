use std::fmt::{Display, Formatter};

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Binary, Coin, Decimal, Timestamp, Uint128};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::token_manager::TokenInfoProvider;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    pub round_length: u64,
    pub lock_epoch_length: u64,
    pub tranches: Vec<TrancheInfo>,
    pub first_round_start: Timestamp,
    pub max_locked_tokens: Uint128,
    pub whitelist_admins: Vec<String>,
    pub initial_whitelist: Vec<String>,
    // Anyone can permissionlessly create ICQs, but addresses in this list can attempt
    // to create ICQs without paying, which will then implicitly be paid for by the contract;
    // and they can also withdraw funds in the *native token denom* from the contract;
    // they can however not withdraw user funds that were locked for voting.
    pub icq_managers: Vec<String>,
    pub max_deployment_duration: u64,
    // A schedule of how the lock power changes over time.
    // The first element is the round number, the second element is the lock power.
    // See the RoundLockPowerSchedule struct for more information.
    pub round_lock_power_schedule: Vec<(u64, Decimal)>,
    pub token_info_providers: Vec<TokenInfoProviderInstantiateMsg>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct TrancheInfo {
    pub name: String,
    pub metadata: String,
}

#[cw_serde]
pub enum TokenInfoProviderInstantiateMsg {
    // After we extract LSM token info provider into separate contract, all token info providers will be instantiated as SCs.
    #[serde(rename = "lsm")]
    LSM {
        max_validator_shares_participating: u64,
        hub_connection_id: String,
        hub_transfer_channel_id: String,
        icq_update_period: u64,
    },
    TokenInfoProviderContract {
        code_id: u64,
        msg: Binary,
        label: String,
        admin: Option<String>,
    },
}

impl Display for TokenInfoProviderInstantiateMsg {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        match self {
            TokenInfoProviderInstantiateMsg::LSM {
                max_validator_shares_participating,
                hub_connection_id,
                hub_transfer_channel_id,
                icq_update_period,
            } => write!(
                f,
                "LSM(max_validator_shares_participating: {}, hub_connection_id: {}, hub_transfer_channel_id: {}, icq_update_period: {})",
                max_validator_shares_participating,
                hub_connection_id,
                hub_transfer_channel_id,
                icq_update_period
            ),
            TokenInfoProviderInstantiateMsg::TokenInfoProviderContract {
                code_id,
                msg,
                label,
                admin,
            } => write!(
                f,
                "TokenInfoProviderContract(code_id: {}, msg: {}, label: {}, admin: {:?})",
                code_id,
                msg,
                label,
                admin
            ),
        }
    }
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
    UnlockTokens {
        lock_ids: Option<Vec<u64>>,
    },
    CreateProposal {
        round_id: Option<u64>,
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
    Unvote {
        tranche_id: u64,
        lock_ids: Vec<u64>,
    },
    AddAccountToWhitelist {
        address: String,
    },
    RemoveAccountFromWhitelist {
        address: String,
    },
    UpdateConfig {
        activate_at: Timestamp,
        max_locked_tokens: Option<u128>,
        known_users_cap: Option<u128>,
        max_deployment_duration: Option<u64>,
    },
    DeleteConfigs {
        timestamps: Vec<Timestamp>,
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

    UpdateTokenGroupRatio {
        token_group_id: String,
        old_ratio: Decimal,
        new_ratio: Decimal,
    },
    AddTokenInfoProvider {
        token_info_provider: TokenInfoProviderInstantiateMsg,
    },
    RemoveTokenInfoProvider {
        provider_id: String,
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

#[derive(Serialize, Deserialize)]
pub enum ReplyPayload {
    InstantiateTokenInfoProvider(TokenInfoProvider),
}
