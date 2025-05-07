#[cfg(test)]
pub mod calculations {
    use crate::error::ContractError;
    use bigdecimal::BigDecimal;
    use bigdecimal::ToPrimitive;
    use num_bigint::BigInt;
    use num_bigint::BigUint;
    use num_bigint::ToBigInt;
    use num_traits::{FromPrimitive, One, Zero}; // We need this for easier BigDecimal manipulations
    use once_cell::sync::Lazy;
    use osmosis_std::types::cosmos::base;
    use std::collections::HashMap;
    use std::str::FromStr; // Import Lazy for static initialization

    // Constants
    const MIN_INITIALIZED_TICK_V2: i64 = -270_000_000;
    const MIN_CURRENT_TICK_V2: i64 = MIN_INITIALIZED_TICK_V2 - 1;
    const EXPONENT_AT_PRICE_ONE: i64 = -6;
    const GEOMETRIC_EXPONENT_INCREMENT_DISTANCE_IN_TICKS: i64 = 9_000_000;

    // Maximum and minimum spot prices used by Osmosis
    pub static MAX_SPOT_PRICE: Lazy<BigDecimal> =
        Lazy::new(|| BigDecimal::from_str("100000000000000000000000000000000000000").unwrap());

    pub static MIN_SPOT_PRICE: Lazy<BigDecimal> = Lazy::new(|| {
        BigDecimal::from_str("0.000000000001").unwrap() // 10^-12
    });

    pub static MIN_SPOT_PRICE_V2: Lazy<BigDecimal> = Lazy::new(|| {
        BigDecimal::new(1.into(), 30) // Equivalent to 1e-30
    });

    #[derive(Debug, Clone)]
    pub struct TickExpIndexData {
        /// If price < initial_price, we are not in this exponent range.
        pub initial_price: BigDecimal,

        /// If price >= max_price, we are not in this exponent range.
        pub max_price: BigDecimal,

        /// Additive increment per tick in this exponent range.
        pub additive_increment_per_tick: BigDecimal,

        /// The tick that corresponds to `initial_price`
        pub initial_tick: i64,
    }

    fn pow10(exp: i64) -> BigDecimal {
        BigDecimal::new(1.into(), -exp)
    }

    pub static TICK_EXP_CACHE: Lazy<HashMap<i64, TickExpIndexData>> = Lazy::new(|| {
        let mut cache = HashMap::new();

        let max_spot_price = BigDecimal::parse_bytes(b"1e38", 10).unwrap();
        let min_spot_price = BigDecimal::parse_bytes(b"1e-30", 10).unwrap();
        let mut cur_exp = 0i64;

        let mut max_price = BigDecimal::from(1u32);
        while max_price < max_spot_price {
            let initial_price = pow_ten_decimal(cur_exp);
            let max_price_val = pow_ten_decimal(cur_exp + 1);
            let additive = pow10(EXPONENT_AT_PRICE_ONE + cur_exp);
            let tick = GEOMETRIC_EXPONENT_INCREMENT_DISTANCE_IN_TICKS * cur_exp;

            cache.insert(
                cur_exp,
                TickExpIndexData {
                    initial_price,
                    max_price: max_price_val.clone(),
                    additive_increment_per_tick: additive,
                    initial_tick: tick,
                },
            );

            max_price = max_price_val;
            cur_exp += 1;
        }

        cur_exp = -1;
        let mut min_price = BigDecimal::from(1u32);
        while min_price > min_spot_price {
            let initial_price = pow10(cur_exp);
            let max_price_val = pow10(cur_exp + 1);
            let additive = pow10(EXPONENT_AT_PRICE_ONE + cur_exp);
            let tick = GEOMETRIC_EXPONENT_INCREMENT_DISTANCE_IN_TICKS * cur_exp;

            cache.insert(
                cur_exp,
                TickExpIndexData {
                    initial_price: initial_price.clone(),
                    max_price: max_price_val,
                    additive_increment_per_tick: additive,
                    initial_tick: tick,
                },
            );

            min_price = initial_price;
            cur_exp -= 1;
        }

        cache
    });

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
        let num_additive_ticks = tick_index
            - (geometric_exponent_delta * GEOMETRIC_EXPONENT_INCREMENT_DISTANCE_IN_TICKS);

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
        //println!("Price: {}", price);

