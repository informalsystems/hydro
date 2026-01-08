use cosmwasm_std::StdError;
#[cfg(feature = "cosmwasm_compat")]
use interface::compat::StdErrExt;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("unauthorized")]
    Unauthorized {},
}

pub fn new_generic_error(msg: impl Into<String>) -> ContractError {
    ContractError::Std(StdError::generic_err(msg))
}
