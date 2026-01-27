use cosmwasm_std::StdError;
use cw_utils::PaymentError;
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("{0}")]
    Payment(#[from] PaymentError),

    #[error("At least one admin should be set")]
    AtLeastOneAdmin {},

    #[error("Unauthorized - only admin can call this")]
    UnauthorizedAdmin {},

    #[error("No dAsset funds available to unbond")]
    NoFundsToUnbond {},

    #[error("At least one executor should be set")]
    AtLeastOneExecutor {},

    #[error("Unauthorized - only executor can call this")]
    UnauthorizedExecutor {},

    #[error("Executor already exists: {address}")]
    ExecutorAlreadyExists { address: String },

    #[error("Executor not found: {address}")]
    ExecutorNotFound { address: String },

    #[error("Insufficient balance for requested amount")]
    InsufficientBalance {},

    #[error("Depositor not whitelisted: {address}")]
    DepositorNotWhitelisted { address: String },

    #[error("Depositor is disabled: {address}")]
    DepositorDisabled { address: String },

    #[error("Token not registered: {denom}")]
    TokenNotRegistered { denom: String },

    #[error("Token not registered by symbol: {symbol}")]
    TokenNotRegisteredBySymbol { symbol: String },

    #[error("Token is disabled: {symbol}")]
    TokenDisabled { symbol: String },

    #[error("Depositor already registered: {address}")]
    DepositorAlreadyRegistered { address: String },

    #[error("Token already registered: {symbol}")]
    TokenAlreadyRegistered { symbol: String },
}
