use cosmwasm_std::StdError;
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Insufficient funds")]
    InsufficientFunds {},

    #[error("No position exists")]
    NoPosition {},

    #[error("Unknown reply id: {id}")]
    UnknownReplyId { id: u64 },

    #[error("Query for pool price failed")]
    PriceQueryFailed {},

    #[error("Ratio is still in the bounds")]
    ThresholdNotMet {},
    #[error("Invalid ratio format")]
    InvalidRatioFormat {},
}
