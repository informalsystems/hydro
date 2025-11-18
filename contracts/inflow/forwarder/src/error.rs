use cosmwasm_std::StdError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("no funds available to forward for denom {denom}")]
    NothingToForward { denom: String },

    #[error("ibc timeout must be positive")]
    InvalidIbcTimeout {},
}
