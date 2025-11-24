use cosmwasm_std::StdError;
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("At least one admin should be set")]
    AtLeastOneAdmin {},

    #[error("At least one supported denom must be provided")]
    AtLeastOneDenom {},

    #[error("Unauthorized - only the admin can call this function")]
    UnauthorizedAdmin {},

    #[error("Unauthorized - only Inflow contract can call this function")]
    Unauthorized {},

    #[error("Unsupported denom: {denom}")]
    UnsupportedDenom { denom: String },

    #[error("Inflow vault not registered: {inflow_address}")]
    InflowNotRegistered { inflow_address: String },

    #[error("Inflow vault already registered: {inflow_address}")]
    InflowAlreadyRegistered { inflow_address: String },

    #[error("Invalid funds: expected exactly one coin, got {count}")]
    InvalidFunds { count: usize },

    #[error("Zero amount not allowed")]
    ZeroAmount {},

    #[error("Insufficient balance for withdrawal")]
    InsufficientBalance {},

    #[error("Mars protocol error: {msg}")]
    MarsProtocolError { msg: String },
}
