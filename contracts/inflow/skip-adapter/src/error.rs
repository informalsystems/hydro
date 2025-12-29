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

    #[error("Unauthorized - only executors or admins can call this function")]
    UnauthorizedExecutor {},

    #[error("Depositor not registered: {depositor_address}")]
    DepositorNotRegistered { depositor_address: String },

    #[error("Depositor already registered: {depositor_address}")]
    DepositorAlreadyRegistered { depositor_address: String },

    #[error("Invalid funds: expected exactly one coin, got {count}")]
    InvalidFunds { count: usize },

    #[error("Zero amount not allowed")]
    ZeroAmount {},

    #[error("Insufficient balance for swap")]
    InsufficientBalance {},

    #[error("Route not registered: {route_id}")]
    RouteNotRegistered { route_id: String },

    #[error("Route already registered: {route_id}")]
    RouteAlreadyRegistered { route_id: String },

    #[error("Route is disabled: {route_id}")]
    RouteDisabled { route_id: String },

    #[error("Invalid route: {reason}")]
    InvalidRoute { reason: String },

    #[error("Executor already exists: {executor}")]
    ExecutorAlreadyExists { executor: String },

    #[error("Executor not found: {executor}")]
    ExecutorNotFound { executor: String },

    #[error("Invalid slippage: {bps} basis points exceeds maximum of {max_bps} (10%)")]
    InvalidSlippage { bps: u64, max_bps: u64 },
}
