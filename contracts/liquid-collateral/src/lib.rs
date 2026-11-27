//! Deprecated: the Vortex contract (liquid-collateral, Osmosis) is no longer used by Hydro.
//! Since Rust 1.100 it is excluded from `make clippy` and `make test-unit`, because its test
//! dependency `osmosis-test-tube` cannot build with cargo's new build-dir layout.

pub mod contract;
pub mod error;
pub mod mock;
pub mod msg;
pub mod state;
pub use crate::error::ContractError;
pub mod calculations;
pub mod testing;
