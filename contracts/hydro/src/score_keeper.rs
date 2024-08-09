use cosmwasm_std::{Decimal, StdError, StdResult, Storage};
use cw_storage_plus::{Item, Map};
use std::str::FromStr;

// Constants to define the suffixes for each map
const SHARES_PREFIX: &str = "shares_";
const POWER_PREFIX: &str = "power_";

// Function to create a storage key for the `shares` storage
fn shares_key(suffix: &str) -> String {
    format!("{}{}", SHARES_PREFIX, suffix)
}

// Function to create a storage key for the `shares` map
fn power_key(suffix: &str) -> String {
    format!("{}{}", SHARES_PREFIX, suffix)
}

// Initialize the maps for a given prefix
pub fn initialize(storage: &mut dyn Storage, prefix: &str) -> StdResult<()> {
    let power_key = power_key(prefix);

    // Initialize the total power to 0
    let total_power: Item<Decimal> = Item::new(&power_key);
    total_power.save(storage, &Decimal::zero())?;

    // nothing has to be initialized for the shares, since they are already a map

    Ok(())
}

// Add validator shares and update the total power
pub fn add_validator_shares(
    storage: &mut dyn Storage,
    prefix: &str,
    validator: String,
    num_shares: Decimal,
    power_ratio: Decimal,
) -> StdResult<()> {
    let shares_key = shares_key(prefix);
    let power_key = power_key(prefix);

    let shares_map: Map<&str, Decimal> = Map::new(&shares_key);
    let mut total_power: Item<Decimal> = Item::new(&power_key);

    // Initialize if needed
    initialize(storage, prefix)?;

    // Update the shares map
    let current_shares = shares_map
        .may_load(storage, &validator)?
        .unwrap_or_else(Decimal::zero);
    let updated_shares = current_shares + num_shares;
    shares_map.save(storage, &validator, &updated_shares)?;

    // Update the total power
    let mut current_power = total_power.load(storage)?;
    let added_power = num_shares * power_ratio;
    current_power += added_power;
    total_power.save(storage, &current_power)?;

    Ok(())
}

// Remove validator shares and update the total power
pub fn remove_validator_shares(
    storage: &mut dyn Storage,
    prefix: &str,
    validator: String,
    num_shares: Decimal,
    power_ratio: Decimal,
) -> StdResult<()> {
    let shares_key = shares_key(prefix);
    let power_key = power_key(prefix);

    let shares_map: Map<&str, Decimal> = Map::new(&shares_key);
    let mut total_power: Item<Decimal> = Item::new(&power_key);

    // Initialize if needed
    initialize(storage, prefix)?;

    // Load current shares
    let current_shares = shares_map
        .may_load(storage, &validator)?
        .unwrap_or_else(Decimal::zero);

    // Ensure the validator has enough shares
    if current_shares < num_shares {
        return Err(StdError::generic_err(
            "Insufficient shares for the validator",
        ));
    }

    // Update the shares map
    let updated_shares = current_shares - num_shares;
    shares_map.save(storage, &validator, &updated_shares)?;

    // Update the total power
    let mut current_power = total_power.load(storage)?;
    let removed_power = num_shares * power_ratio;
    current_power -= removed_power;
    total_power.save(storage, &current_power)?;

    Ok(())
}

// Update the power ratio for a validator
pub fn update_power_ratio(
    storage: &mut dyn Storage,
    prefix: &str,
    validator: String,
    old_power_ratio: Decimal,
    new_power_ratio: Decimal,
) -> StdResult<()> {
    let shares_key = shares_key(prefix);
    let power_key = power_key(prefix);

    let shares_map: Map<&str, Decimal> = Map::new(&shares_key);
    let mut total_power: Item<Decimal> = Item::new(&power_key);

    // Initialize if needed
    let _ = initialize(storage, prefix);

    // Load current shares
    let current_shares = shares_map
        .may_load(storage, &validator)?
        .unwrap_or_else(Decimal::zero);
    if current_shares == Decimal::zero() {
        return Ok(()); // No operation if the validator has no shares
    }

    // Update the total power
    let mut current_power = total_power.load(storage)?;
    let old_power = current_shares * old_power_ratio;
    let new_power = current_shares * new_power_ratio;

    current_power = current_power - old_power + new_power;
    total_power.save(storage, &current_power)?;

    Ok(())
}
