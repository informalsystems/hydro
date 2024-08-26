use cosmwasm_std::{Decimal, StdError, StdResult, Storage};
use cw_storage_plus::{Item, Map};

use crate::{
    lsm_integration::get_validator_power_ratio_for_round,
    score_keeper_state::{get_prop_power_key, get_total_round_power_key},
};

// The score keeper is a module that keeps track of amounts of individual validator shares, and power ratios (i.e. how many
// tokens a share of a validator represents). It stores the shares and power ratios for each validator in separate maps,
// and keeps those up-to-date with the total power (computed by multiplying the individual shares with the power ratio).
// The total is updated when either the shares or the power ratio of a validator is updated.

// Constants to define the suffixes for each map
const SHARES_PREFIX: &str = "shares_";
const POWER_PREFIX: &str = "power_";

// Function to create a storage key for the `shares` storage
fn shares_key(suffix: &str) -> String {
    format!("{}{}", SHARES_PREFIX, suffix)
}

// Function to create a storage key for the `shares` map
fn power_key(suffix: &str) -> String {
    format!("{}{}", POWER_PREFIX, suffix)
}

fn get_shares_map(suffix: &str) -> Map<&str, Decimal> {
    Map::new_dyn(shares_key(suffix))
}

fn get_power_item(suffix: &str) -> Item<Decimal> {
    Item::new_dyn(power_key(suffix))
}

pub fn get_shares(storage: &dyn Storage, key: &str, validator: String) -> StdResult<Decimal> {
    let shares_map = get_shares_map(key);
    Ok(shares_map
        .may_load(storage, &validator)?
        .unwrap_or_else(Decimal::zero))
}

pub fn get_total_power(storage: &dyn Storage, key: &str) -> StdResult<Decimal> {
    let total_power = get_power_item(key);
    Ok(total_power.may_load(storage)?.unwrap_or_else(Decimal::zero))
}

pub fn get_total_power_for_round(storage: &dyn Storage, round_id: u64) -> StdResult<Decimal> {
    let total_round_power_key = get_total_round_power_key(round_id);
    get_total_power(storage, total_round_power_key.as_str())
}

pub fn get_total_power_for_proposal(storage: &dyn Storage, prop_id: u64) -> StdResult<Decimal> {
    let prop_power_key = get_prop_power_key(prop_id);
    get_total_power(storage, prop_power_key.as_str())
}

// Initialize the maps for a given prefix
pub fn initialize_if_nil(storage: &mut dyn Storage, prefix: &str) -> StdResult<()> {
    let power_key = power_key(prefix);

    // Initialize the total power to 0
    let total_power: Item<Decimal> = Item::new_dyn(power_key);

    // Initialize if the total power has not been set
    if total_power.may_load(storage)?.is_none() {
        total_power.save(storage, &Decimal::zero())?;
    }

    // nothing has to be initialized for the shares and power ratio, since they are already a map

    Ok(())
}

// Add validator shares and update the total power
pub fn add_validator_shares(
    storage: &mut dyn Storage,
    key: &str,
    validator: String,
    num_shares: Decimal,
    power_ratio: Decimal,
) -> StdResult<()> {
    let shares_map: Map<&str, Decimal> = get_shares_map(key);
    let total_power: Item<Decimal> = get_power_item(key);

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
    let added_power = num_shares * power_ratio;

    current_power += added_power;
    total_power.save(storage, &current_power)?;

    Ok(())
}

pub fn add_validator_shares_to_round_total(
    storage: &mut dyn Storage,
    round_id: u64,
    validator: String,
    num_shares: Decimal,
) -> StdResult<()> {
    let power_ratio = get_validator_power_ratio_for_round(storage, round_id, validator.clone())?;
    let total_round_power_key = get_total_round_power_key(round_id);
    add_validator_shares(
        storage,
        total_round_power_key.as_str(),
        validator,
        num_shares,
        power_ratio,
    )
}

