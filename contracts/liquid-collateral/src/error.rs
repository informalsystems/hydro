use cosmwasm_std::StdError;
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Invalid tick range")]
    InvalidTickRange {},

    #[error("Insufficient funds")]
    InsufficientFunds {},

    #[error("No position exists")]
    NoPosition {},
} 