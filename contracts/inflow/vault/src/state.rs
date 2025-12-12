use cosmwasm_std::{Addr, StdResult, Storage};
use cw_storage_plus::{Item, Map};
use interface::inflow_vault::{
    AdapterInfo, Config, PayoutEntry, WithdrawalEntry, WithdrawalQueueInfo,
};

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
