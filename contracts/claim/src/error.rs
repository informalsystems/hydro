use cosmwasm_std::StdError;
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("Unauthorized")]
    Unauthorized,

    #[error("No funds sent with CreateDistribution")]
    NoFundsSent,

    #[error("Distribution not found: {id}")]
    DistributionNotFound { id: u64 },

    #[error("Distribution {id} has not expired yet")]
    DistributionNotExpired { id: u64 },

    #[error("Distribution {id} has no funds to sweep")]
    NoFundsToSweep { id: u64 },

    #[error("No pending claims for sender")]
    NoPendingClaims,

    #[error("Claims list cannot be empty")]
    EmptyClaims,

    #[error("Total weight must be greater than zero")]
    ZeroTotalWeight,

    #[error("Expiry must be in the future")]
    ExpiryInPast,
}
