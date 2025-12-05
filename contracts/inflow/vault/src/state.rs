use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, StdResult, Storage, Timestamp, Uint128};
use cw_storage_plus::{Item, Map};

/// Configuration of the Inflow smart contract
pub const CONFIG: Item<Config> = Item::new("config");

/// Addresses that are allowed to execute permissioned actions on the smart contract.
pub const WHITELIST: Map<Addr, ()> = Map::new("whitelist");

/// Next withdrawal ID to be used when a user makes a withdrawal request that ends up in the queue.
pub const NEXT_WITHDRAWAL_ID: Item<u64> = Item::new("next_withdrawal_id");

/// Keeps track of the last withdrawal ID that has been funded and marked as ready to be paid out to a user.
pub const LAST_FUNDED_WITHDRAWAL_ID: Item<u64> = Item::new("last_funded_withdrawal_id");

/// Next payout ID to be used to record history when a user actually gets the tokens paid out.
pub const NEXT_PAYOUT_ID: Item<u64> = Item::new("next_payout_id");

/// Pending withdrawal requests queue. The key is the withdrawal ID, and the value is a WithdrawalEntry.
/// We use auto-incrementing withdrawal IDs to be able to fulfill withdrawal requests in a "first comes-
/// first served" manner.
/// WITHDRAWAL_REQUESTS: key(withdrawal_id) -> WithdrawalEntry
pub const WITHDRAWAL_REQUESTS: Map<u64, WithdrawalEntry> = Map::new("withdrawal_requests");

/// Information about the current state of withdrawal queue, including total shares burned,
/// total withdrawal amount and withdrawal amount requested that hasn't been provided yet.
pub const WITHDRAWAL_QUEUE_INFO: Item<WithdrawalQueueInfo> = Item::new("withdrawal_queue_info");

/// Mapping from user address to a list of their current withdrawal request IDs.
/// USER_WITHDRAWAL_REQUESTS: key(user_address) -> withdrawal_request_ids
pub const USER_WITHDRAWAL_REQUESTS: Map<Addr, Vec<u64>> = Map::new("user_withdrawal_requests");

/// History of all payouts made to users.
/// PAYOUTS_HISTORY: key(user_address, payout_id) -> PayoutEntry
pub const PAYOUTS_HISTORY: Map<(Addr, u64), PayoutEntry> = Map::new("payouts_history");

/// Registered adapters for deploying funds to external protocols.
/// ADAPTERS: key(adapter_name) -> AdapterInfo
pub const ADAPTERS: Map<String, AdapterInfo> = Map::new("adapters");

#[cw_serde]
pub struct Config {
    /// Token denom that users can deposit into the vault.
    pub deposit_denom: String,
    /// Denom of the vault shares token that is issued to users when they deposit tokens into the vault.
    pub vault_shares_denom: String,
    /// Address of the Control Center contract.
    pub control_center_contract: Addr,
    /// Address of the token info provider contract used to obtain the ratio of the
    /// deposit token to the base token. If None, then the deposit token is the base token.
    pub token_info_provider_contract: Option<Addr>,
    /// Maximum number of pending withdrawal requests allowed per user.
    pub max_withdrawals_per_user: u64,
}

#[cw_serde]
pub struct WithdrawalQueueInfo {
    // Total shares burned for all withdrawals currently in the withdrawal queue.
    pub total_shares_burned: Uint128,
    // Total amount to be withdrawn for all withdrawal entries currently in the withdrawal queue.
    // Sum of all `amount_to_receive` fields in all withdrawal entries, regardless of whether the
    // funds for payouts have been provided to the smart contract or not.
    pub total_withdrawal_amount: Uint128,
    // Sum of `amount_to_receive` fields of all withdrawal entries from the withdrawal queue
    // for which the funds have not been provided yet.
    pub non_funded_withdrawal_amount: Uint128,
}

