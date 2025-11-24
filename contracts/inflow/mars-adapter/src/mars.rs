// Mars Protocol Integration Module
//
// This module contains message types and helper functions for interacting with Mars Protocol
// Credit Manager contract on Osmosis.

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{to_json_binary, Addr, Coin, QuerierWrapper, StdResult, Uint128, WasmMsg};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Mars Credit Manager Execute Messages
#[cw_serde]
pub enum MarsExecuteMsg {
    /// Create a new credit account
    CreateCreditAccount { account_kind: Option<String> },
    /// Update an existing credit account with actions
    UpdateCreditAccount {
        account_id: String,
        actions: Vec<Action>,
    },
}

/// Mars Credit Manager Query Messages
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MarsQueryMsg {
    /// Query account positions
    Positions {
        account_id: String,
        action: Option<String>,
    },
}

/// Response from Mars Positions query
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct PositionsResponse {
    pub account_id: String,
    pub deposits: Vec<Coin>,
    pub debts: Vec<Coin>,
    pub lends: Vec<Coin>,
    // Other fields omitted for simplicity (vaults, staked_astro_lps, perps, etc.)
}

/// Actions that can be performed on a Mars credit account
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Deposit coins into the credit account
    Deposit(Coin),
    /// Lend coins from the credit account
    Lend(Coin),
    /// Reclaim coins from lending
    Reclaim(Coin),
    /// Withdraw coins to a wallet
    WithdrawToWallet { coin: Coin, recipient: Addr },
}

/// Helper function to create a Mars CreateCreditAccount message
pub fn create_mars_account_msg(
    mars_contract: Addr,
    account_kind: Option<String>,
) -> StdResult<WasmMsg> {
    let msg = MarsExecuteMsg::CreateCreditAccount { account_kind };
    Ok(WasmMsg::Execute {
        contract_addr: mars_contract.to_string(),
        msg: to_json_binary(&msg)?,
        funds: vec![],
    })
}

/// Helper function to create a Mars UpdateCreditAccount message for deposit + lend
pub fn create_mars_deposit_lend_msg(
    mars_contract: Addr,
    account_id: String,
    coin: Coin,
) -> StdResult<WasmMsg> {
    let msg = MarsExecuteMsg::UpdateCreditAccount {
        account_id,
        actions: vec![Action::Deposit(coin.clone()), Action::Lend(coin.clone())],
    };
    Ok(WasmMsg::Execute {
        contract_addr: mars_contract.to_string(),
        msg: to_json_binary(&msg)?,
        funds: vec![coin],
    })
}

/// Helper function to create a Mars UpdateCreditAccount message for reclaim + withdraw
pub fn create_mars_reclaim_withdraw_msg(
    mars_contract: Addr,
    account_id: String,
    coin: Coin,
    recipient: Addr,
) -> StdResult<WasmMsg> {
    let msg = MarsExecuteMsg::UpdateCreditAccount {
        account_id,
        actions: vec![
            Action::Reclaim(coin.clone()),
            Action::WithdrawToWallet { coin, recipient },
        ],
    };
    Ok(WasmMsg::Execute {
        contract_addr: mars_contract.to_string(),
        msg: to_json_binary(&msg)?,
        funds: vec![],
    })
}

/// Query Mars positions for a given account
pub fn query_mars_positions(
    querier: &QuerierWrapper,
    mars_contract: &Addr,
    account_id: String,
) -> StdResult<PositionsResponse> {
    let query_msg = MarsQueryMsg::Positions {
        account_id,
        action: None,
    };

    querier.query_wasm_smart(mars_contract.to_string(), &query_msg)
}

/// Get the lent amount for a specific denom from Mars positions
pub fn get_lent_amount_for_denom(
    querier: &QuerierWrapper,
    mars_contract: &Addr,
    account_id: String,
    denom: &str,
) -> StdResult<Uint128> {
    let positions = query_mars_positions(querier, mars_contract, account_id)?;

    // Find the lent amount for the requested denom
    let lent_amount = positions
        .lends
        .iter()
        .find(|coin| coin.denom == denom)
        .map(|coin| coin.amount)
        .unwrap_or_else(Uint128::zero);

    Ok(lent_amount)
}
