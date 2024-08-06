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
    for query_round_id in (0..=round_id).rev() {
        let validators = VALIDATORS_PER_ROUND.load(deps.storage, query_round_id)?;
        if validators.len() > 0 {
            return Ok(validators);
        }
    }
    return Err(StdError::generic_err(format!(
        "Validators have not been set for round {} or any previous round",
        round_id
    )));
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
pub fn validate_denom(
    deps: Deps,
    env: Env,
    denom: String,
    max_validators: u64,
) -> StdResult<String> {
    let validator = get_validator_from_denom(denom)?;

    let constants = crate::state::CONSTANTS.load(deps.storage)?;
    let round_id = compute_current_round_id(&env, &constants)?;
    let validators = get_validators_for_round(deps, round_id)?;
    if validators.contains(&validator) {
        Ok(validator)
    } else {
        Err(StdError::generic_err(format!(
            "Validator {} is not among the top {} validators",
            validator, max_validators
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