#[cw_serde]
pub struct WithdrawalEntry {
    pub id: u64,
    pub initiated_at: Timestamp,
    pub withdrawer: Addr,
    pub shares_burned: Uint128,
    pub amount_to_receive: Uint128,
    pub is_funded: bool,
}
#[cw_serde]
pub struct PayoutEntry {
    pub id: u64,
    pub executed_at: Timestamp,
    pub recipient: Addr,
    pub vault_shares_burned: Uint128,
    pub amount_received: Uint128,
}

/// Controls whether an adapter participates in automated allocation
#[cw_serde]
pub enum AllocationMode {
    /// Adapter is included in automated deposit/withdrawal allocation via calculate_venues_allocation
    Automated,
    /// Adapter is only accessible via manual DepositToAdapter/WithdrawFromAdapter operations
    Manual,
}

/// Controls whether adapter operations update the Control Center's deployed amount
///
/// # Race Condition Warnings
///
/// ## Automated + Tracked (DANGEROUS)
/// Using `Tracked` with `AllocationMode::Automated` creates a race condition:
/// 1. Automated deposits/withdrawals update deployed amount without admin knowledge
/// 2. If a manual `SubmitDeployedAmount` call is in progress, it may overwrite with stale data
/// 3. Result: Deployed amount becomes inaccurate
///
/// **Recommendation**: Use `NotTracked` for automated adapters unless absolutely necessary.
///
/// ## Manual + Tracked
/// When using `Tracked` with `AllocationMode::Manual`:
/// - Ensure no `SubmitDeployedAmount` proposal is pending before manual operations
/// - After manual deposit/withdraw, re-snapshot deployed amount if a proposal was in flight
/// - Otherwise, the proposal will overwrite the auto-updated value with stale data
#[cw_serde]
pub enum DeploymentTracking {
    /// Deposits/withdrawals update Control Center's deployed amount
    /// WARNING: See race condition notes above
    Tracked,
    /// Position is queryable but not included in deployed amount
    /// RECOMMENDED for automated adapters
    NotTracked,
}

/// Information about a registered adapter
#[cw_serde]
pub struct AdapterInfo {
    /// Contract address of the adapter
    pub address: Addr,
    /// Controls whether adapter participates in automated deposit/withdrawal allocation.
    /// When Manual, the adapter is skipped by calculate_venues_allocation.
    /// Admin operations (DepositToAdapter, WithdrawFromAdapter) work regardless of this setting.
    pub allocation_mode: AllocationMode,
    /// Controls whether deposits/withdrawals to/from this adapter update Control Center's deployed amount.
    /// When Tracked, operations call AddToDeployedAmount/SubFromDeployedAmount.
    /// When NotTracked, position is queryable via DepositorPosition but not in deployed amount.
    pub deployment_tracking: DeploymentTracking,
    /// Human-readable name for display purposes
    pub name: String,
    /// Optional description of the adapter and what protocol it integrates with
    pub description: Option<String>,
}

pub fn load_config(storage: &dyn Storage) -> StdResult<Config> {
    CONFIG.load(storage)
}

pub fn load_withdrawal_queue_info(storage: &dyn Storage) -> StdResult<WithdrawalQueueInfo> {
    WITHDRAWAL_QUEUE_INFO.load(storage)
}

/// Retrieves the withdrawal ID to be used and increments the stored value for the next withdrawal ID.
pub fn get_next_withdrawal_id(storage: &mut dyn Storage) -> StdResult<u64> {
    let withdrawal_id = NEXT_WITHDRAWAL_ID.load(storage)?;
    NEXT_WITHDRAWAL_ID.save(storage, &(withdrawal_id + 1))?;

    Ok(withdrawal_id)
}

/// Retrieves the payout ID to be used and increments the stored value for the next payout ID.
pub fn get_next_payout_id(storage: &mut dyn Storage) -> StdResult<u64> {
    let payout_id = NEXT_PAYOUT_ID.load(storage)?;
    NEXT_PAYOUT_ID.save(storage, &(payout_id + 1))?;

    Ok(payout_id)
}
