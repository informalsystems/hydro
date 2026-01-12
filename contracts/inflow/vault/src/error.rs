use cosmwasm_std::{
    CheckedFromRatioError, ConversionOverflowError, OverflowError, StdError, Uint128,
};
use cw_utils::PaymentError;
use interface::inflow_vault::DeploymentTracking;
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

    #[error("Adapter already exists: {name}")]
    AdapterAlreadyExists { name: String },

    #[error("Adapter not found: {name}")]
    AdapterNotFound { name: String },

    #[error("Adapter not included in automated allocation: {name}")]
    AdapterNotIncludedInAutomatedAllocation { name: String },

    #[error("Insufficient vault balance: available {available}, required {required}")]
    InsufficientBalance {
        available: Uint128,
        required: Uint128,
    },

    #[error("Adapter deployment tracking mismatch: {from_adapter} is {from_tracking:?}, {to_adapter} is {to_tracking:?}. Cannot move non-deposit_denom funds between adapters with different tracking types.")]
    AdapterTrackingMismatch {
        from_adapter: String,
        to_adapter: String,
        from_tracking: DeploymentTracking,
        to_tracking: DeploymentTracking,
    },

    #[error("Zero amount not allowed")]
    ZeroAmount {},
}

pub fn new_generic_error(msg: impl Into<String>) -> ContractError {
    ContractError::Std(StdError::generic_err(msg))
}
