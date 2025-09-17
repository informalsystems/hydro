use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Decimal, Uint128};

// A multiplier to normalize shares, such that when a validator has just been created
// and was never slashed, 1 Token = Shares / TOKEN_TO_SHARES_MULTIPLIER.
pub const TOKENS_TO_SHARES_MULTIPLIER: Uint128 = Uint128::new(1_000_000_000_000_000_000);

#[cw_serde]
#[derive(Default)]
pub struct ValidatorInfo {
    pub address: String,
    pub delegated_tokens: Uint128,
    pub power_ratio: Decimal,
}

impl ValidatorInfo {
    pub fn new(address: String, delegated_tokens: Uint128, power_ratio: Decimal) -> Self {
        Self {
            address,
            delegated_tokens,
            power_ratio,
        }
    }
}
