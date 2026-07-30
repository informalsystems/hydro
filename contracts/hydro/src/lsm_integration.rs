use cosmwasm_std::{StdError, StdResult};

pub const COSMOS_VALIDATOR_PREFIX: &str = "cosmosvaloper";
pub const COSMOS_VALIDATOR_ADDR_LENGTH: usize = 52; // e.g. cosmosvaloper15w6ra6m68c63t0sv2hzmkngwr9t88e23r8vtg5

// Given an input denom, verifies that it is a valid LSM tokenized share denom and returns the validator address contained in the denom.
pub fn extract_validator_from_lsm_denom(denom: String) -> StdResult<String> {
    let denom_parts: Vec<&str> = denom.split("/").collect();

    if denom_parts.len() != 2
        || denom_parts[0].len() != COSMOS_VALIDATOR_ADDR_LENGTH
        || !denom_parts[0].starts_with(COSMOS_VALIDATOR_PREFIX)
        || denom_parts[1].parse::<u64>().is_err()
    {
        return Err(StdError::generic_err(
            "Only LSTs from the Cosmos Hub can be locked.",
        ));
    }

    Ok(denom_parts[0].to_string())
}
