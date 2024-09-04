use cosmwasm_std::{Decimal, StdError, StdResult, Storage};
use cw_storage_plus::Map;

use crate::lsm_integration::get_validator_power_ratio_for_round;

// The score keeper is a module that keeps track of amounts of individual validator shares, and power ratios (i.e. how many
// tokens a share of a validator represents). It stores the shares and power ratios for each validator in separate maps,
// and keeps those up-to-date with the total power (computed by multiplying the individual shares with the power ratio).
// The total is updated when either the shares or the power ratio of a validator is updated.

// SCALED_PROPOSAL_SHARES_MAP: key(proposal_id, validator_address) -> number_of_shares
const SCALED_PROPOSAL_SHARES_MAP: Map<(u64, String), Decimal> =
    Map::new("scaled_proposal_power_shares");

// PROPOSAL_TOTAL_MAP: key(proposal_id) -> total_power
const PROPOSAL_TOTAL_MAP: Map<u64, Decimal> = Map::new("proposal_power_total");

pub fn get_total_power_for_proposal(storage: &dyn Storage, prop_id: u64) -> StdResult<Decimal> {
    Ok(PROPOSAL_TOTAL_MAP
        .may_load(storage, prop_id)?
        .unwrap_or(Decimal::zero()))
}

pub fn get_validator_shares_for_proposal(
    storage: &dyn Storage,
    prop_id: u64,
    validator: String,
) -> StdResult<Decimal> {
    Ok(SCALED_PROPOSAL_SHARES_MAP
        .may_load(storage, (prop_id, validator))?
        .unwrap_or(Decimal::zero()))
}

// Add validator shares and update the total power
pub fn add_validator_shares(
    storage: &mut dyn Storage,
    index_key: u64,
    shares_map: Map<(u64, String), Decimal>,
    total_map: Map<u64, Decimal>,
    validator: String,
    num_shares: Decimal,
    power_ratio: Decimal,
) -> StdResult<()> {
    let key = (index_key, validator.clone());

    // Update the shares map
    let current_shares = shares_map
        .may_load(storage, key.clone())?
        .unwrap_or_else(Decimal::zero);
    let updated_shares = current_shares + num_shares;
    shares_map.save(storage, key, &updated_shares)?;

    // Update the total power
    let mut current_power = total_map
        .load(storage, index_key)
        .unwrap_or(Decimal::zero());
    let added_power = num_shares * power_ratio;

    current_power += added_power;
    total_map.save(storage, index_key, &current_power)?;

    Ok(())
}

pub fn add_validator_shares_to_proposal(
    storage: &mut dyn Storage,
    round_id: u64,
    prop_id: u64,
    validator: String,
    num_shares: Decimal,
) -> StdResult<()> {
    let power_ratio = get_validator_power_ratio_for_round(storage, round_id, validator.clone())?;
    add_validator_shares(
        storage,
        prop_id,
        SCALED_PROPOSAL_SHARES_MAP,
        PROPOSAL_TOTAL_MAP,
        validator,
        num_shares,
        power_ratio,
    )
}

// Remove validator shares and update the total power
pub fn remove_validator_shares(
    storage: &mut dyn Storage,
    index_key: u64,
    shares_map: Map<(u64, String), Decimal>,
    total_map: Map<u64, Decimal>,
    validator: String,
    num_shares: Decimal,
    power_ratio: Decimal,
) -> StdResult<()> {
    let key = (index_key, validator.clone());

    // Load current shares
    let current_shares = shares_map
        .may_load(storage, key.clone())?
        .unwrap_or_else(Decimal::zero);

    // Ensure the validator has enough shares
    if current_shares < num_shares {
        return Err(StdError::generic_err(
            "Insufficient shares for the validator",
        ));
    }

    // Update the shares map
    let updated_shares = current_shares - num_shares;
    shares_map.save(storage, key, &updated_shares)?;

    // Update the total power
    let mut current_power = total_map.load(storage, index_key)?;
    let removed_power = num_shares * power_ratio;
    current_power -= removed_power;
    total_map.save(storage, index_key, &current_power)?;

    Ok(())
}

