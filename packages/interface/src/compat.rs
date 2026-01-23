use cosmwasm_std::{StdError, Uint128, Uint256};

pub trait StdErrExt {
    fn generic_err<T: Into<String>>(msg: T) -> StdError {
        StdError::msg(msg.into())
    }

    // https://github.com/CosmWasm/cosmwasm/blob/33191cb067be86d3b5733e21b4f0fc61343f0661/packages/std/src/errors/std_error.rs#L51C14-L51C30
    fn not_found<T: Into<String>>(msg: T) -> StdError {
        StdError::msg(format!("{} not found", msg.into()))
    }
}

impl StdErrExt for StdError {}

pub trait Uint256TestingExt {
    fn u128(&self) -> u128;
}

impl Uint256TestingExt for Uint256 {
    fn u128(&self) -> u128 {
        Uint128::try_from(*self).unwrap().u128()
    }
}
