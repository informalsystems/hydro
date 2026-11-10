//! A small copy of the hATOM messages the gate uses (hatom-contracts `e30edbe`,
//! `contracts/hatom/src/msg.rs`). hATOM lives in its own repo, so its crate is not a dependency.
//!
//! The responses derive plain `serde` (not `cw_serde`) so that fields hATOM may add later do not
//! break the gate: unknown fields are ignored.

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Coin, Decimal256, Uint128, Uint256};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[cw_serde]
pub struct BasketEntry {
    pub validator: String,
    pub weight_bps: u32,
}

#[cw_serde]
pub enum HatomExecuteMsg {
    MintFromNative {
        validator: Option<String>,
        recipient: Option<String>,
        max_legs: Option<u32>,
        min_out: Option<Uint128>,
    },
    Burn {
        recipient: Option<String>,
        share_owner: Option<String>,
        max_legs: Option<u32>,
        split: Option<Vec<BasketEntry>>,
        min_out: Option<Uint128>,
    },
    Compound {},
}

#[cw_serde]
pub enum HatomQueryMsg {
    Config {},
    Rate {},
    SimulateMintNative {
        amount: Uint128,
        validator: Option<String>,
        max_legs: Option<u32>,
    },
    SimulateBurn {
        amount: Uint128,
        max_legs: Option<u32>,
        split: Option<Vec<BasketEntry>>,
    },
}

/// The part of hATOM's `Config` the gate reads.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct HatomConfig {
    pub denom: String,
    pub paused: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct RateResponse {
    pub exchange_rate: Decimal256,
    pub supply: Uint128,
    /// `u` hATOM are worth `⌊u × holders_value / supply⌋` ATOM.
    pub holders_value: Uint256,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Allocation {
    pub validator: String,
    pub amount: Uint128,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct MintSimulation {
    pub minted: Uint128,
    pub imbalance_fee: Uint128,
    pub imbalance_rebate: Uint128,
    pub allocation: Vec<Allocation>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct BurnSimulation {
    pub value: Uint128,
    pub imbalance_fee: Uint128,
    pub imbalance_rebate: Uint128,
    pub out: Uint128,
    pub allocation: Vec<Allocation>,
}

/// `data` of a successful `MintFromNative`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct MintResult {
    pub minted: Uint128,
}

/// `data` of a successful `Burn`: the LSM shares sent to the recipient.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct BurnResult {
    pub shares: Vec<Coin>,
}

impl RateResponse {
    /// The ATOM value of `h` hATOM, floored.
    pub fn value_of(&self, h: Uint128) -> Uint128 {
        if self.supply.is_zero() {
            return Uint128::zero();
        }
        let v = Uint256::from(h)
            .checked_mul(self.holders_value)
            .map(|n| n / Uint256::from(self.supply))
            .unwrap_or(Uint256::MAX);
        Uint128::try_from(v).unwrap_or(Uint128::MAX)
    }
}
