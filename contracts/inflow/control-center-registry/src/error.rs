use cosmwasm_std::StdError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("unauthorized")]
    Unauthorized {},

    #[error("control center {address} is already registered")]
    AlreadyRegistered { address: String },

    #[error("control center {address} is not registered")]
    NotRegistered { address: String },
}
