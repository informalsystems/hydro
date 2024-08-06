use cosmwasm_std::{Decimal, Deps, DepsMut, Env, StdError, StdResult, Uint128, Validator};

use crate::{
    contract::compute_current_round_id,
    state::{LOCKED_VALIDATOR_SHARES, VALIDATOR_SHARE_TO_TOKEN_RATIO},
};

// Returns the max_validators with the largest voting power.
pub fn get_validators(deps: Deps, max_validators: u64) -> Vec<String> {
    // TODO: return the max_validators with the largest voting power
    vec!["validator1".to_string(), "validator2".to_string()]
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
pub fn validate_denom(deps: Deps, denom: String, max_validators: u64) -> StdResult<String> {
    let validator = get_validator_from_denom(denom)?;
    let validators = get_validators(deps, max_validators);
    if validators.contains(&validator) {
        Ok(validator)
    } else {
        Err(StdError::generic_err(format!(
            "Validator {} is not among the top {} validators",
            validator, max_validators
        )))
    }
}

// Returns the amount of voting-power equivalent
// of tokens that was locked in the contract at the end of the given round_id.
// This only counts tokenized shares corresponding
// to the top max_validators validators.
// It also normalizes the shares to the amount of
// staked tokens that they represent.
// If the given round_id is in the future, this will return an error.
pub fn compute_total_locked_tokens_helper(
    deps: Deps,
    env: Env,
    round_id: u64,
    max_validators: u64,
) -> StdResult<u128> {
    // get the top max_validators validators
    let validators = get_validators(deps, max_validators);

    let constants = crate::state::CONSTANTS.load(deps.storage)?;
    let current_round_id = compute_current_round_id(&env, &constants)?;

    if round_id > current_round_id {
        return Err(StdError::generic_err(format!(
            "Round {} is in the future",
            round_id
        )));
    }

    // sum the locked tokens of the top max_validators validators
    let mut total_locked_tokens = 0;
    for validator in validators {
        let locked_shares = LOCKED_VALIDATOR_SHARES
            .load(deps.storage, validator.clone())
            .unwrap_or(0); // get the amount of tokens locked for that validator, or 0 if none are locked

        let ratio = VALIDATOR_SHARE_TO_TOKEN_RATIO
            .load(deps.storage, (round_id, validator))
            .unwrap(); // get the ratio of shares to staked tokens for that validator
                       // if no ratio is stored, we will panic, since that should not happen

        // get the amount of tokens the shares represent by multiplying the ratio by the locked shares
        let locked_tokens = ratio
            .checked_mul(Decimal::new(Uint128::new(locked_shares)))
            .unwrap();

        total_locked_tokens += locked_tokens.to_uint_ceil().u128();
    }

    Ok(total_locked_tokens)
}

// Returns the amount of voting-power equivalent
// of tokens that was locked in the contract
// at the end of the given round
// (or at the time of the query, if the given round_id is the current round).
// considering only the top max_validator_shares_participating validators.
// If the given round_id is in the future, this will return an error.
pub fn compute_total_locked_tokens(deps: Deps, env: Env, round_id: u64) -> StdResult<u128> {
    let constants = crate::state::CONSTANTS.load(deps.storage)?;

    compute_total_locked_tokens_helper(
        deps,
        env,
        round_id,
        constants.max_validator_shares_participating,
    )
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
