use cosmwasm_std::StdError;
use hex::FromHexError;
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("{0}")]
    Hex(#[from] FromHexError),

    #[error("Invalid root hash")]
    InvalidRootHash {},

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Cannot remove the last admin")]
    CannotRemoveLastAdmin {},
}
