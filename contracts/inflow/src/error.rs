use cosmwasm_std::{OverflowError, StdError};
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

    #[error(transparent)]
    NeutronError(#[from] NeutronError),
}
