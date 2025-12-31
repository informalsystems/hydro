use cosmwasm_std::StdError;
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("At least one admin should be set")]
    AtLeastOneAdmin {},

    #[error("Unauthorized")]
    UnauthorizedAdmin {},

    #[error("No dATOM funds available to unbond")]
    NoFundsToUnbond {},

    #[error("At least one executor should be set")]
    AtLeastOneExecutor {},

    #[error("Unauthorized - only executor can call this")]
    UnauthorizedExecutor {},
}