        if price > max_spot_price() || price < min_spot_price_v2() {
            //return Err(ContractError::PriceOutOfBounds);
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
            let sqrt_price =
                BigDecimal::from_f64(sqrt).ok_or(ContractError::InvalidConversion {})?;
            return Ok(sqrt_price);
        }
        return Err(ContractError::InvalidConversion {});
    }

    pub fn price_to_tick(price: &BigDecimal) -> Result<i64, String> {
        if price <= &BigDecimal::zero() {
            return Err("price must be greater than zero".to_string());
        }

        // Clamp to min/max bounds if necessary
        if price > &MAX_SPOT_PRICE || price < &MIN_SPOT_PRICE {
            return Err("price is outside bounds".to_string());
        }

        if price == &BigDecimal::from(1u32) {
            return Ok(0);
        }

        // Determine which geometric spacing we're in
        let mut geo_spacing = None;

        if price > &BigDecimal::from(1u32) {
            let mut idx = 0;
            loop {
                if let Some(data) = TICK_EXP_CACHE.get(&idx) {
                    if &data.max_price >= price {
                        geo_spacing = Some(data);
                        break;
                    }
                } else {
                    break;
                }
                idx += 1;
            }
        } else {
            let mut idx = -1;
            loop {
                if let Some(data) = TICK_EXP_CACHE.get(&idx) {
                    if &data.initial_price <= price {
                        geo_spacing = Some(data);
                        break;
                    }
                } else {
                    break;
                }
                idx -= 1;
            }
        }

        let data = geo_spacing.ok_or("could not find appropriate tick spacing")?;

        let price_delta = price - &data.initial_price;
        let ticks_filled = &price_delta / &data.additive_increment_per_tick;
        let tick_index = data.initial_tick + ticks_filled.to_bigint().unwrap().to_i64().unwrap();

        Ok(tick_index)
    }
    pub fn calc_amount0(
        amount1: BigDecimal,
        lower_tick: i64,
        upper_tick: i64,
        current_sqrt_price: BigDecimal,
    ) -> BigInt {
        // Calculate liquidity1
        let liquidity1 =
            calc_liquidity_amount1(lower_tick, current_sqrt_price.clone(), amount1.clone());

        // Convert upper tick to sqrt price
        let upper_tick_sqrt = tick_to_sqrt_price(upper_tick).unwrap();

        // Set sqrtPriceA and sqrtPriceB based on the current price and upper tick
        let (sqrt_price_a, sqrt_price_b) = if current_sqrt_price > upper_tick_sqrt {
            (upper_tick_sqrt.clone(), current_sqrt_price)
        } else {
            (current_sqrt_price, upper_tick_sqrt.clone())
        };

        // Calculate the numerator and denominator for the final result
        let numerator = liquidity1 * (sqrt_price_b.clone() - sqrt_price_a.clone());
        let denominator = sqrt_price_b * sqrt_price_a;

        // Calculate the result (liquidity0) and round it up
        let result = numerator / denominator;

        round_up(&result)
    }

    pub fn round_up(value: &BigDecimal) -> BigInt {
        let int_part = value.to_bigint().unwrap();
        if value > &BigDecimal::from(int_part.clone()) {
            int_part + 1
        } else {
            int_part
        }
    }

    pub fn calc_amount1(
        amount0: BigDecimal,
        lower_tick: i64,
        upper_tick: i64,
        current_sqrt_price: BigDecimal,
    ) -> BigDecimal {
        // Calculate liquidity0
        let liquidity0 =
            calc_liquidity_amount0(upper_tick, current_sqrt_price.clone(), amount0.clone());
        //let liquidity0 = liquidity0.with_scale(0); // round up
        let liquidity0 = round_up_bigdec(&liquidity0); // round up

        // Sqrt prices
        let lower_tick_sqrt = tick_to_sqrt_price(lower_tick).unwrap();
        let (mut sqrt_price_a, mut sqrt_price_b) = (current_sqrt_price.clone(), lower_tick_sqrt);

        if sqrt_price_a > sqrt_price_b {
            std::mem::swap(&mut sqrt_price_a, &mut sqrt_price_b);
        }

        let diff = &sqrt_price_b - &sqrt_price_a;
        let result = &liquidity0 * diff;

        //result.round(0).to_bigint().unwrap()
        result
    }

    pub fn round_up_bigdec(value: &BigDecimal) -> BigInt {
        let integer_part = value.with_scale(0);
        let fractional_part = value - &integer_part;

        if fractional_part.is_zero() || value < &BigDecimal::zero() {
            integer_part.to_bigint().unwrap()
        } else {
            integer_part.to_bigint().unwrap() + BigInt::from(1)
        }
    }

    pub fn calc_liquidity_amount1(
        lower_tick: i64,
        current_sqrt_price: BigDecimal,
        amount1: BigDecimal,
    ) -> BigDecimal {
        // Convert the lower tick to its corresponding sqrt price
        let lower_tick_sqrt = tick_to_sqrt_price(lower_tick).unwrap();

        // Set sqrtPriceA and sqrtPriceB based on the current price and lower tick
        let (sqrt_price_a, sqrt_price_b) = if current_sqrt_price == lower_tick_sqrt {
            // If sqrtPriceA equals sqrtPriceB, return zero liquidity
            return BigDecimal::zero();
        } else if current_sqrt_price > lower_tick_sqrt {
            (lower_tick_sqrt.clone(), current_sqrt_price)
        } else {
            (current_sqrt_price, lower_tick_sqrt.clone())
        };

        // Perform the division and return the result
        amount1 / (sqrt_price_b - sqrt_price_a)
    }

    pub fn calc_liquidity_amount0(
        upper_tick: i64,
        current_sqrt_price: BigDecimal,
        amount0: BigDecimal,
    ) -> BigDecimal {
        let upper_tick_sqrt = tick_to_sqrt_price(upper_tick).unwrap();

        let mut sqrt_price_a = current_sqrt_price.clone();
        let mut sqrt_price_b = upper_tick_sqrt;

        if sqrt_price_a == sqrt_price_b {
            return BigDecimal::zero();
        }

        if sqrt_price_a > sqrt_price_b {
            std::mem::swap(&mut sqrt_price_a, &mut sqrt_price_b);
        }

        let numerator = BigDecimal::from(amount0) * &sqrt_price_a * &sqrt_price_b;
        let denominator = &sqrt_price_b - &sqrt_price_a;

        numerator / denominator
    }

    /// Calculates the total required amount of token1 (counterparty) to be provided,
    /// including a liquidation bonus equal to `liquidation_bonus` percent of `amount0`
    /// valued at the lower tick price.
    pub fn calc_required_token1_with_bonus(
        amount0: BigDecimal,           // Principal token amount (e.g., OSMO)
        amount1: BigDecimal,           // Counterparty token amount (e.g., USDC), already computed
        lower_tick: i64,               // Lower tick of the LP range
        liquidation_bonus: BigDecimal, // e.g., 0.20 for 20% bonus
    ) -> BigInt {
        // Step 1: Compute price at lower tick
        let sqrt_price_lower = tick_to_sqrt_price(lower_tick).expect("invalid lower tick");
        let price_at_lower_tick = &sqrt_price_lower * &sqrt_price_lower;

        // Step 2: Calculate bonus value in token1 denom
        let bonus_value = amount0 * price_at_lower_tick * liquidation_bonus;

        // Step 3: Total required token1 = amount1 + bonus
        let total = amount1 + bonus_value;

        // Step 4: Round up
        round_up_bigdec(&total)
    }
}
