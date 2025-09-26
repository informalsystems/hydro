use cosmwasm_std::{CheckedFromRatioError, OverflowError, StdError};
use cw_utils::PaymentError;
use thiserror::Error;

use crate::cw721::Error as Cw721Error;

#[derive(Error, Debug)]
pub enum ContractError {
    #[error(transparent)]
    Std(#[from] StdError),

    #[error(transparent)]
    OverflowError(#[from] OverflowError),

    #[error(transparent)]
    CheckedFromRatioError(#[from] CheckedFromRatioError),

    #[error("Unauthorized")]
    Unauthorized,

    #[error(transparent)]
    PaymentError(#[from] PaymentError),

    #[error(transparent)]
    Cw721Error(#[from] Cw721Error),

    #[error("Paused")]
    Paused,
}

pub fn new_generic_error(msg: impl Into<String>) -> ContractError {
    ContractError::Std(StdError::generic_err(msg))
}