pub fn add_validator_shares_to_proposal(
    storage: &mut dyn Storage,
    round_id: u64,
    prop_id: u64,
    validator: String,
    num_shares: Decimal,
) -> StdResult<()> {
    let power_ratio = get_validator_power_ratio_for_round(storage, round_id, validator.clone())?;
    let prop_power_key = get_prop_power_key(prop_id);
    add_validator_shares(
        storage,
        prop_power_key.as_str(),
        validator,
        num_shares,
        power_ratio,
    )
}

// Remove validator shares and update the total power
pub fn remove_validator_shares(
    storage: &mut dyn Storage,
    key: &str,
    validator: String,
    num_shares: Decimal,
    power_ratio: Decimal,
) -> StdResult<()> {
    let shares_map = get_shares_map(key);
    let total_power = get_power_item(key);

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
    let removed_power = num_shares * power_ratio;
    current_power -= removed_power;
    total_power.save(storage, &current_power)?;

    Ok(())
}

pub fn remove_validator_shares_from_round_total(
    storage: &mut dyn Storage,
    round_id: u64,
    validator: String,
    num_shares: Decimal,
) -> StdResult<()> {
    let power_ratio = get_validator_power_ratio_for_round(storage, round_id, validator.clone())?;
    let total_round_power_key = get_total_round_power_key(round_id);
    remove_validator_shares(
        storage,
        total_round_power_key.as_str(),
        validator,
        num_shares,
        power_ratio,
    )
}

// Removes the given number of validator shares from the given proposal,
// which must be a proposal for the given round_id.
pub fn remove_validator_shares_from_proposal(
    storage: &mut dyn Storage,
    round_id: u64,
    prop_id: u64,
    validator: String,
    num_shares: Decimal,
) -> StdResult<()> {
    let power_ratio = get_validator_power_ratio_for_round(storage, round_id, validator.clone())?;
    let prop_power_key = get_prop_power_key(prop_id);
    remove_validator_shares(
        storage,
        prop_power_key.as_str(),
        validator,
        num_shares,
        power_ratio,
    )
}

// A more gas efficient version of remove_validator_shares_from_proposal
// when removing shares from multiple validators
pub fn remove_many_validator_shares_from_proposal(
    storage: &mut dyn Storage,
    round_id: u64,
    prop_id: u64,
    vals_and_shares: Vec<(String, Decimal)>,
) -> StdResult<()> {
    // do not reuse remove_validator_shares_from_proposal
    // instead, we will directly update the shares and power
    // to be more gas efficient
    let prop_power_key = get_prop_power_key(prop_id);
    let mut total_power = get_total_power(storage, prop_power_key.as_str())?;

    for (validator, num_shares) in vals_and_shares {
        let power_ratio =
            get_validator_power_ratio_for_round(storage, round_id, validator.clone())?;
        let shares_map = get_shares_map(prop_power_key.as_str());
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
        let removed_power = num_shares * power_ratio;
        total_power -= removed_power;
    }

    let total_power_item = get_power_item(prop_power_key.as_str());
    total_power_item.save(storage, &total_power)
}

// Update the power ratio for a validator and recomputes
// the total power for the given key
pub fn update_power_ratio(
    storage: &mut dyn Storage,
    key: &str,
    validator: String,
    old_power_ratio: Decimal,
    new_power_ratio: Decimal,
) -> StdResult<()> {
    let shares_map = get_shares_map(key);
    let total_power = get_power_item(key);

    // Initialize if needed
    let _ = initialize_if_nil(storage, key);

    // Load current shares
    let current_shares = shares_map
        .may_load(storage, &validator)?
        .unwrap_or_else(Decimal::zero);
    if current_shares == Decimal::zero() {
        return Ok(()); // No operation if the validator has no shares
    }

    // store the power from this validator before the update
    let old_power = current_shares * old_power_ratio;

    // Update the total power
    let mut current_power = total_power.load(storage)?;
    let new_power = current_shares * new_power_ratio;

    current_power = current_power - old_power + new_power;
    total_power.save(storage, &current_power)?;

    Ok(())
}

