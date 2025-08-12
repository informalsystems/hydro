use cosmwasm_std::{Deps, DepsMut, Order, Response};
use cw_storage_plus::{Bound, Item};
use neutron_sdk::bindings::{msg::NeutronMsg, query::NeutronQuery};

use crate::{
    cw721,
    error::ContractError,
    state::{LOCKS_MAP_V2, TOKEN_IDS},
};

// Temporary migration state - tracks the last processed lock_id during TOKEN_IDS migration
// This is removed after migration is complete
pub const TOKEN_IDS_MIGRATION_PROGRESS: Item<u64> = Item::new("token_ids_migration_progress");

/// Migrates existing lockups to populate the TOKEN_IDS store in batches
/// This migration iterates through LOCKS_MAP_V2 and adds lock IDs to TOKEN_IDS
/// if they are NFTs (non-LSM lockups)
pub fn migrate_populate_token_ids(
    deps: &mut DepsMut<NeutronQuery>,
    limit: u32,
) -> Result<Response<NeutronMsg>, ContractError> {
    let limit = limit as usize;

    // Get the last processed lock_id from previous migration runs
    let last_processed = TOKEN_IDS_MIGRATION_PROGRESS.may_load(deps.storage)?;

    // For first run: None (starts from beginning)
    // For subsequent runs: start after last processed (exclusive)
    let start_bound = last_processed.map(Bound::exclusive);

    // Get all lockups starting from appropriate bound, up to 'limit' entries
    let lockups: Vec<_> = LOCKS_MAP_V2
        .range(deps.storage, start_bound, None, Order::Ascending)
        .take(limit)
        .collect::<Result<Vec<_>, _>>()?;

    let mut processed_count = 0;
    let mut added_count = 0;
    let mut last_processed_id = last_processed.unwrap_or(0);

    for (lock_id, lock_entry) in lockups {
        // Check if this lockup should be a NFT (non-LSM lockup)
        if !cw721::is_denom_lsm(&deps.as_ref(), lock_entry.funds.denom)? {
            // Add to TOKEN_IDS if not already present
            if !TOKEN_IDS.has(deps.storage, lock_id) {
                TOKEN_IDS.save(deps.storage, lock_id, &())?;
                added_count += 1;
            }
        }

        processed_count += 1;
        last_processed_id = lock_id;
    }

    // Update migration progress if we processed any items
    if processed_count > 0 {
        TOKEN_IDS_MIGRATION_PROGRESS.save(deps.storage, &last_processed_id)?;
    }

    Ok(Response::new()
        .add_attribute("action", "migrate_populate_token_ids")
        .add_attribute(
            "previous_last_processed",
            last_processed
                .map(|id| id.to_string())
                .unwrap_or("None".to_string()),
        )
        .add_attribute("processed_count", processed_count.to_string())
        .add_attribute("added_count", added_count.to_string())
        .add_attribute("last_processed_id", last_processed_id.to_string()))
}

/// Check if the TOKEN_IDS migration is complete
pub fn is_token_ids_migration_done(deps: Deps<NeutronQuery>) -> Result<bool, ContractError> {
    let last_processed = TOKEN_IDS_MIGRATION_PROGRESS.may_load(deps.storage)?;

    let highest_lock_id = LOCKS_MAP_V2
        .range(deps.storage, None, None, Order::Descending)
        .next()
        .transpose()?
        .map(|(lock_id, _)| lock_id);

    Ok(match highest_lock_id {
        None => true, // No lockups = migration complete
        Some(highest_id) => last_processed.unwrap_or(0) >= highest_id,
    })
}

/// Clean up migration progress after completion
pub fn cleanup_migration_progress(deps: &mut DepsMut<NeutronQuery>) {
    TOKEN_IDS_MIGRATION_PROGRESS.remove(deps.storage);
}