// A more gas efficient version of remove_validator_shares_from_proposal
// when removing shares from multiple validators
pub fn remove_many_validator_shares_from_proposal(
    storage: &mut dyn Storage,
    round_id: u64,
    prop_id: u64,
    vals_and_shares: Vec<(String, Decimal)>,
) -> StdResult<()> {
    let mut total_power = get_total_power_for_proposal(storage, prop_id)?;

    // do not reuse remove_validator_shares_from_proposal
    // instead, we will directly update the shares and power
    // to be more gas efficient
    for (validator, num_shares) in vals_and_shares {
        let power_ratio =
            get_validator_power_ratio_for_round(storage, round_id, validator.clone())?;
        let current_shares = SCALED_PROPOSAL_SHARES_MAP
            .may_load(storage, (prop_id, validator.clone()))?
            .unwrap_or_else(Decimal::zero);

        // Ensure the validator has enough shares
        if current_shares < num_shares {
            return Err(StdError::generic_err(
                "Insufficient shares for the validator",
            ));
        }

        // Update the shares map
        let updated_shares = current_shares - num_shares;
        SCALED_PROPOSAL_SHARES_MAP.save(storage, (prop_id, validator), &updated_shares)?;

        // Update the total power
        let removed_power = num_shares * power_ratio;

        if total_power < removed_power {
            return Err(StdError::generic_err("Insufficient total power"));
        }

        total_power -= removed_power;
    }

    PROPOSAL_TOTAL_MAP.save(storage, prop_id, &total_power)
}

// Update the power ratio for a validator and recomputes
// the total power for the given key
pub fn update_power_ratio(
    storage: &mut dyn Storage,
    index_key: u64,
    shares_map: Map<(u64, String), Decimal>,
    total_map: Map<u64, Decimal>,
    validator: String,
    old_power_ratio: Decimal,
    new_power_ratio: Decimal,
) -> StdResult<()> {
    // Load current shares
    let current_shares = shares_map
        .may_load(storage, (index_key, validator))?
        .unwrap_or_else(Decimal::zero);
    if current_shares == Decimal::zero() {
        return Ok(()); // No operation if the validator has no shares
    }

    // store the power from this validator before the update
    let old_power = current_shares * old_power_ratio;

    // Update the total power
    let mut current_power = total_map
        .load(storage, index_key)
        .unwrap_or(Decimal::zero());
    let new_power = current_shares * new_power_ratio;

    current_power = current_power - old_power + new_power;
    total_map.save(storage, index_key, &current_power)?;

    Ok(())
}

pub fn update_power_ratio_for_proposal(
    storage: &mut dyn Storage,
    prop_id: u64,
    validator: String,
    old_power_ratio: Decimal,
    new_power_ratio: Decimal,
) -> StdResult<()> {
    update_power_ratio(
        storage,
        prop_id,
        SCALED_PROPOSAL_SHARES_MAP,
        PROPOSAL_TOTAL_MAP,
        validator,
        old_power_ratio,
        new_power_ratio,
    )
}

#[cfg(test)]
mod tests {
    use crate::lsm_integration::get_total_power_for_round;
    use crate::testing_lsm_integration::set_validator_power_ratio;

    use super::*;
    use cosmwasm_std::testing::mock_dependencies;
    use cosmwasm_std::{Decimal, StdError, Storage};
    use proptest::prelude::*;

    // Utility function to initialize a mock storage
    fn initialize_storage() -> Box<dyn Storage> {
        Box::new(mock_dependencies().storage)
    }

    fn get_shares_and_power_for_proposal(
        storage: &dyn Storage,
        prop_id: u64,
        validator: String,
    ) -> (Decimal, Decimal) {
        let shares = get_validator_shares_for_proposal(storage, prop_id, validator).unwrap();
        let total_power = get_total_power_for_proposal(storage, prop_id).unwrap();
        (shares, total_power)
    }

