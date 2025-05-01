use cosmwasm_std::StdError;
use hex::FromHexError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("{0}")]
    FromHexError(#[from] FromHexError),

    #[error("Unauthorized")]
    Unauthorized,

    #[error("Verification Failed")]
    VerificationFailed,

    #[error("Wrong Hash Length")]
    WrongLength,
}

pub fn new_generic_error(msg: impl Into<String>) -> ContractError {
    ContractError::Std(StdError::generic_err(msg))
}
