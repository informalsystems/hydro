use cosmwasm_std::{CheckedFromRatioError, ConversionOverflowError, OverflowError, StdError};
use cw_utils::PaymentError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("{0}")]
    PaymentError(#[from] PaymentError),

    #[error("{0}")]
    OverflowError(#[from] OverflowError),

    #[error("{0}")]
    CheckedFromRatioError(#[from] CheckedFromRatioError),

    #[error("{0}")]
    ConversionOverflowError(#[from] ConversionOverflowError),

    #[error("Unauthorized")]
    Unauthorized,
}

pub fn new_generic_error(msg: impl Into<String>) -> ContractError {
    ContractError::Std(StdError::generic_err(msg))
}