    // Table-based tests
    #[test]
    fn test_uninitialized() {
        let mut binding = initialize_storage();
        let storage = binding.as_mut();
        let index_key = 5;

        let total_power = get_total_power_for_round(storage, index_key).unwrap();
        assert_eq!(total_power, Decimal::zero());

        let total_power = get_total_power_for_proposal(storage, index_key).unwrap();
        assert_eq!(total_power, Decimal::zero());
    }

    #[test]
    fn test_add_validator_shares() {
        let mut binding = initialize_storage();
        let storage = binding.as_mut();

        let index_key = 5;
        let validator = "validator1";
        let num_shares = Decimal::from_ratio(100u128, 1u128);
        let power_ratio = Decimal::from_ratio(2u128, 1u128);

        // add validator shares with the specified power ratio
        let result = add_validator_shares(
            storage,
            index_key,
            SCALED_PROPOSAL_SHARES_MAP,
            PROPOSAL_TOTAL_MAP,
            validator.to_string(),
            num_shares,
            power_ratio,
        );
        assert!(result.is_ok());

        let (shares, total_power) =
            get_shares_and_power_for_proposal(storage, index_key, validator.to_string());
        assert_eq!(shares, num_shares);
        assert_eq!(total_power, num_shares * power_ratio);
    }

    #[test]
    fn test_remove_validator_shares() {
        let mut binding = initialize_storage();
        let storage = binding.as_mut();

        let index_key = 5;
        let validator = "validator1";
        let num_shares = Decimal::from_ratio(100u128, 1u128);
        let power_ratio = Decimal::from_ratio(2u128, 1u128);

        // Add shares first
        let _ = add_validator_shares(
            storage,
            index_key,
            SCALED_PROPOSAL_SHARES_MAP,
            PROPOSAL_TOTAL_MAP,
            validator.to_string(),
            num_shares,
            power_ratio,
        );

        // Now remove shares
        let result = remove_validator_shares(
            storage,
            index_key,
            SCALED_PROPOSAL_SHARES_MAP,
            PROPOSAL_TOTAL_MAP,
            validator.to_string(),
            num_shares,
            power_ratio,
        );
        assert!(result.is_ok());

        let (shares, total_power) =
            get_shares_and_power_for_proposal(storage, index_key, validator.to_string());
        assert_eq!(shares, Decimal::zero());
        assert_eq!(total_power, Decimal::zero());
    }

    #[test]
    fn test_remove_validator_shares_insufficient_shares() {
        let mut binding = initialize_storage();
        let storage = binding.as_mut();

        let index_key = 10;
        let validator = "validator1";
        let num_shares = Decimal::from_ratio(100u128, 1u128);
        let power_ratio = Decimal::from_ratio(2u128, 1u128);

        // Add a smaller amount of shares
        let _ = add_validator_shares(
            storage,
            index_key,
            SCALED_PROPOSAL_SHARES_MAP,
            PROPOSAL_TOTAL_MAP,
            validator.to_string(),
            Decimal::from_ratio(50u128, 1u128),
            power_ratio,
        );

        // Attempt to remove more shares than exist
        let result = remove_validator_shares(
            storage,
            index_key,
            SCALED_PROPOSAL_SHARES_MAP,
            PROPOSAL_TOTAL_MAP,
            validator.to_string(),
            num_shares,
            power_ratio,
        );
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

        let key = 5;
        let validator = "validator1";
        let num_shares = Decimal::from_ratio(100u128, 1u128);
        let old_power_ratio = Decimal::from_ratio(2u128, 1u128);
        let new_power_ratio = Decimal::from_ratio(3u128, 1u128);

        // Add shares
        let _ = add_validator_shares(
            storage,
            key,
            SCALED_PROPOSAL_SHARES_MAP,
            PROPOSAL_TOTAL_MAP,
            validator.to_string(),
            num_shares,
            old_power_ratio,
        );

        // update the power ratio
        let res = update_power_ratio(
            storage,
            key,
            SCALED_PROPOSAL_SHARES_MAP,
            PROPOSAL_TOTAL_MAP,
            validator.to_string(),
            old_power_ratio,
            new_power_ratio,
        );

        assert!(res.is_ok());

        let (_, total_power) =
            get_shares_and_power_for_proposal(storage, key, validator.to_string());
        assert_eq!(total_power, num_shares * new_power_ratio);
    }

