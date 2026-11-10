use cosmwasm_std::{
    CheckedFromRatioError, CheckedMultiplyFractionError, CheckedMultiplyRatioError,
    ConversionOverflowError, DivideByZeroError, OverflowError, StdError, Uint128,
};
use cw_utils::PaymentError;
use thiserror::Error;

/// The errors of SPEC §11. The hATOM and vault errors reach the caller as the failed
/// submessage's error, passed through unchanged.
#[derive(Error, Debug, PartialEq)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("{0}")]
    Payment(#[from] PaymentError),

    #[error("{0}")]
    Overflow(#[from] OverflowError),

    #[error("{0}")]
    DivideByZero(#[from] DivideByZeroError),

    #[error("{0}")]
    CheckedMultiplyRatio(#[from] CheckedMultiplyRatioError),

    #[error("{0}")]
    CheckedMultiplyFraction(#[from] CheckedMultiplyFractionError),

    #[error("{0}")]
    CheckedFromRatio(#[from] CheckedFromRatioError),

    #[error("{0}")]
    ConversionOverflow(#[from] ConversionOverflowError),

    #[error("unauthorized")]
    Unauthorized,

    #[error("the gate is paused")]
    Paused,

    #[error("the share price moved too far from the reference: an operator must check it and reset the reference")]
    PriceJump,

    #[error("the share price is too far from the stated price")]
    ResetTooFar,

    #[error("the free buffer is too low for this exit")]
    BufferTooLow,

    #[error("not enough shares in the inventory")]
    NoInventory,

    #[error("the ATOM sent is below the cost basis of the shares")]
    BelowCost,

    #[error("{got} is below the minimum asked ({min})")]
    BelowMinimum { got: Uint128, min: Uint128 },

    #[error("the trade's output would be zero")]
    ZeroOutput,

    #[error("cap exceeded: {which}")]
    CapExceeded { which: String },

    #[error("the batch has no capacity left for this booking")]
    BatchFull,

    #[error("the batch has matured or is about to")]
    BatchMatured,

    #[error("not due yet")]
    NotDue,

    #[error("nothing to do")]
    NothingToDo,

    #[error("invalid config: {0}")]
    InvalidConfig(&'static str),

    #[error("unknown batch")]
    UnknownBatch,

    #[error("unknown booking")]
    UnknownBooking,

    #[error("another operation on this contract is in progress")]
    OpInProgress,

    #[error("the burn would take from too many validators")]
    TooManyLegs,

    #[error("the burn's imbalance fee is above the limit")]
    FeeTooHigh,

    #[error("the vault queued the withdrawal instead of paying it")]
    WithdrawalQueued,

    #[error("the vault has unfunded queued withdrawals")]
    QueueNotEmpty,

    #[error("invariant violated: {0}")]
    Invariant(String),
}
