use cosmwasm_std::StdError;
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
}
