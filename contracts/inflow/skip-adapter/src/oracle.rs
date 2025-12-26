use cosmwasm_std::{Coin, Decimal, Deps, Uint128};
use neutron_sdk::bindings::query::NeutronQuery;
use neutron_std::types::slinky::oracle::v1::OracleQuerier;
use neutron_std::types::slinky::types::v1::CurrencyPair;

use crate::error::ContractError;
use crate::msg::Asset;
use crate::state::{Config, RouteConfig, DENOM_SYMBOL_REGISTRY};

/// Main entry point: Calculate oracle-enhanced min_asset
/// Returns the MAX of (provided_min_asset, oracle_calculated_min_asset)
/// Falls back to provided_min_asset if oracle calculation fails
pub fn calculate_min_asset_with_oracle(
    deps: &Deps<NeutronQuery>,
    coin_in: &Coin,
    route_config: &RouteConfig,
    config: &Config,
    provided_min_asset: &Asset,
) -> Asset {
    // Try to calculate oracle-based min_asset
    let oracle_min_asset = match calculate_oracle_min_asset(
        deps,
        coin_in.amount,
        &route_config.denom_in,
        &route_config.denom_out,
        config.max_slippage_bps,
    ) {
        Some(amount) => {
            // Successfully calculated oracle min_asset
            deps.api
                .debug(&format!("Oracle calculated min_asset: {}", amount));
            Asset::Native {
                denom: route_config.denom_out.clone(),
                amount,
            }
        }
        None => {
            // Oracle calculation failed, use provided min_asset
            deps.api
                .debug("Oracle calculation failed, using provided min_asset");
            return provided_min_asset.clone();
        }
    };

    // Return the maximum of oracle and provided min_asset
    max_asset(&oracle_min_asset, provided_min_asset)
}

/// Calculate oracle-based min_asset amount
/// Returns None if calculation fails at any step
fn calculate_oracle_min_asset(
    deps: &Deps<NeutronQuery>,
    amount_in: Uint128,
    denom_in: &str,
    denom_out: &str,
    slippage_bps: u64,
) -> Option<Uint128> {
    // Calculate expected output using oracle prices
    let expected_output = calculate_expected_output_direct(deps, amount_in, denom_in, denom_out)?;

    // Apply slippage to get minimum
    apply_slippage(expected_output, slippage_bps).ok()
}

/// Calculate expected output using direct pair (denom_in -> denom_out)
/// Uses oracle prices with USD as intermediary
fn calculate_expected_output_direct(
    deps: &Deps<NeutronQuery>,
    amount_in: Uint128,
    denom_in: &str,
    denom_out: &str,
) -> Option<Uint128> {
    // Get symbols for both denoms
    let symbol_in = get_symbol_for_denom(deps, denom_in)?;
    let symbol_out = get_symbol_for_denom(deps, denom_out)?;

    // Query oracle prices (both in USD)
    let price_in = query_slinky_price(deps, &symbol_in)?;
    let price_out = query_slinky_price(deps, &symbol_out)?;

    // Prevent division by zero
    if price_out.is_zero() {
        deps.api.debug(&format!(
            "Price for {} is zero, cannot calculate output",
            symbol_out
        ));
        return None;
    }

    // Calculate: amount_in * (price_in / price_out)
    // Convert amount_in to USD value, then to output amount
    let amount_in_decimal = Decimal::from_atomics(amount_in, 0).ok()?;
    let usd_value = amount_in_decimal.checked_mul(price_in).ok()?;
    let output_decimal = usd_value.checked_div(price_out).ok()?;

    // Convert back to Uint128
    let output_amount = output_decimal.atomics().try_into().ok().map(Uint128::new)?;

    deps.api.debug(&format!(
        "Oracle calculation: {} {} * ({} / {}) = {} {}",
        amount_in, symbol_in, price_in, price_out, output_amount, symbol_out
    ));

    Some(output_amount)
}

/// Query Slinky oracle for price of symbol in USD
/// Returns None if query fails or price is unavailable
fn query_slinky_price(deps: &Deps<NeutronQuery>, symbol: &str) -> Option<Decimal> {
    let currency_pair = CurrencyPair {
        base: symbol.to_string(),
        quote: "USD".to_string(),
    };

    let querier = OracleQuerier::new(&deps.querier);

    match querier.get_price(Some(currency_pair)) {
        Ok(response) => {
            if let Some(price_response) = response.price {
                // Extract price from response
                // Slinky returns price as a string
                if let Ok(price) = price_response.price.parse::<Decimal>() {
                    deps.api
                        .debug(&format!("Oracle price for {}/USD: {}", symbol, price));
                    return Some(price);
                }

                deps.api.debug(&format!(
                    "Failed to parse price for {}/USD: {}",
                    symbol, price_response.price
                ));
            } else {
                deps.api
                    .debug(&format!("No price available for {}/USD", symbol));
            }
            None
        }
        Err(e) => {
            deps.api
                .debug(&format!("Oracle query failed for {}/USD: {}", symbol, e));
            None
        }
    }
}

/// Get symbol for a denom from registry
/// Returns None if mapping doesn't exist
fn get_symbol_for_denom(deps: &Deps<NeutronQuery>, denom: &str) -> Option<String> {
    match DENOM_SYMBOL_REGISTRY.may_load(deps.storage, denom.to_string()) {
        Ok(Some(mapping)) => Some(mapping.symbol),
        Ok(None) => {
            deps.api
                .debug(&format!("No symbol mapping found for denom: {}", denom));
            None
        }
        Err(e) => {
            deps.api.debug(&format!(
                "Error loading symbol mapping for denom {}: {}",
                denom, e
            ));
            None
        }
    }
}

/// Apply slippage to amount
/// Returns amount * (10000 - slippage_bps) / 10000
fn apply_slippage(amount: Uint128, slippage_bps: u64) -> Result<Uint128, ContractError> {
    if slippage_bps >= 10000 {
        return Err(ContractError::InvalidSlippage {
            bps: slippage_bps,
            max_bps: 10000,
        });
    }

    let multiplier = 10000u128 - slippage_bps as u128;
    let multiplied = amount
        .checked_mul(Uint128::from(multiplier))
        .map_err(|_| ContractError::PriceCalculationOverflow {})?;

    let result = multiplied
        .checked_div(Uint128::from(10000u128))
        .map_err(|_| ContractError::PriceCalculationOverflow {})?;

    Ok(result)
}

/// Return max of two Assets (by amount)
/// If denoms don't match, returns asset1
/// If both are Native with matching denoms, returns the one with larger amount
fn max_asset(asset1: &Asset, asset2: &Asset) -> Asset {
    match (asset1, asset2) {
        (
            Asset::Native {
                denom: denom1,
                amount: amount1,
            },
            Asset::Native {
                denom: denom2,
                amount: amount2,
            },
        ) => {
            if denom1 == denom2 {
                if amount1 >= amount2 {
                    asset1.clone()
                } else {
                    asset2.clone()
                }
            } else {
                // Denoms don't match, return first one
                asset1.clone()
            }
        }
        (
            Asset::Cw20 {
                address: addr1,
                amount: amount1,
            },
            Asset::Cw20 {
                address: addr2,
                amount: amount2,
            },
        ) => {
            if addr1 == addr2 {
                if amount1 >= amount2 {
                    asset1.clone()
                } else {
                    asset2.clone()
                }
            } else {
                // Addresses don't match, return first one
                asset1.clone()
            }
        }
        _ => {
            // Different asset types, return first one
            asset1.clone()
        }
    }
}
