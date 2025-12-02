use cosmwasm_std::StdError;
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

    #[error("Invalid funds: expected exactly one coin, got {count}")]
    InvalidFunds { count: usize },

    #[error("Zero amount not allowed")]
    ZeroAmount {},

    #[error("Insufficient balance for withdrawal")]
    InsufficientBalance {},

    #[error("Token not registered: {denom}")]
    TokenNotRegistered { denom: String },

    #[error("Chain not registered: {chain_id}")]
    ChainNotRegistered { chain_id: String },

    #[error("Recipient not allowed: {recipient} on chain {chain_id}")]
    RecipientNotAllowed { recipient: String, chain_id: String },

    #[error("Chain not allowed for depositor: {chain_id}")]
    ChainNotAllowedForDepositor { chain_id: String },

    #[error("Deposit instructions required for IBC transfers")]
    InstructionsRequired {},

    #[error("Withdrawal not allowed for this depositor")]
    WithdrawalNotAllowed {},

    #[error("Invalid capabilities format")]
    InvalidCapabilities {},

    #[error("Executor already exists: {executor}")]
    ExecutorAlreadyExists { executor: String },

    #[error("Executor not found: {executor}")]
    ExecutorNotFound { executor: String },

    #[error("Unauthorized - only executors or admins can call this function")]
    UnauthorizedExecutor {},
}
