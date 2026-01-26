use cosmwasm_std::{CheckedFromRatioError, ConversionOverflowError, OverflowError, StdError};
use cw_utils::PaymentError;
use neutron_sdk::NeutronError;
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

    #[error(transparent)]
    NeutronError(#[from] NeutronError),

    #[error("Unauthorized")]
    Unauthorized,

    #[error("Invalid fee rate: must be between 0 and 1")]
    InvalidFeeRate,

    #[error("Fee accrual is disabled (fee_rate is zero)")]
    FeeAccrualDisabled,

    #[error("No shares have been issued yet")]
    NoSharesIssued,

    #[error("Fee recipient must be set before setting a non-zero fee rate")]
    FeeRecipientNotSet,
}

pub fn new_generic_error(msg: impl Into<String>) -> ContractError {
    ContractError::Std(StdError::generic_err(msg))
}
