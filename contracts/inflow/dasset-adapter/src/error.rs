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

    #[error("No dAsset funds available to unbond")]
    NoFundsToUnbond {},

    #[error("Withdrawal failed: {reason}")]
    WithdrawalFailed { reason: String },

    #[error("No base asset funds received from withdrawal")]
    NoFundsReceived {},

    #[error("At least one executor should be set")]
    AtLeastOneExecutor {},

    #[error("Unauthorized - only executor can call this")]
    UnauthorizedExecutor {},
}
