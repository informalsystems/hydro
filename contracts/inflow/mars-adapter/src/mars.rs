// Mars Protocol Integration Module
//
// This module contains message types and helper functions for interacting with Mars Protocol
// Credit Manager contract on Osmosis.

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{to_json_binary, Addr, Coin, QuerierWrapper, StdResult, Uint128, WasmMsg};

/// Mars Credit Manager Execute Messages
#[cw_serde]
pub enum MarsCreditManagerExecuteMsg {
    /// Create a new credit account
    CreateCreditAccount(Option<String>),
    /// Update an existing credit account with actions
    UpdateCreditAccount {
        account_id: String,
        actions: Vec<Action>,
    },
}

/// Mars Credit Manager Query Messages
#[cw_serde]
pub enum MarsCreditManagerQueryMsg {
    /// Query account positions
    Positions {
        account_id: String,
        action: Option<String>,
    },
}

/// Mars Params contract query messages
#[cw_serde]
pub enum MarsParamsQueryMsg {
    TotalDeposit { denom: String },
}

/// Response from Mars Params TotalDeposit query
#[cw_serde]
pub struct TotalDepositResponse {
    pub denom: String,
    pub cap: Uint128,
    pub amount: Uint128,
}

/// Response from Mars Positions query
#[cw_serde]
pub struct PositionsResponse {
    pub account_id: String,
    pub deposits: Vec<Coin>,
    pub debts: Vec<Coin>,
    pub lends: Vec<Coin>,
    // Other fields omitted for simplicity (vaults, staked_astro_lps, perps, etc.)
}

/// Amount type for Mars actions (Lend, Reclaim, Withdraw)
/// Can be either "account_balance" or { exact: Uint128 }
#[cw_serde]
pub enum ActionAmount {
    /// Exact amount
    Exact(String),
    /// Use entire account balance
    AccountBalance,
}

/// ActionCoin used for Lend, Reclaim, and Withdraw actions
#[cw_serde]
pub struct ActionCoin {
    pub denom: String,
    pub amount: ActionAmount,
}

/// Actions that can be performed on a Mars credit account
#[cw_serde]
pub enum Action {
    /// Deposit coins into the credit account (uses regular Coin)
    Deposit(Coin),
    /// Lend coins from the credit account (uses ActionCoin)
    Lend(ActionCoin),
    /// Reclaim coins from lending (uses ActionCoin)
    Reclaim(ActionCoin),
    /// Withdraw coins from the credit account (uses ActionCoin)
    Withdraw(ActionCoin),
    /// Withdraw coins to a wallet (uses ActionCoin)
    WithdrawToWallet { coin: ActionCoin, recipient: String },
}

/// Helper function to create a Mars CreateCreditAccount message
pub fn create_mars_account_msg(
    mars_credit_manager: Addr,
    account_kind: Option<String>,
) -> StdResult<WasmMsg> {
    let msg = MarsCreditManagerExecuteMsg::CreateCreditAccount(account_kind);
    Ok(WasmMsg::Execute {
        contract_addr: mars_credit_manager.to_string(),
        msg: to_json_binary(&msg)?,
        funds: vec![],
    })
}

/// Helper function to create a Mars UpdateCreditAccount message for deposit + lend
pub fn create_mars_deposit_lend_msg(
    mars_credit_manager: Addr,
    account_id: String,
    coin: Coin,
) -> StdResult<WasmMsg> {
    let action_coin = ActionCoin {
        denom: coin.denom.clone(),
        amount: ActionAmount::Exact(coin.amount.to_string()),
    };

    let msg = MarsCreditManagerExecuteMsg::UpdateCreditAccount {
        account_id,
        actions: vec![Action::Deposit(coin.clone()), Action::Lend(action_coin)],
    };
    Ok(WasmMsg::Execute {
        contract_addr: mars_credit_manager.to_string(),
        msg: to_json_binary(&msg)?,
        funds: vec![coin],
    })
}

/// Helper function to create a Mars UpdateCreditAccount message for reclaim + withdraw
pub fn create_mars_reclaim_withdraw_msg(
    mars_credit_manager: Addr,
    account_id: String,
    coin: Coin,
    recipient: Addr,
) -> StdResult<WasmMsg> {
    let action_coin = ActionCoin {
        denom: coin.denom.clone(),
        amount: ActionAmount::Exact(coin.amount.to_string()),
    };

    let msg = MarsCreditManagerExecuteMsg::UpdateCreditAccount {
        account_id,
        actions: vec![
            Action::Reclaim(action_coin.clone()),
            Action::WithdrawToWallet {
                coin: action_coin,
                recipient: recipient.to_string(),
            },
        ],
    };
    Ok(WasmMsg::Execute {
        contract_addr: mars_credit_manager.to_string(),
        msg: to_json_binary(&msg)?,
        funds: vec![],
    })
}

/// Query Mars positions for a given account
pub fn query_mars_positions(
    querier: &QuerierWrapper,
    mars_credit_manager: &Addr,
    account_id: String,
) -> StdResult<PositionsResponse> {
    let query_msg = MarsCreditManagerQueryMsg::Positions {
        account_id,
        action: None,
    };

    querier.query_wasm_smart(mars_credit_manager.to_string(), &query_msg)
}

/// Get the lent amount for a specific denom from Mars positions
pub fn get_lent_amount_for_denom(
    querier: &QuerierWrapper,
    mars_credit_manager: &Addr,
    account_id: String,
    denom: &str,
) -> StdResult<Uint128> {
    let positions = query_mars_positions(querier, mars_credit_manager, account_id)?;

    // Find the lent amount for the requested denom
    let lent_amount = positions
        .lends
        .iter()
        .find(|coin| coin.denom == denom)
        .map(|coin| coin.amount)
        .unwrap_or_else(Uint128::zero);

    Ok(lent_amount)
}

/// Query Mars Params contract for total deposit info
pub fn query_mars_total_deposit(
    querier: &QuerierWrapper,
    mars_params: &Addr,
    denom: String,
) -> StdResult<TotalDepositResponse> {
    let query_msg = MarsParamsQueryMsg::TotalDeposit { denom };
    querier.query_wasm_smart(mars_params.to_string(), &query_msg)
}
