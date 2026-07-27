use cosmwasm_std::{Decimal, StdError};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ContractError {
    #[error(transparent)]
    Std(#[from] StdError),

    #[error("Sender is not in the whitelist")]
    Unauthorized {},

    #[error("Whitelist must contain at least one address")]
    WhitelistEmpty {},

    #[error("Address {address} is already in the whitelist")]
    AddressAlreadyInWhitelist { address: String },

    #[error("Address {address} is not in the whitelist")]
    AddressNotInWhitelist { address: String },

    #[error("Ratio difference {actual_diff} exceeds maximum allowed {max_diff} (old_ratio: {old_ratio}, new_ratio: {new_ratio})")]
    RatioDiffExceedsThreshold {
        old_ratio: Decimal,
        new_ratio: Decimal,
        max_diff: Decimal,
        actual_diff: Decimal,
    },
}
