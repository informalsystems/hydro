use cosmwasm_std::{Decimal, StdError, StdResult, Storage};
use cw_storage_plus::{Item, Map};
use std::str::FromStr;

// Constants to define the suffixes for each map
const SHARES_PREFIX: &str = "shares_";
const POWER_PREFIX: &str = "power_";
const POWER_RATIO_PREFIX: &str = "power_ratio_";

// Function to create a storage key for the `shares` storage
fn shares_key(suffix: &str) -> String {
    format!("{}{}", SHARES_PREFIX, suffix)
}

// Function to create a storage key for the `shares` map
fn power_key(suffix: &str) -> String {
    format!("{}{}", POWER_PREFIX, suffix)
}

// Function to create a storage key for the `power ratio` map
fn power_ratio_key(suffix: &str) -> String {
    format!("{}{}", POWER_RATIO_PREFIX, suffix)
}

// Initialize the maps for a given prefix
pub fn initialize_if_nil(storage: &mut dyn Storage, prefix: &str) -> StdResult<()> {
    let power_key = power_key(prefix);

    // Initialize the total power to 0
    let total_power: Item<Decimal> = Item::new(&power_key);

    // Initialize if the total power has not been set
    if total_power.may_load(storage)?.is_none() {
        total_power.save(storage, &Decimal::zero())?;
    }

    // nothing has to be initialized for the shares and power ratio, since they are already a map

    Ok(())
}

pub fn get_power_ratio_for_validator(
    storage: &dyn Storage,
    key: &str,
    validator: String,
) -> StdResult<Decimal> {
    let power_ratio_key = power_ratio_key(key);
    let power_ratio_map: Map<&str, Decimal> = Map::new(&power_ratio_key);

    // Load the power ratio for the validator
    let power_ratio = power_ratio_map
        .may_load(storage, &validator)?
        .unwrap_or_else(Decimal::zero);

    Ok(power_ratio)
}

// Add validator shares and update the total power
pub fn add_validator_shares(
    storage: &mut dyn Storage,
    key: &str,
    validator: String,
    num_shares: Decimal,
) -> StdResult<()> {
    let shares_key = shares_key(key);
    let power_key = power_key(key);

    let shares_map: Map<&str, Decimal> = Map::new(&shares_key);
    let total_power: Item<Decimal> = Item::new(&power_key);

    // Initialize if needed
    initialize_if_nil(storage, key)?;

    // Update the shares map
    let current_shares = shares_map
        .may_load(storage, &validator)?
        .unwrap_or_else(Decimal::zero);
    let updated_shares = current_shares + num_shares;
    shares_map.save(storage, &validator, &updated_shares)?;

    // Update the total power
    let mut current_power = total_power.load(storage)?;
    let added_power = num_shares * get_power_ratio_for_validator(storage, key, validator.clone())?;

    current_power += added_power;
    total_power.save(storage, &current_power)?;

    Ok(())
}

// Remove validator shares and update the total power
pub fn remove_validator_shares(
    storage: &mut dyn Storage,
    key: &str,
    validator: String,
    num_shares: Decimal,
) -> StdResult<()> {
    let shares_key = shares_key(key);
    let power_key = power_key(key);

    let shares_map: Map<&str, Decimal> = Map::new(&shares_key);
    let total_power: Item<Decimal> = Item::new(&power_key);

    // Initialize if needed
    initialize_if_nil(storage, key)?;

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
    let removed_power =
        num_shares * get_power_ratio_for_validator(storage, key, validator.clone())?;
    current_power -= removed_power;
    total_power.save(storage, &current_power)?;

    Ok(())
}

