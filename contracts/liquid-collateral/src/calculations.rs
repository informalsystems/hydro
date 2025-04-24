use std::str::FromStr;

use crate::error::ContractError;
use bigdecimal::BigDecimal;
use bigdecimal::ToPrimitive;
use num_bigint::BigInt;
use num_bigint::BigUint;
use num_bigint::ToBigInt;
use num_traits::{FromPrimitive, One, Zero}; // We need this for easier BigDecimal manipulations

// Constants
const MIN_INITIALIZED_TICK_V2: i64 = -270_000_000;
const MIN_CURRENT_TICK_V2: i64 = MIN_INITIALIZED_TICK_V2 - 1;
const EXPONENT_AT_PRICE_ONE: i64 = -6;
const GEOMETRIC_EXPONENT_INCREMENT_DISTANCE_IN_TICKS: i64 = 9_000_000;

fn pow_ten_decimal(exponent: i64) -> BigDecimal {
    let ten = 10.to_bigint().unwrap();

    if exponent >= 0 {
        let pow = ten.pow(exponent as u32);
        BigDecimal::new(pow, 0) // scale = 0
    } else {
        let pow = ten.pow((-exponent) as u32);
        BigDecimal::new(1.to_bigint().unwrap(), 0) / BigDecimal::new(pow, 0)
    }
}

fn min_spot_price_v2() -> BigDecimal {
    // Create BigUint for 10^30
    let ten_to_the_30 = BigUint::from_u64(10u64).unwrap().pow(30);

    // Convert 1 into a BigDecimal
    let one = BigDecimal::from_u128(1u128).unwrap();

    // Convert 10^30 to BigInt
    let ten_to_the_30_bigint = num_bigint::BigInt::from(ten_to_the_30);

    // Create BigDecimal for 10^30
    let denominator = BigDecimal::from(ten_to_the_30_bigint);

    // Perform division
    one / denominator
}

fn max_spot_price() -> BigDecimal {
    // Create BigUint for 10^30
    let value = BigUint::from_u64(10u64).unwrap().pow(30);

    // Convert BigUint to BigInt first
    let big_int_value = num_bigint::BigInt::from(value);

    // Convert BigInt to BigDecimal
    BigDecimal::from(big_int_value)
}

// TickToAdditiveGeometricIndices
fn tick_to_additive_geometric_indices(tick_index: i64) -> Result<(i64, i64), ContractError> {
    if tick_index == 0 {
        return Ok((0, 0));
    }
    if tick_index == MIN_INITIALIZED_TICK_V2 || tick_index == MIN_CURRENT_TICK_V2 {
        return Ok((0, -30));
    }
    if tick_index < MIN_CURRENT_TICK_V2 {
        return Err(ContractError::TickIndexTooLow);
    }
    if tick_index > i64::MAX {
        return Err(ContractError::TickIndexTooHigh);
    }

    let geometric_exponent_delta = tick_index / GEOMETRIC_EXPONENT_INCREMENT_DISTANCE_IN_TICKS;
    let num_additive_ticks =
        tick_index - (geometric_exponent_delta * GEOMETRIC_EXPONENT_INCREMENT_DISTANCE_IN_TICKS);

    Ok((num_additive_ticks, geometric_exponent_delta))
}

// TickToPrice
pub fn tick_to_price(tick_index: i64) -> Result<BigDecimal, ContractError> {
    if tick_index == 0 {
        return Ok(BigDecimal::one());
    }

    if tick_index == MIN_INITIALIZED_TICK_V2 || tick_index == MIN_CURRENT_TICK_V2 {
        return Ok(min_spot_price_v2());
    }

    let (num_additive_ticks, geometric_exponent_delta) =
        tick_to_additive_geometric_indices(tick_index)?;
    let mut exponent_at_current_tick = EXPONENT_AT_PRICE_ONE + geometric_exponent_delta;
    let mut unscaled_price = 1_000_000i64;

    if tick_index < 0 {
        exponent_at_current_tick -= 1;
        unscaled_price *= 10;
    }

    unscaled_price += num_additive_ticks;

    let base = pow_ten_decimal(exponent_at_current_tick);
    // Multiply base by unscaled_price
    let price = base * BigDecimal::from_i64(unscaled_price).unwrap();
    //let price = price / BigDecimal::from(10u64.pow(14));
    println!("Price: {}", price);

    if price > max_spot_price() || price < min_spot_price_v2() {
        return Err(ContractError::PriceOutOfBounds);
    }

    Ok(price)
}

/// TickToSqrtPrice computes the square root of the price from the tick index.
pub fn tick_to_sqrt_price(tick_index: i64) -> Result<BigDecimal, ContractError> {
    let price_bigdec = tick_to_price(tick_index)?;

    if tick_index >= MIN_INITIALIZED_TICK_V2 {
        // This is where the precision is truncated to 18 decimals
        let price = price_bigdec; // No need to convert to Dec as we already have BigDec
        let sqrt = price
            .to_f64()
            .ok_or(ContractError::InvalidConversion {})?
            .sqrt();
        let sqrt_price = BigDecimal::from_f64(sqrt).ok_or(ContractError::InvalidConversion {})?;
        return Ok(sqrt_price);
    }
    return Err(ContractError::InvalidConversion {});
}