    // Property-based tests using proptest
    proptest! {
        #[test]
        fn proptest_multi_add_remove_validator_shares(num_shares in 1u128..1_000_000u128, num_shares2 in 1u128..1_000_000u128, power_ratio in 1u128..1_000u128, power_ratio2 in 1u128..1_000u128) {
            let mut binding = initialize_storage();
            let storage = binding.as_mut();

            let key = 5;
            let validator = "validator1";
            let num_shares = Decimal::from_ratio(num_shares, 1u128);
            let power_ratio = Decimal::from_ratio(power_ratio, 1u128);
            let num_shares2 = Decimal::from_ratio(num_shares2, 1u128);
            let power_ratio2 = Decimal::from_ratio(power_ratio2, 1u128);

            let res = add_validator_shares(storage,
                key,
                SCALED_PROPOSAL_SHARES_MAP,
                PROPOSAL_TOTAL_MAP,
                validator.to_string(), num_shares, power_ratio);
            assert!(res.is_ok(), "Error adding validator shares: {:?}", res);
            let (shares, total_power) = get_shares_and_power_for_proposal(storage, key, validator.to_string());

            // Check if shares and power are correct after adding
            assert_eq!(shares, num_shares);
            assert_eq!(total_power, num_shares * power_ratio);

            // set the power ratio
            let res = update_power_ratio(
                storage,
                key,
                SCALED_PROPOSAL_SHARES_MAP,
                PROPOSAL_TOTAL_MAP,
                validator.to_string(), power_ratio, power_ratio2);
            assert!(res.is_ok(), "Error updating power ratio: {:?}", res);

            // add the second shares
            let res = add_validator_shares(
                storage,
                key,
                SCALED_PROPOSAL_SHARES_MAP,
                PROPOSAL_TOTAL_MAP,
                validator.to_string(), num_shares2, power_ratio2);
            assert!(res.is_ok(), "Error adding validator shares: {:?}", res);
            let (shares, total_power) = get_shares_and_power_for_proposal(storage, key, validator.to_string());

            // Check if shares and power are correct after the second addition
            assert_eq!(shares, num_shares + num_shares2);
            assert_eq!(total_power, num_shares * power_ratio2 + num_shares2 * power_ratio2);

            // successively remove the shares
            let res = remove_validator_shares(storage, key,
                SCALED_PROPOSAL_SHARES_MAP,
                PROPOSAL_TOTAL_MAP,
                validator.to_string(), num_shares2, power_ratio2);
            assert!(res.is_ok(), "Error removing validator shares: {:?}", res);
            let (shares, total_power) = get_shares_and_power_for_proposal(storage, key, validator.to_string());

            // Check if shares and power are correct after removing
            assert_eq!(shares, num_shares);
            assert_eq!(total_power, num_shares * power_ratio2);

            // set the power ratio
            let res = update_power_ratio(storage, key,
                SCALED_PROPOSAL_SHARES_MAP,
                PROPOSAL_TOTAL_MAP,
                validator.to_string(), power_ratio2, power_ratio);
            assert!(res.is_ok(), "Error updating power ratio: {:?}", res);

            let (_, total_power) = get_shares_and_power_for_proposal(storage, key, validator.to_string());

            // Check that the power ratio is updated correctly
            assert_eq!(total_power, num_shares * power_ratio);

            let res = remove_validator_shares(storage, key,
                SCALED_PROPOSAL_SHARES_MAP,
                PROPOSAL_TOTAL_MAP,
                validator.to_string(), num_shares, power_ratio);
            assert!(res.is_ok(), "Error removing validator shares: {:?}", res);
            let (shares, total_power) = get_shares_and_power_for_proposal(storage, key, validator.to_string());

            // Check if shares and power are zero after removing the second batch of shares
            assert_eq!(shares, Decimal::zero());
            assert_eq!(total_power, Decimal::zero());
        }

        #[test]
        fn proptest_update_power_ratio(num_shares in 1u128..1_000_000u128, old_power_ratio in 1u128..1_000u128, new_power_ratio in 1u128..1_000u128) {
            let mut binding = initialize_storage();
        let storage = binding.as_mut();

            let key = 8;
            let validator = "validator1";
            let num_shares = Decimal::from_ratio(num_shares, 1u128);
            let old_power_ratio = Decimal::from_ratio(old_power_ratio, 1u128);
            let new_power_ratio = Decimal::from_ratio(new_power_ratio, 1u128);

            // set the power ratio
            let res = update_power_ratio(storage, key,
                SCALED_PROPOSAL_SHARES_MAP,
                PROPOSAL_TOTAL_MAP,
                validator.to_string(), Decimal::zero(), old_power_ratio);
            assert!(res.is_ok(), "Error updating power ratio: {:?}", res);

            let res = add_validator_shares(storage, key,
                SCALED_PROPOSAL_SHARES_MAP,
                PROPOSAL_TOTAL_MAP,
                validator.to_string(), num_shares, old_power_ratio);
            assert!(res.is_ok(), "Error adding validator shares: {:?}", res);

            // set the power ratio
            let res = update_power_ratio(storage, key,
                SCALED_PROPOSAL_SHARES_MAP,
                PROPOSAL_TOTAL_MAP,
                validator.to_string(), old_power_ratio, new_power_ratio);
            assert!(res.is_ok(), "Error updating power ratio: {:?}", res);

            let (_, total_power) = get_shares_and_power_for_proposal(storage, key, validator.to_string());

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
        SCALED_PROPOSAL_SHARES_MAP
            .save(storage, (prop_id, validator1.clone()), &initial_shares1)
            .unwrap();
        SCALED_PROPOSAL_SHARES_MAP
            .save(storage, (prop_id, validator2.clone()), &initial_shares2)
            .unwrap();
        set_validator_power_ratio(storage, round_id, validator1.as_str(), power_ratio1);
        set_validator_power_ratio(storage, round_id, validator2.as_str(), power_ratio2);

        // Mock the total power
        let total_power = Decimal::percent(100);
        PROPOSAL_TOTAL_MAP
            .save(storage, prop_id, &total_power)
            .unwrap();

        // Remove shares
        let vals_and_shares = vec![
            (validator1.clone(), Decimal::percent(10)),
            (validator2.clone(), Decimal::percent(10)),
        ];
        remove_many_validator_shares_from_proposal(storage, round_id, prop_id, vals_and_shares)
            .unwrap();

        // Check the updated shares
        let updated_shares1 = SCALED_PROPOSAL_SHARES_MAP
            .load(storage, (prop_id, validator1))
            .unwrap();
        let updated_shares2 = SCALED_PROPOSAL_SHARES_MAP
            .load(storage, (prop_id, validator2))
            .unwrap();
        assert_eq!(updated_shares1, Decimal::percent(40));
        assert_eq!(updated_shares2, Decimal::percent(20));

        // Check the updated total power
        let updated_total_power = PROPOSAL_TOTAL_MAP.load(storage, prop_id).unwrap();
        let expected_total_power: Decimal = total_power
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
        SCALED_PROPOSAL_SHARES_MAP
            .save(storage, (prop_id, validator1.clone()), &initial_shares1)
            .unwrap();
        set_validator_power_ratio(storage, round_id, validator1.as_str(), power_ratio1);

        // Mock the total power
        let total_power = Decimal::percent(100);
        PROPOSAL_TOTAL_MAP
            .save(storage, prop_id, &total_power)
            .unwrap();

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