// Update the power ratio for a validator and recomputes
// the total power for the given key
pub fn update_power_ratio(
    storage: &mut dyn Storage,
    key: &str,
    validator: String,
    new_power_ratio: Decimal,
) -> StdResult<()> {
    let shares_key = shares_key(key);
    let power_key = power_key(key);
    let power_ratio_key = power_ratio_key(key);

    let shares_map: Map<&str, Decimal> = Map::new(&shares_key);
    let total_power: Item<Decimal> = Item::new(&power_key);
    let power_ratio_map: Map<&str, Decimal> = Map::new(&power_ratio_key);

    // Initialize if needed
    let _ = initialize_if_nil(storage, key);

    // Update the power ratio map
    power_ratio_map.save(storage, &validator, &new_power_ratio)?;

    // Load current shares
    let current_shares = shares_map
        .may_load(storage, &validator)?
        .unwrap_or_else(Decimal::zero);
    if current_shares == Decimal::zero() {
        return Ok(()); // No operation if the validator has no shares
    }

    // Update the total power
    let mut current_power = total_power.load(storage)?;
    let old_power = current_shares * get_power_ratio_for_validator(storage, key, validator)?;
    let new_power = current_shares * new_power_ratio;

    current_power = current_power - old_power + new_power;
    total_power.save(storage, &current_power)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::testing::mock_dependencies;
    use cosmwasm_std::{Decimal, StdError, Storage};
    use proptest::prelude::*;

    // Utility function to initialize a mock storage
    fn initialize_storage() -> Box<dyn Storage> {
        Box::new(mock_dependencies().storage)
    }

    // Helper function to retrieve the shares and power values
    fn get_shares_and_power(
        storage: &dyn Storage,
        prefix: &str,
        validator: &str,
    ) -> (Decimal, Decimal) {
        let shares_key = shares_key(prefix);
        let power_key = power_key(prefix);
        let shares_map: Map<&str, Decimal> = Map::new(&shares_key);
        let total_power: Item<Decimal> = Item::new(&power_key);

        let shares = shares_map
            .may_load(storage, validator)
            .unwrap()
            .unwrap_or_else(Decimal::zero);

        let power = total_power
            .may_load(storage)
            .unwrap()
            .unwrap_or_else(Decimal::zero);

        (shares, power)
    }

    // Table-based tests
    #[test]
    fn test_initialize() {
        let mut binding = initialize_storage();
        let storage = binding.as_mut();

        let key = "test";
        let result = initialize_if_nil(storage, key);
        assert!(result.is_ok());

        let (_, total_power) = get_shares_and_power(storage, key, "validator1");
        assert_eq!(total_power, Decimal::zero());
    }

    #[test]
    fn test_add_validator_shares() {
        let mut binding = initialize_storage();
        let storage = binding.as_mut();

        let key = "test";
        let validator = "validator1";
        let num_shares = Decimal::from_ratio(100u128, 1u128);
        let power_ratio = Decimal::from_ratio(2u128, 1u128);

        // set the power ratio
        update_power_ratio(storage, key, validator.to_string(), power_ratio);

        let result = add_validator_shares(storage, key, validator.to_string(), num_shares);
        assert!(result.is_ok());

        let (shares, total_power) = get_shares_and_power(storage, key, validator);
        assert_eq!(shares, num_shares);
        assert_eq!(total_power, num_shares * power_ratio);
    }

    #[test]
    fn test_remove_validator_shares() {
        let mut binding = initialize_storage();
        let storage = binding.as_mut();

        let key = "test";
        let validator = "validator1";
        let num_shares = Decimal::from_ratio(100u128, 1u128);
        let power_ratio = Decimal::from_ratio(2u128, 1u128);

        // Add shares first
        let _ = add_validator_shares(storage, key, validator.to_string(), num_shares, power_ratio);

        // Now remove shares
        let result =
            remove_validator_shares(storage, key, validator.to_string(), num_shares, power_ratio);
        assert!(result.is_ok());

        let (shares, total_power) = get_shares_and_power(storage, key, validator);
        assert_eq!(shares, Decimal::zero());
        assert_eq!(total_power, Decimal::zero());
    }

    #[test]
    fn test_remove_validator_shares_insufficient_shares() {
        let mut binding = initialize_storage();
        let storage = binding.as_mut();

        let key = "test";
        let validator = "validator1";
        let num_shares = Decimal::from_ratio(100u128, 1u128);
        let power_ratio = Decimal::from_ratio(2u128, 1u128);

        // Add a smaller amount of shares
        let _ = add_validator_shares(
            storage,
            key,
            validator.to_string(),
            Decimal::from_ratio(50u128, 1u128),
            power_ratio,
        );

        // Attempt to remove more shares than exist
        let result =
            remove_validator_shares(storage, key, validator.to_string(), num_shares, power_ratio);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            StdError::generic_err("Insufficient shares for the validator")
        );
    }

    #[test]
    fn test_update_power_ratio() {
        let mut binding = initialize_storage();
        let storage = binding.as_mut();

        let key = "test";
        let validator = "validator1";
        let num_shares = Decimal::from_ratio(100u128, 1u128);
        let old_power_ratio = Decimal::from_ratio(2u128, 1u128);
        let new_power_ratio = Decimal::from_ratio(3u128, 1u128);

        // Add shares first
        let _ = add_validator_shares(
            storage,
            key,
            validator.to_string(),
            num_shares,
            old_power_ratio,
        );

        // Update the power ratio
        let result = update_power_ratio(
            storage,
            key,
            validator.to_string(),
            old_power_ratio,
            new_power_ratio,
        );
        assert!(result.is_ok());

        let (_, total_power) = get_shares_and_power(storage, key, validator);
        assert_eq!(total_power, num_shares * new_power_ratio);
    }

    // Property-based tests using proptest
    proptest! {
        #[test]
        fn proptest_multi_add_remove_validator_shares(num_shares in 1u128..1_000_000u128, num_shares2 in 1u128..1_000_000u128, power_ratio in 1u128..1_000u128, power_ratio2 in 1u128..1_000u128) {
            let mut binding = initialize_storage();
            let storage = binding.as_mut();

            let key = "test";
            let validator = "validator1";
            let num_shares = Decimal::from_ratio(num_shares, 1u128);
            let power_ratio = Decimal::from_ratio(power_ratio, 1u128);
            let num_shares2 = Decimal::from_ratio(num_shares2, 1u128);
            let power_ratio2 = Decimal::from_ratio(power_ratio2, 1u128);

            let res = add_validator_shares(storage, key, validator.to_string(), num_shares, power_ratio);
            assert!(res.is_ok(), "Error adding validator shares: {:?}", res);
            let (shares, total_power) = get_shares_and_power(storage, key, validator);

            // Check if shares and power are correct after adding
            assert_eq!(shares, num_shares);
            assert_eq!(total_power, num_shares * power_ratio);

            // add the second shares
            let res = add_validator_shares(storage, key, validator.to_string(), num_shares2, power_ratio2);
            assert!(res.is_ok(), "Error adding validator shares: {:?}", res);
            let (shares, total_power) = get_shares_and_power(storage, key, validator);

            // Check if shares and power are correct after the second addition
            assert_eq!(shares, num_shares + num_shares2);
            assert_eq!(total_power, num_shares * power_ratio + num_shares2 * power_ratio2);

            // successively remove the shares
            let res = remove_validator_shares(storage, key, validator.to_string(), num_shares2, power_ratio2);
            assert!(res.is_ok(), "Error removing validator shares: {:?}", res);
            let (shares, total_power) = get_shares_and_power(storage, key, validator);

            // Check if shares and power are zero after removing
            assert_eq!(shares, num_shares);
            assert_eq!(total_power, num_shares * power_ratio);

            let res = remove_validator_shares(storage, key, validator.to_string(), num_shares, power_ratio);
            assert!(res.is_ok(), "Error removing validator shares: {:?}", res);
            let (shares, total_power) = get_shares_and_power(storage, key, validator);

            // Check if shares and power are zero after removing the second batch of shares
            assert_eq!(shares, Decimal::zero());
            assert_eq!(total_power, Decimal::zero());
        }

        #[test]
        fn proptest_update_power_ratio(num_shares in 1u128..1_000_000u128, old_power_ratio in 1u128..1_000u128, new_power_ratio in 1u128..1_000u128) {
            let mut binding = initialize_storage();
        let storage = binding.as_mut();

            let key = "test";
            let validator = "validator1";
            let num_shares = Decimal::from_ratio(num_shares, 1u128);
            let old_power_ratio = Decimal::from_ratio(old_power_ratio, 1u128);
            let new_power_ratio = Decimal::from_ratio(new_power_ratio, 1u128);

            let _ = add_validator_shares(storage, key, validator.to_string(), num_shares, old_power_ratio);
            let _ = update_power_ratio(storage, key, validator.to_string(), old_power_ratio, new_power_ratio);

            let (_, total_power) = get_shares_and_power(storage, key, validator);

            // Check if total power is updated correctly
            assert_eq!(total_power, num_shares * new_power_ratio);
        }
    }
}
