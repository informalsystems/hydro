use cosmwasm_std::{Decimal, SignedDecimal, StdError, StdResult, Storage};
use cw_storage_plus::Map;
use std::collections::HashMap;
use std::convert::TryFrom;

use crate::{
    lsm_integration::get_validator_power_ratio_for_round,
    state::{PROPOSAL_TOTAL_MAP, SCALED_PROPOSAL_SHARES_MAP},
};

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

pub fn remove_validator_shares_from_proposal(
    storage: &mut dyn Storage,
    round_id: u64,
    prop_id: u64,
    validator: String,
    num_shares: Decimal,
) -> StdResult<()> {
    let power_ratio = get_validator_power_ratio_for_round(storage, round_id, validator.clone())?;
    remove_validator_shares(
        storage,
        prop_id,
        SCALED_PROPOSAL_SHARES_MAP,
        PROPOSAL_TOTAL_MAP,
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

// Helper function to batch add validator shares to proposals
// This function takes a SignedDecimal in shares so it is possible to use it to "remove" shares
// The function remove_many_validator_shares_from_proposal should not be needed anymore.
// Note: The below allow is necesary to avoid clippy warning about redundant closure
// Otherwise, it throws another error: expected an `FnOnce()` closure, found `cosmwasm_std::Decimal`
// and suggests to wrap the Decimal in a closure with no arguments: `|| { /* code */ }`
#[allow(clippy::redundant_closure)]
pub fn add_many_validator_shares_to_proposal(
    storage: &mut dyn Storage,
    round_id: u64,
    proposal_id: u64,
    vals_and_shares: Vec<(String, SignedDecimal)>,
) -> StdResult<()> {
    let total_power_decimal = get_total_power_for_proposal(storage, proposal_id)?;

    // Convert total_power to SignedDecimal for computation
    let mut total_power = SignedDecimal::try_from(total_power_decimal).map_err(|_| {
        StdError::generic_err("Failed to convert Decimal to SignedDecimal for total_power")
    })?;

    for (validator, num_shares) in vals_and_shares {
        let power_ratio_decimal =
            get_validator_power_ratio_for_round(storage, round_id, validator.clone())?;

        let power_ratio = SignedDecimal::try_from(power_ratio_decimal).map_err(|_| {
            StdError::generic_err("Failed to convert Decimal to SignedDecimal for power_ratio")
        })?;

        let current_shares = SCALED_PROPOSAL_SHARES_MAP
            .may_load(storage, (proposal_id, validator.clone()))?
            .unwrap_or_else(|| Decimal::zero());

        // Convert current shares to SignedDecimal for computation
        let current_shares_signed = SignedDecimal::try_from(current_shares).map_err(|_| {
            StdError::generic_err("Failed to convert Decimal to SignedDecimal for current_shares")
        })?;

        // Compute the updated shares as SignedDecimal
        let updated_shares_signed = current_shares_signed + num_shares;

        // Ensure updated shares cannot go negative
        if updated_shares_signed.is_negative() {
            return Err(StdError::generic_err(
                "Shares for a validator cannot be negative",
            ));
        }

        // Convert updated shares back to Decimal for storage
        let updated_shares = Decimal::try_from(updated_shares_signed).map_err(|_| {
            StdError::generic_err("Failed to convert SignedDecimal to Decimal for updated_shares")
        })?;

        // Save updated shares
        SCALED_PROPOSAL_SHARES_MAP.save(
            storage,
            (proposal_id, validator.clone()),
            &updated_shares,
        )?;

        // Update the total power
        let delta_power = num_shares * power_ratio;
        let new_total_power = total_power + delta_power;

        // Ensure total power cannot be negative
        if new_total_power.is_negative() {
            return Err(StdError::generic_err("Total power cannot be negative"));
        }

        total_power = new_total_power;
    }

    // Convert total_power back to Decimal for storage
    let final_total_power = Decimal::try_from(total_power).map_err(|_| {
        StdError::generic_err("Failed to convert SignedDecimal to Decimal for total_power")
    })?;

    // Save updated total power
    PROPOSAL_TOTAL_MAP.save(storage, proposal_id, &final_total_power)
}

// Struct to track voting power changes for a proposal
#[derive(Debug, Default)]
pub struct ProposalPowerUpdate {
    pub validator_shares: HashMap<String, SignedDecimal>, // validator -> shares
}

// This function combines 2 proposal power updates into a single one.
// It sums the shares for each validator in the updates.
pub fn combine_proposal_power_updates(
    updates1: HashMap<u64, ProposalPowerUpdate>,
    updates2: HashMap<u64, ProposalPowerUpdate>,
) -> HashMap<u64, ProposalPowerUpdate> {
    let mut combined_updates = updates1;

    for (proposal_id, mut update2) in updates2 {
        // Clean up any zero entries in update2 before inserting
        update2
            .validator_shares
            .retain(|_, shares| !shares.is_zero());

        if update2.validator_shares.is_empty() {
            continue;
        }

        combined_updates
            .entry(proposal_id)
            .and_modify(|update1| {
                for (validator, shares) in update2.validator_shares.drain() {
                    update1
                        .validator_shares
                        .entry(validator)
                        .and_modify(|existing| {
                            *existing += shares;
                        })
                        .or_insert(shares);
                }

                // Remove any validator entries that sum to zero
                update1
                    .validator_shares
                    .retain(|_, shares| !shares.is_zero());
            })
            .or_insert(update2);
    }

    // Remove any proposals that end up with no validator shares
    combined_updates.retain(|_, update| !update.validator_shares.is_empty());

    combined_updates
}

// This function applies the changes in the proposal power updates to the storage.
// As it is basically a wrapper around add_many_validator_shares_to_proposal function,
//  it will update SCALED_PROPOSAL_SHARES_MAP and TOTAL_PROPOSAL_MAP.
pub fn apply_proposal_changes(
    storage: &mut dyn Storage,
    round_id: u64,
    changes: HashMap<u64, ProposalPowerUpdate>,
) -> StdResult<()> {
    for (proposal_id, changes) in changes {
        let vals_and_shares: Vec<(String, SignedDecimal)> = changes
            .validator_shares
            .into_iter()
            .filter(|(_, shares)| !shares.is_zero())
            .collect();

        if !vals_and_shares.is_empty() {
            add_many_validator_shares_to_proposal(storage, round_id, proposal_id, vals_and_shares)?;
        }
    }

    Ok(())
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
    use crate::testing_lsm_integration::set_validator_power_ratio;
    use crate::testing_mocks::no_op_grpc_query_mock;
    use crate::{lsm_integration::get_total_power_for_round, testing_mocks::mock_dependencies};

    use super::*;
    use cosmwasm_std::{Decimal, StdError, Storage};
    use proptest::prelude::*;
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
        let deps = mock_dependencies(no_op_grpc_query_mock());
        let index_key = 5;

        let total_power = get_total_power_for_round(&deps.as_ref(), index_key).unwrap();
        assert_eq!(total_power, Decimal::zero());

        let total_power = get_total_power_for_proposal(deps.as_ref().storage, index_key).unwrap();
        assert_eq!(total_power, Decimal::zero());
    }

    #[test]
    fn test_add_validator_shares() {
        let mut deps = mock_dependencies(no_op_grpc_query_mock());
        let storage = deps.as_mut().storage;

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
        let mut deps = mock_dependencies(no_op_grpc_query_mock());
        let storage = deps.as_mut().storage;

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
        let mut deps = mock_dependencies(no_op_grpc_query_mock());
        let storage = deps.as_mut().storage;

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
        let mut deps = mock_dependencies(no_op_grpc_query_mock());
        let storage = deps.as_mut().storage;

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
            let mut deps = mock_dependencies(no_op_grpc_query_mock());
        let storage = deps.as_mut().storage;

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
            let mut deps = mock_dependencies(no_op_grpc_query_mock());
            let storage = deps.as_mut().storage;

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
        let mut deps = mock_dependencies(no_op_grpc_query_mock());
        let storage = deps.as_mut().storage;
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
        let mut deps = mock_dependencies(no_op_grpc_query_mock());
        let storage = deps.as_mut().storage;
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
