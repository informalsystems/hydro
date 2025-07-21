use std::fmt::{Display, Formatter};

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    to_json_binary, Addr, Binary, Coin, CosmosMsg, Decimal, StdResult, Timestamp, Uint128, WasmMsg,
};
use cw_utils::Expiration;
use interface::gatekeeper::SignatureInfo;
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
    // Provides inputs for instantiation of the Gatekeeper contract that
    // controls how many tokens each user can lock at a given point in time.
    pub gatekeeper: Option<InstantiateContractMsg>,
    // The CW721 Collection Info (default: name: "Hydro Lockups", symbol: "hydro-lockups")
    pub cw721_collection_info: Option<CollectionInfo>,
    // Maximum duration (in seconds) after which a lock is considered expired in lock tracking.
    pub lock_expiry_duration_seconds: u64,
    // Maximum allowed depth of a lock's ancestor tree to prevent excessive nesting and state complexity.
    pub lock_depth_limit: u64,
    // Pending slashes are accumulated for a lockup until the slash percentage threshold is reached.
    // After that, pending slashes will be applied to the lockup and tokens will be deducted from it.
    pub slash_percentage_threshold: Decimal,
    // Address that will receive the tokens slashed from the lockups.
    pub slash_tokens_receiver_addr: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct TrancheInfo {
    pub name: String,
    pub metadata: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct CollectionInfo {
    pub name: String,
    pub symbol: String,
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
    Base {
        token_group_id: String,
        denom: String,
    },
    TokenInfoProviderContract {
        code_id: u64,
        msg: Binary,
        label: String,
        admin: Option<String>,
    },
}

#[cw_serde]
pub struct InstantiateContractMsg {
    pub code_id: u64,
    pub msg: Binary,
    pub label: String,
    pub admin: Option<String>,
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
                "LSM(max_validator_shares_participating: {max_validator_shares_participating}, hub_connection_id: {hub_connection_id}, hub_transfer_channel_id: {hub_transfer_channel_id}, icq_update_period: {icq_update_period})"
            ),
            TokenInfoProviderInstantiateMsg::Base {
                token_group_id,
                denom,
            } => write!(
                f,
                "Base(token_group_id: {token_group_id}, denom: {denom})"
            ),
            TokenInfoProviderInstantiateMsg::TokenInfoProviderContract {
                code_id,
                msg,
                label,
                admin,
            } => write!(
                f,
                "TokenInfoProviderContract(code_id: {code_id}, msg: {msg}, label: {label}, admin: {admin:?})"
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
        proof: Option<LockTokensProof>,
    },
    RefreshLockDuration {
        lock_ids: Vec<u64>,
        lock_duration: u64,
    },
    SplitLock {
        lock_id: u64,
        amount: Uint128,
    },
    MergeLocks {
        lock_ids: Vec<u64>,
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
        config: UpdateConfigData,
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
    SetGatekeeper {
        gatekeeper_addr: Option<String>,
    },
    /// Transfer is a base message to move a lockup to another account without triggering actions
    TransferNft {
        recipient: String,
        token_id: String,
    },
    /// This transfers ownership of the token to contract account.
    /// contract must be an address controlled by a smart contract, which implements the CW721Receiver interface.
    /// The msg will be passed to the recipient contract, along with the token_id.
    SendNft {
        contract: String,
        token_id: String,
        msg: Binary,
    },
    /// Allows spender to transfer / send the lockup from the owner's account.
    /// If expiration is set, then this allowance has a time/height limit
    Approve {
        spender: String,
        token_id: String,
        expires: Option<Expiration>,
    },
    /// Remove previously granted Approval
    Revoke {
        spender: String,
        token_id: String,
    },
    /// Allows operator to transfer / send any token from the owner's account.
    /// If expiration is set, then this allowance has a time/height limit
    ApproveAll {
        operator: String,
        expires: Option<Expiration>,
    },
    /// Remove previously granted ApproveAll permission
    RevokeAll {
        operator: String,
    },
    /// Allows whitelisted admin to set the drop token info for lockup conversions.
    SetDropTokenInfo {
        core_address: String,
        d_token_denom: String,
        puppeteer_address: String,
    },

    /// Allows users to convert their lockups to dTokens.
    /// This action is only available if the drop token info is set.
    ConvertLockupToDtoken {
        lock_ids: Vec<u64>,
    },
    /// Allows whitelisted admins to slash lockups that voted for a given proposal.
    SlashProposalVoters {
        round_id: u64,
        tranche_id: u64,
        proposal_id: u64,
        slash_percent: Decimal,
        start_from: u64,
        limit: u64,
    },
    /// Allows users to remove/reduce pending slash fully or partially by inserting funds
    BuyoutPendingSlash {
        lock_id: u64,
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

/// For detailed explanation of the fields take a look at ExecuteLockTokensMsg located in the interface package
#[cw_serde]
pub struct LockTokensProof {
    pub maximum_amount: Uint128,
    pub proof: Vec<String>,
    pub sig_info: Option<SignatureInfo>,
}

#[cw_serde]
pub struct ConvertLockupPayload {
    pub lock_id: u64,
    pub amount: Uint128,
    pub sender: Addr,
}

#[cw_serde]
pub struct UpdateConfigData {
    pub activate_at: Timestamp,
    pub max_locked_tokens: Option<u128>,
    pub known_users_cap: Option<u128>,
    pub max_deployment_duration: Option<u64>,
    pub cw721_collection_info: Option<CollectionInfo>,
    pub lock_depth_limit: Option<u64>,
    pub lock_expiry_duration_seconds: Option<u64>,
    pub slash_percentage_threshold: Option<Decimal>,
    pub slash_tokens_receiver_addr: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub enum ReplyPayload {
    InstantiateTokenInfoProvider(TokenInfoProvider),
    InstantiateGatekeeper,
    ConvertLockup(ConvertLockupPayload),
}

#[cw_serde]
pub struct Cw721ReceiveMsg {
    pub sender: String,
    pub token_id: String,
    pub msg: Binary,
}

impl Cw721ReceiveMsg {
    /// serializes the message
    pub fn into_json_binary(self) -> StdResult<Binary> {
        let msg = ReceiverExecuteMsg::ReceiveNft(self);
        to_json_binary(&msg)
    }

    /// creates a cosmos_msg sending this struct to the named contract
    pub fn into_cosmos_msg<TAddress: Into<String>, TCustomResponseMsg>(
        self,
        contract_addr: TAddress,
    ) -> StdResult<CosmosMsg<TCustomResponseMsg>>
    where
        TCustomResponseMsg: Clone + std::fmt::Debug + PartialEq + JsonSchema,
    {
        let msg = self.into_json_binary()?;
        let execute = WasmMsg::Execute {
            contract_addr: contract_addr.into(),
            msg,
            funds: vec![],
        };
        Ok(execute.into())
    }
}

/// This is just a helper to properly serialize the above message.
/// The actual receiver should include this variant in the larger ExecuteMsg enum
#[cw_serde]
pub enum ReceiverExecuteMsg {
    ReceiveNft(Cw721ReceiveMsg),
}
