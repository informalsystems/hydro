use cosmwasm_std::{StdError, Uint128};
use cw_utils::PaymentError;
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("At least one admin should be set")]
    AtLeastOneAdmin {},

    #[error("Unauthorized - only admins can call this function")]
    UnauthorizedAdmin {},

    #[error("Unauthorized - only a registered depositor can call this function")]
    Unauthorized {},

    #[error("Depositor not registered: {depositor_address}")]
    DepositorNotRegistered { depositor_address: String },

    #[error("Depositor already registered: {depositor_address}")]
    DepositorAlreadyRegistered { depositor_address: String },

    #[error(transparent)]
    PaymentError(#[from] PaymentError),

    #[error("Zero amount not allowed")]
    ZeroAmount {},

    #[error("Insufficient balance for withdrawal. Has: {has}, needs: {needs}")]
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

    #[error("Chain not registered: {chain_id}")]
    ChainNotRegistered { chain_id: String },

    #[error("Chain already registered: {chain_id}")]
    ChainAlreadyRegistered { chain_id: String },

    #[error("Invalid chain config: {reason}")]
    InvalidChainConfig { reason: String },

    #[error("Invalid Noble address: {address} - {reason}")]
    InvalidNobleAddress { address: String, reason: String },

    #[error("Invalid EVM address: {address} - {reason}")]
    InvalidEvmAddress { address: String, reason: String },

    #[error("Destination address not allowed for chain {chain_id}: {address}")]
    DestinationAddressNotAllowed { chain_id: String, address: String },

    #[error("Destination address already exists for chain {chain_id}: {address}")]
    DestinationAddressAlreadyExists { chain_id: String, address: String },

    #[error("Destination address doesn't exist for chain {chain_id}: {address}")]
    DestinationAddressDoesNotExist { chain_id: String, address: String },
}
