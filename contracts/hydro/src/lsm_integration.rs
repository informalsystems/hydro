use cosmwasm_std::{Deps, DepsMut, Env, StdError, StdResult};
use cw_storage_plus::Map;

use crate::contract::compute_current_round_id;

// For each round, stores the list of validators whose shares are eligible to vote.
// We only store the top MAX_VALIDATOR_SHARES_PARTICIPATING validators by delegated tokens,
// to avoid DoS attacks where someone creates a large number of validators with very small amounts of shares.
// VALIDATORS_PER_ROUND: key(round_id) -> Vec<validator_address>
pub const VALIDATORS_PER_ROUND: Map<u64, Vec<String>> = Map::new("validators_per_round");

// Returns the validators from the store for the round.
// If the validators have not been set for the round
// (presumably because the round has not gone for long enough for them to be updated)
// it will fall back to getting the validators for the previous round.
// If those are also not set, it will return an error.
pub fn get_validators_for_round(deps: Deps, round_id: u64) -> StdResult<Vec<String>> {
    // try to get the validators for the round id
    let validators = VALIDATORS_PER_ROUND.may_load(deps.storage, round_id)?;

    // if the validators are not set for this round, try to get the validators for the previous round
    let validators = match validators {
        Some(validators) => validators,
        None => {
            // if the round id is 0, we can't get the validators for the previous round
            if round_id == 0 {
                return Err(StdError::generic_err("Validators are not set"));
            }

            // get the validators for the previous round
            let validators = VALIDATORS_PER_ROUND
                .load(deps.storage, round_id - 1)
                .map_err(|_| {
                    StdError::generic_err(format!(
                        "Failed to load validators for rounds {} and {}",
                        round_id,
                        round_id - 1
                    ))
                })?;
            validators
        }
    };

    Ok(validators)
}

// Sets the validators for the current round.
// This can be called multiple times in a round, and will overwrite the previous validators
// for this round.
pub fn set_current_validators(deps: DepsMut, env: Env, validators: Vec<String>) -> StdResult<()> {
    let round_id = compute_current_round_id(&env, &crate::state::CONSTANTS.load(deps.storage)?)?;
    VALIDATORS_PER_ROUND.save(deps.storage, round_id, &validators)?;
    Ok(())
}

// Returns the validator that this denom
// represents tokenized shares from.
// Returns an error if the denom is not
// an LSM tokenized share.
pub fn get_validator_from_denom(denom: String) -> StdResult<String> {
    // if the denom is an LSM tokenized share, its name is of the form
    // validatoraddress/record_id

    // resolve the denom
    let parts: Vec<&str> = denom.split('/').collect();
    if parts.len() != 2 {
        return Err(StdError::generic_err(format!(
            "Invalid denom {}: not an LSM tokenized share",
            denom
        )));
    }

    // return the validator address
    Ok(parts[0].to_string())
}

// Returns OK if the denom is a valid LSM tokenized share
// of a validator that is also currently among the top
// max_validators validators, and returns the address of that validator.
pub fn validate_denom(deps: Deps, env: Env, denom: String) -> StdResult<String> {
    let validator = get_validator_from_denom(denom)?;

    let constants = crate::state::CONSTANTS.load(deps.storage)?;
    let round_id = compute_current_round_id(&env, &constants)?;
    let max_validators = constants.max_validator_shares_participating;

    let validators = get_validators_for_round(deps, round_id)?;
    if validators.contains(&validator) {
        Ok(validator)
    } else {
        Err(StdError::generic_err(format!(
            "Validator {} is not present; possibly they are not part of the top {} validators by delegated tokens",
            validator,
            max_validators
        )))
    }
}

#[cfg(test)]
mod tests {
    use cosmwasm_std::{
        testing::{mock_dependencies, mock_env},
        Storage, Timestamp,
    };

    use crate::testing::ONE_DAY_IN_NANO_SECONDS;

    use super::*;

    fn get_default_constants() -> crate::state::Constants {
        crate::state::Constants {
            round_length: ONE_DAY_IN_NANO_SECONDS,
            lock_epoch_length: 1,
            first_round_start: Timestamp::from_seconds(0),
            max_locked_tokens: 1,
            paused: false,
            max_validator_shares_participating: 2,
        }
    }

    #[test]
    fn test_validate_denom() {
        struct TestCase {
            denom: String,
            expected_result: Result<String, StdError>,
            setup: Box<dyn Fn(&mut dyn Storage, &mut Env)>,
        }

        let test_cases = vec![
            TestCase {
                denom: "invalid_denom".to_string(),
                expected_result: Err(StdError::generic_err(
                    "Invalid denom invalid_denom: not an LSM tokenized share",
                )),
                setup: Box::new(|storage, env| {
                    let round_id = 0;
                    VALIDATORS_PER_ROUND
                        .save(
                            storage,
                            round_id,
                            &vec!["validator2".to_string(), "validator3".to_string()],
                        )
                        .unwrap();
                }),
            },
            TestCase {
                denom: "validator1/record_id".to_string(),
                expected_result: Err(StdError::generic_err("Validator validator1 is not present; possibly they are not part of the top 2 validators by delegated tokens")),
                setup: Box::new(|storage, env| {
                    let round_id = 0;
                    VALIDATORS_PER_ROUND.save(storage, round_id, &vec!["validator2".to_string(), "validator3".to_string()]).unwrap();
                }),
            },
            TestCase {
                denom: "validator1/record_id".to_string(),
                expected_result: Err(StdError::generic_err("Validator validator1 is not present; possibly they are not part of the top 2 validators by delegated tokens")),
                setup: Box::new(|storage, env| {
                    let round_id = 1;
                    VALIDATORS_PER_ROUND.save(storage, round_id - 1, &vec!["validator1".to_string(), "validator2".to_string()]).unwrap();
                    VALIDATORS_PER_ROUND.save(storage, round_id, &vec!["validator2".to_string(), "validator3".to_string()]).unwrap();

                    env.block.time = Timestamp::from_nanos(ONE_DAY_IN_NANO_SECONDS+1);
                }),
            },
            TestCase {
                denom: "validator1/record_id".to_string(),
                expected_result: Ok("validator1".to_string()),
                setup: Box::new(|storage, env| {
                    let constants = get_default_constants();
                    crate::state::CONSTANTS.save(storage, &constants).unwrap();
                    let round_id = 0;
                    VALIDATORS_PER_ROUND
                        .save(
                            storage,
                            round_id,
                            &vec!["validator1".to_string(), "validator2".to_string()],
                        )
                        .unwrap();
                }),
            },
        ];

        for (i, test_case) in test_cases.into_iter().enumerate() {
            let mut deps = mock_dependencies();
            let mut env = mock_env();

            let constants = get_default_constants();
            crate::state::CONSTANTS
                .save(&mut deps.storage, &constants)
                .unwrap();

            env.block.time = Timestamp::from_seconds(0);

            (test_case.setup)(&mut deps.storage, &mut env);

            let result = validate_denom(deps.as_ref(), env.clone(), test_case.denom.clone());

            assert_eq!(
                result, test_case.expected_result,
                "Test case {} failed: expected {:?}, got {:?}",
                i, test_case.expected_result, result
            );
        }
    }
}
