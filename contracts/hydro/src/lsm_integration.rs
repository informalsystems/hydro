use cosmwasm_std::{Decimal, Deps, DepsMut, Env, StdError, StdResult, Uint128, Validator};

use crate::{
    contract::compute_current_round_id,
    state::{LOCKED_TOKENS, VALIDATORS_PER_ROUND, VALIDATOR_SHARE_TO_TOKEN_RATIO},
};

// Returns the validators from the store for the round.
// If the validators have not been set for the round
// (presumably because the round has not gone for long enough for them to be updated)
// it will fall back to getting the validators for previous rounds, since
// these are then the most up-to-date information we have.
// If no validators have been set for any previous round, it will return an error.
pub fn get_validators_for_round(deps: Deps, round_id: u64) -> StdResult<Vec<String>> {
    // TODO: error when the round id is in the future
    for query_round_id in (0..=round_id).rev() {
        let validators_res = VALIDATORS_PER_ROUND.load(deps.storage, query_round_id);

        if validators_res.is_ok() {
            return validators_res;
        } else {
            // if we have an error, it is because the validators have not been set for this round
            // we can ignore this error and continue to the next round

            // log the error for debugging
            deps.api.debug(&format!(
                "Did not find validators for round {}, trying previous rounds; error was: {}",
                query_round_id,
                validators_res.unwrap_err()
            ));

            continue;
        }
    }
    return Err(StdError::generic_err(format!(
        "Validators have not been set for round {} or any previous round",
        round_id
    )));
}

// Get the power ratio of a given validator for a given round.
// If the power ratio has not been set for the round,
// it will try to get the power ratio for previous rounds, until it finds a round that
// has the power ratio set.
// If no power ratio has been set for any previous round, it will return an error.
// pub fn get_validator_power_ratio_for_round(
//     deps: Deps,
//     round_id: u64,
//     validator: String,
// ) -> StdResult<Vec<(String, Decimal)>> {
//     // TODO: error when the round_id is in the future

//     // checks if the power ratio is not set for this round
//     // note: this needs to check whether any ratios have been set for this round,
//     // not just whether the ratio for this validator has been set,
//     //
// }

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

// Updates the weight of the shares of a validator, for the current round.
// This is the amount of staked tokens that each share of this validator represents.
pub fn update_share_weight(
    deps: DepsMut,
    env: Env,
    val: String,
    new_weight: Decimal,
) -> StdResult<()> {
    let constants = crate::state::CONSTANTS.load(deps.storage)?;
    let round_id = compute_current_round_id(&env, &constants)?;
    VALIDATOR_SHARE_TO_TOKEN_RATIO.save(deps.storage, (round_id, val), &new_weight)
}