pub fn update_power_ratio_for_proposal(
    storage: &mut dyn Storage,
    prop_id: u64,
    validator: String,
    old_power_ratio: Decimal,
    new_power_ratio: Decimal,
) -> StdResult<()> {
    let prop_power_key = get_prop_power_key(prop_id);
    update_power_ratio(
        storage,
        prop_power_key.as_str(),
        validator,
        old_power_ratio,
        new_power_ratio,
    )
}

pub fn update_power_ratio_for_round_total(
    storage: &mut dyn Storage,
    round_id: u64,
    validator: String,
    old_power_ratio: Decimal,
    new_power_ratio: Decimal,
) -> StdResult<()> {
    let total_round_power_key = get_total_round_power_key(round_id);
    update_power_ratio(
        storage,
        total_round_power_key.as_str(),
        validator,
        old_power_ratio,
        new_power_ratio,
    )
}

#[cfg(test)]
mod tests {
    use crate::lsm_integration::set_new_validator_power_ratio_for_round;

    use super::*;
    use cosmwasm_std::testing::mock_dependencies;
    use cosmwasm_std::{Decimal, StdError, Storage};
    use proptest::prelude::*;

    // Utility function to initialize a mock storage
    fn initialize_storage() -> Box<dyn Storage> {
        Box::new(mock_dependencies().storage)
    }

