use cosmwasm_std::{Deps, StdError, StdResult, Validator};

use crate::state::LOCKED_VALIDATOR_SHARES;

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
// of tokens that is locked in the contract.
// This only counts tokenized shares corresponding
// to the top max_validators validators.
pub fn compute_total_locked_tokens_helper(deps: Deps, max_validators: u64) -> u128 {
    // get the top max_validators validators
    let validators = get_validators(deps, max_validators);

    // sum the locked tokens of the top max_validators validators
    let mut total_locked_tokens = 0;
    for validator in validators {
        let locked_tokens = LOCKED_VALIDATOR_SHARES
            .load(deps.storage, validator)
            .unwrap_or(0); // get the amount of tokens locked for that validator, or 0 if none are locked
        total_locked_tokens += locked_tokens;
    }

    total_locked_tokens
}

// Returns the amount of voting-power equivalent
// of tokens that is locked in the contract,
// considering only the top max_validator_shares_participating validators.
pub fn compute_total_locked_tokens(deps: Deps) -> u128 {
    let constants = crate::state::CONSTANTS.load(deps.storage).unwrap();
    compute_total_locked_tokens_helper(deps, constants.max_validator_shares_participating)
}
