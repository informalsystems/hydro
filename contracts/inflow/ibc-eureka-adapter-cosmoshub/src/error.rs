use cosmwasm_std::{StdError, Uint128};
use cw_utils::PaymentError;
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error(transparent)]
    PaymentError(#[from] PaymentError),

    #[error("At least one admin must be set")]
    AtLeastOneAdmin {},

    #[error("Unauthorized - only admins can call this function")]
    UnauthorizedAdmin {},

    #[error("Unauthorized - only a registered depositor can call this function")]
    Unauthorized {},

    #[error("Depositor not registered: {depositor_address}")]
    DepositorNotRegistered { depositor_address: String },

    #[error("Depositor already registered: {depositor_address}")]
    DepositorAlreadyRegistered { depositor_address: String },

    #[error("Zero amount not allowed")]
    ZeroAmount {},

    #[error("Zero fee amount not allowed")]
    ZeroFeeAmount {},

    #[error("Insufficient balance. Has: {has}, needs: {needs}")]
    InsufficientBalance { has: Uint128, needs: Uint128 },

    #[error("Wrong token denom: {denom}")]
    WrongTokenDenom { denom: String },

    #[error("Withdrawal not allowed for this depositor")]
    WithdrawalNotAllowed {},

    #[error("Executor already exists: {executor}")]
    ExecutorAlreadyExists { executor: String },

    #[error("Executor not found: {executor}")]
    ExecutorNotFound { executor: String },

    #[error("Admin already exists: {admin}")]
    AdminAlreadyExists { admin: String },

    #[error("Admin not found: {admin}")]
    AdminNotFound { admin: String },

    #[error("Cannot remove the last admin")]
    CannotRemoveLastAdmin {},

    #[error("Unauthorized - only executors or admins can call this function")]
    UnauthorizedExecutor {},

    #[error("Denom not allowed: {denom}")]
    DenomNotAllowed { denom: String },

    #[error("Denom already allowed: {denom}")]
    DenomAlreadyAllowed { denom: String },

    #[error("Invalid EVM address: {address} - {reason}")]
    InvalidEvmAddress { address: String, reason: String },

    #[error("Destination address not allowed: {address}")]
    DestinationAddressNotAllowed { address: String },

    #[error("Destination address already exists: {address}")]
    DestinationAddressAlreadyExists { address: String },

    #[error("Destination address doesn't exist: {address}")]
    DestinationAddressDoesNotExist { address: String },

    #[error("Invalid config: {reason}")]
    InvalidConfig { reason: String },
}
