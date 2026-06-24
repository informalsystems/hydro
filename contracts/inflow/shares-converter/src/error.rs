use cosmwasm_std::StdError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("Unauthorized")]
    Unauthorized,

    #[error("Conversion pair not found for denom: {denom}")]
    PairNotFound { denom: String },

    #[error("Conversion pair already exists for denom: {denom}")]
    PairAlreadyExists { denom: String },

    #[error("Must send exactly one coin")]
    InvalidFunds,

    #[error("Insufficient converter balance: available {available}, required {required}")]
    InsufficientBalance {
        available: cosmwasm_std::Uint128,
        required: cosmwasm_std::Uint128,
    },
}