    // Helper function to retrieve the shares and power values at once
    fn get_shares_and_power(
        storage: &dyn Storage,
        prefix: &str,
        validator: &str,
    ) -> (Decimal, Decimal) {
        (
            get_shares(storage, prefix, validator.to_string()).unwrap(),
            get_total_power(storage, prefix).unwrap(),
        )
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

        // add validator shares with the specified power ratio
        let result =
            add_validator_shares(storage, key, validator.to_string(), num_shares, power_ratio);
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

        // Add shares
        let _ = add_validator_shares(
            storage,
            key,
            validator.to_string(),
            num_shares,
            old_power_ratio,
        );

        // update the power ratio
        let res = update_power_ratio(
            storage,
            key,
            validator.to_string(),
            old_power_ratio,
            new_power_ratio,
        );

        assert!(res.is_ok());

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

            // set the power ratio
            let res = update_power_ratio(storage, key, validator.to_string(), power_ratio, power_ratio2);
            assert!(res.is_ok(), "Error updating power ratio: {:?}", res);

            // add the second shares
            let res = add_validator_shares(storage, key, validator.to_string(), num_shares2, power_ratio2);
            assert!(res.is_ok(), "Error adding validator shares: {:?}", res);
            let (shares, total_power) = get_shares_and_power(storage, key, validator);

            // Check if shares and power are correct after the second addition
            assert_eq!(shares, num_shares + num_shares2);
            assert_eq!(total_power, num_shares * power_ratio2 + num_shares2 * power_ratio2);

            // successively remove the shares
            let res = remove_validator_shares(storage, key, validator.to_string(), num_shares2, power_ratio2);
            assert!(res.is_ok(), "Error removing validator shares: {:?}", res);
            let (shares, total_power) = get_shares_and_power(storage, key, validator);

            // Check if shares and power are correct after removing
            assert_eq!(shares, num_shares);
            assert_eq!(total_power, num_shares * power_ratio2);

            // set the power ratio
            let res = update_power_ratio(storage, key, validator.to_string(), power_ratio2, power_ratio);
            assert!(res.is_ok(), "Error updating power ratio: {:?}", res);

            let (_, total_power) = get_shares_and_power(storage, key, validator);

            // Check that the power ratio is updated correctly
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

            // set the power ratio
            let res = update_power_ratio(storage, key, validator.to_string(), Decimal::zero(), old_power_ratio);
            assert!(res.is_ok(), "Error updating power ratio: {:?}", res);

            let res = add_validator_shares(storage, key, validator.to_string(), num_shares, old_power_ratio);
            assert!(res.is_ok(), "Error adding validator shares: {:?}", res);

            // set the power ratio
            let res = update_power_ratio(storage, key, validator.to_string(), old_power_ratio, new_power_ratio);
            assert!(res.is_ok(), "Error updating power ratio: {:?}", res);

            let (_, total_power) = get_shares_and_power(storage, key, validator);

            // Check if total power is updated correctly
            assert_eq!(total_power, num_shares * new_power_ratio);
        }
    }

    #[test]
    fn test_remove_many_validator_shares_from_proposal() {
        let mut deps = mock_dependencies();
        let storage = &mut deps.storage;
        let round_id = 1;
        let prop_id = 1;

        // Setup initial state
        let validator1 = "validator1".to_string();
        let validator2 = "validator2".to_string();
        let initial_shares1 = Decimal::percent(50);
        let initial_shares2 = Decimal::percent(30);
        let power_ratio1 = Decimal::percent(10);
        let power_ratio2 = Decimal::percent(20);

        // Mock the initial shares and power ratios
        let prop_power_key = get_prop_power_key(prop_id);
        let shares_map = get_shares_map(prop_power_key.as_str());
        shares_map
            .save(storage, &validator1, &initial_shares1)
            .unwrap();
        shares_map
            .save(storage, &validator2, &initial_shares2)
            .unwrap();
        set_new_validator_power_ratio_for_round(
            storage,
            round_id,
            validator1.clone(),
            power_ratio1,
        )
        .unwrap();
        set_new_validator_power_ratio_for_round(
            storage,
            round_id,
            validator2.clone(),
            power_ratio2,
        )
        .unwrap();

        // Mock the total power
        let total_power = Decimal::percent(100);
        let total_power_item = get_power_item(prop_power_key.as_str());
        total_power_item.save(storage, &total_power).unwrap();

        // Remove shares
        let vals_and_shares = vec![
            (validator1.clone(), Decimal::percent(10)),
            (validator2.clone(), Decimal::percent(10)),
        ];
        remove_many_validator_shares_from_proposal(storage, round_id, prop_id, vals_and_shares)
            .unwrap();

        // Check the updated shares
        let updated_shares1 = shares_map.load(storage, &validator1).unwrap();
        let updated_shares2 = shares_map.load(storage, &validator2).unwrap();
        assert_eq!(updated_shares1, Decimal::percent(40));
        assert_eq!(updated_shares2, Decimal::percent(20));

        // Check the updated total power
        let updated_total_power = total_power_item.load(storage).unwrap();
        let expected_total_power = total_power
            - (Decimal::percent(10) * power_ratio1)
            - (Decimal::percent(10) * power_ratio2);
        assert_eq!(updated_total_power, expected_total_power);
    }

    #[test]
    fn test_remove_many_validator_shares_from_proposal_insufficient_shares() {
        let mut deps = mock_dependencies();
        let storage = &mut deps.storage;
        let round_id = 1;
        let prop_id = 1;

        // Setup initial state
        let validator1 = "validator1".to_string();
        let initial_shares1 = Decimal::percent(5);
        let power_ratio1 = Decimal::percent(10);

        // Mock the initial shares and power ratios
        let prop_power_key = get_prop_power_key(prop_id);
        let shares_map = get_shares_map(prop_power_key.as_str());
        shares_map
            .save(storage, &validator1, &initial_shares1)
            .unwrap();
        set_new_validator_power_ratio_for_round(
            storage,
            round_id,
            validator1.clone(),
            power_ratio1,
        )
        .unwrap();

        // Mock the total power
        let total_power = Decimal::percent(100);
        let total_power_item = get_power_item(prop_power_key.as_str());
        total_power_item.save(storage, &total_power).unwrap();

        // Attempt to remove more shares than available
        let vals_and_shares = vec![(validator1.clone(), Decimal::percent(10))];
        let result =
            remove_many_validator_shares_from_proposal(storage, round_id, prop_id, vals_and_shares);

        // Check for error
        match result {
            Err(StdError::GenericErr { msg, .. }) => {
                assert_eq!(msg, "Insufficient shares for the validator")
            }
            _ => panic!("Expected error, but got {:?}", result),
        }
    }
}
