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

    #[error("Query for pool position")]
    PositionQueryFailed {},

    #[error("Position not found")]
    PositionNotFound {},

    #[error("Ratio is still in bounds. Principal token amount is: {amount}")]
    ThresholdNotMet { amount: String },
    #[error("Invalid ratio format")]
    InvalidRatioFormat {},
    #[error("Threshold not set")]
    ThresholdNotSet {},
    #[error("Invalid conversion")]
    InvalidConversion {},
    #[error("Excessive liquidation amount")]
    ExcessiveLiquidationAmount {},
    #[error("Asset not found")]
    AssetNotFound {},
    #[error("No liquidator address")]
    NoLiquidatorAddress {},
}
