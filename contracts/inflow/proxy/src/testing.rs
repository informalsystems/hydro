use std::collections::HashMap;

use crate::{
    execute, instantiate,
    testing_mocks::{control_center_subvaults_mock, inflow_config_mock},
    ExecuteMsg, InstantiateMsg,
};
use cosmwasm_std::{
    from_json,
    testing::{mock_dependencies, mock_env, MockApi},
    Addr, BankMsg, Coin, CosmosMsg, WasmMsg,
};
use interface::inflow::{Config as InflowConfig, ExecuteMsg as InflowExecuteMsg};
use test_utils::{
    testing_mocks::{setup_contract_smart_query_mock, MockWasmQuerier},
    utils::get_message_info,
};

const PROXY: &str = "proxy";
const CREATOR: &str = "creator";
const ADMIN_1: &str = "admin1";
const NON_ADMIN: &str = "non_admin";
const RECIPIENT: &str = "recipient";

const CONTROL_CENTER_1: &str = "control_center_1";
const CONTROL_CENTER_2: &str = "control_center_2";
const INFLOW_VAULT_1: &str = "inflow_vault_1";
const INFLOW_VAULT_2: &str = "inflow_vault_2";
const INFLOW_VAULT_3: &str = "inflow_vault_3";

const DEPOSIT_DENOM_1: &str = "deposit_denom_1";
const DEPOSIT_DENOM_2: &str = "deposit_denom_2";
const DEPOSIT_DENOM_3: &str = "deposit_denom_3";

const SHARES_DENOM_1: &str = "shares_denom_1";
const SHARES_DENOM_2: &str = "shares_denom_2";
const SHARES_DENOM_3: &str = "shares_denom_3";

fn get_default_instantiate_msg(admins: Vec<Addr>, control_centers: Vec<Addr>) -> InstantiateMsg {
    InstantiateMsg {
        admins: admins.into_iter().map(|addr| addr.to_string()).collect(),
        control_centers: control_centers
            .into_iter()
            .map(|addr| addr.to_string())
            .collect(),
    }
}

fn get_default_inflow_config(
    api: &MockApi,
    deposit_denom: &str,
    vault_shares_denom: &str,
) -> InflowConfig {
    InflowConfig {
        control_center_contract: api.addr_make(CONTROL_CENTER_1),
        max_withdrawals_per_user: 10,
        token_info_provider_contract: None,
        deposit_denom: deposit_denom.to_owned(),
        vault_shares_denom: vault_shares_denom.to_owned(),
    }
}

#[test]
fn instantiate_test() {
    let (mut deps, env) = (mock_dependencies(), mock_env());

    let creator_info = get_message_info(&deps.api, CREATOR, &[]);

    let admin1 = deps.api.addr_make(ADMIN_1);
    let control_center_1 = deps.api.addr_make(CONTROL_CENTER_1);

    let err = instantiate(
        deps.as_mut(),
        env.clone(),
        creator_info.clone(),
        get_default_instantiate_msg(vec![], vec![]),
    )
    .unwrap_err();
    assert!(err
        .to_string()
        .to_lowercase()
        .contains("no admins provided"));

    let err = instantiate(
        deps.as_mut(),
        env.clone(),
        creator_info.clone(),
        get_default_instantiate_msg(vec![admin1.clone()], vec![]),
    )
    .unwrap_err();
    assert!(err
        .to_string()
        .to_lowercase()
        .contains("no control centers provided"));

    instantiate(
        deps.as_mut(),
        env.clone(),
        creator_info.clone(),
        get_default_instantiate_msg(vec![admin1.clone()], vec![control_center_1.clone()]),
    )
    .unwrap();
}

#[test]
fn forward_to_inflow_test() {
    let (mut deps, mut env) = (mock_dependencies(), mock_env());

    let creator_info = get_message_info(&deps.api, CREATOR, &[]);

    let proxy = deps.api.addr_make(PROXY);
    let admin1 = deps.api.addr_make(ADMIN_1);

    let control_center_1 = deps.api.addr_make(CONTROL_CENTER_1);
    let control_center_2 = deps.api.addr_make(CONTROL_CENTER_2);

    let inflow_vault_1 = deps.api.addr_make(INFLOW_VAULT_1);
    let inflow_vault_2 = deps.api.addr_make(INFLOW_VAULT_2);
    let inflow_vault_3 = deps.api.addr_make(INFLOW_VAULT_3);

    let vault_config_1 = get_default_inflow_config(&deps.api, DEPOSIT_DENOM_1, SHARES_DENOM_1);
    let vault_config_2 = get_default_inflow_config(&deps.api, DEPOSIT_DENOM_2, SHARES_DENOM_2);
    let vault_config_3 = get_default_inflow_config(&deps.api, DEPOSIT_DENOM_3, SHARES_DENOM_3);

    instantiate(
        deps.as_mut(),
        env.clone(),
        creator_info.clone(),
        get_default_instantiate_msg(
            vec![admin1.clone()],
            vec![control_center_1.clone(), control_center_2.clone()],
        ),
    )
    .unwrap();

    env.contract.address = proxy.clone();

    let wasm_querier = MockWasmQuerier::new(HashMap::from_iter([
        setup_contract_smart_query_mock(
            control_center_1.clone(),
            control_center_subvaults_mock(vec![inflow_vault_1.clone(), inflow_vault_2.clone()]),
        ),
        setup_contract_smart_query_mock(
            control_center_2.clone(),
            control_center_subvaults_mock(vec![inflow_vault_3.clone()]),
        ),
        setup_contract_smart_query_mock(inflow_vault_1.clone(), inflow_config_mock(vault_config_1)),
        setup_contract_smart_query_mock(inflow_vault_2.clone(), inflow_config_mock(vault_config_2)),
        setup_contract_smart_query_mock(inflow_vault_3.clone(), inflow_config_mock(vault_config_3)),
    ]));

    let querier_for_deps = wasm_querier.clone();
    deps.querier
        .update_wasm(move |q| querier_for_deps.handler(q));

    let deposit_token_1_balance = 1500u128;
    let deposit_token_3_balance = 2500u128;

    // Mock Proxy contract balances of deposit tokens 1 and 3, in order to test forward_to_inflow()
    deps.querier.bank.update_balance(
        &env.contract.address,
        vec![
            Coin::new(deposit_token_1_balance, DEPOSIT_DENOM_1),
            Coin::new(deposit_token_3_balance, DEPOSIT_DENOM_3),
        ],
    );

    let expected_results: HashMap<String, (String, u128)> = HashMap::from_iter([
        (
            inflow_vault_1.to_string(),
            (DEPOSIT_DENOM_1.to_string(), deposit_token_1_balance),
        ),
        (
            inflow_vault_3.to_string(),
            (DEPOSIT_DENOM_3.to_string(), deposit_token_3_balance),
        ),
    ]);

    let non_admin_info = get_message_info(&deps.api, NON_ADMIN, &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        non_admin_info.clone(),
        ExecuteMsg::ForwardToInflow {},
    )
    .unwrap();

    assert_eq!(2, res.messages.len());

    for msg in res.messages {
        match msg.msg {
            CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr,
                msg,
                funds,
            }) => {
                let expected_result = expected_results.get(&contract_addr).unwrap();
                let inflow_execute_msg = from_json::<InflowExecuteMsg>(msg).unwrap();
                match inflow_execute_msg {
                    InflowExecuteMsg::Deposit { on_behalf_of } => {
                        assert!(on_behalf_of.is_none());
                    }
                    _ => panic!("unexpected msg"),
                }

                assert_eq!(funds.len(), 1);
                assert_eq!(funds[0].clone().denom, expected_result.0);
                assert_eq!(funds[0].clone().amount.u128(), expected_result.1);
            }
            _ => panic!("unexpected msg"),
        }
    }
}

#[test]
fn withdraw_receipt_tokens_test() {
    let (mut deps, mut env) = (mock_dependencies(), mock_env());

    let creator_info = get_message_info(&deps.api, CREATOR, &[]);
    let admin_info = get_message_info(&deps.api, ADMIN_1, &[]);

    let proxy = deps.api.addr_make(PROXY);
    let admin1 = deps.api.addr_make(ADMIN_1);
    let recipient = deps.api.addr_make(RECIPIENT);

    let control_center_1 = deps.api.addr_make(CONTROL_CENTER_1);

    instantiate(
        deps.as_mut(),
        env.clone(),
        creator_info.clone(),
        get_default_instantiate_msg(vec![admin1.clone()], vec![control_center_1.clone()]),
    )
    .unwrap();

    env.contract.address = proxy.clone();

    let vault_shares_1_balance = Coin::new(1500u128, SHARES_DENOM_1);

    // Mock Proxy contract balance of vault shares tokens 1, in order to test withdraw_receipt_tokens_test()
    deps.querier
        .bank
        .update_balance(&env.contract.address, vec![vault_shares_1_balance.clone()]);

    // Only admin can execute this action
    let non_admin_info = get_message_info(&deps.api, NON_ADMIN, &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        non_admin_info.clone(),
        ExecuteMsg::WithdrawReceiptTokens {
            address: recipient.to_string(),
            coin: Coin::new(1700u128, SHARES_DENOM_1),
        },
    )
    .unwrap_err();
    assert!(res.to_string().to_lowercase().contains("unauthorized"));

    let res = execute(
        deps.as_mut(),
        env.clone(),
        admin_info.clone(),
        ExecuteMsg::WithdrawReceiptTokens {
            address: recipient.to_string(),
            coin: Coin::new(1700u128, SHARES_DENOM_1),
        },
    )
    .unwrap();

    assert_eq!(1, res.messages.len());

    match res.messages[0].clone().msg {
        CosmosMsg::Bank(BankMsg::Send { to_address, amount }) => {
            assert_eq!(to_address, recipient.to_string());
            assert_eq!(amount.len(), 1);
            assert_eq!(amount[0].denom, vault_shares_1_balance.denom);
            assert_eq!(amount[0].amount, vault_shares_1_balance.amount);
        }
        _ => panic!("unexpected msg"),
    }
}

#[test]
fn withdraw_funds_test() {
    let (mut deps, mut env) = (mock_dependencies(), mock_env());

    let creator_info = get_message_info(&deps.api, CREATOR, &[]);
    let admin_info = get_message_info(&deps.api, ADMIN_1, &[]);
    let non_admin_info = get_message_info(&deps.api, NON_ADMIN, &[]);

    let proxy = deps.api.addr_make(PROXY);
    let admin1 = deps.api.addr_make(ADMIN_1);
    let recipient = deps.api.addr_make(RECIPIENT);

    let control_center_1 = deps.api.addr_make(CONTROL_CENTER_1);

    let inflow_vault_1 = deps.api.addr_make(INFLOW_VAULT_1);
    let inflow_vault_2 = deps.api.addr_make(INFLOW_VAULT_2);

    let vault_config_1 = get_default_inflow_config(&deps.api, DEPOSIT_DENOM_1, SHARES_DENOM_1);
    let vault_config_2 = get_default_inflow_config(&deps.api, DEPOSIT_DENOM_2, SHARES_DENOM_2);

    instantiate(
        deps.as_mut(),
        env.clone(),
        creator_info.clone(),
        get_default_instantiate_msg(vec![admin1.clone()], vec![control_center_1.clone()]),
    )
    .unwrap();

    env.contract.address = proxy.clone();

    let wasm_querier = MockWasmQuerier::new(HashMap::from_iter([
        setup_contract_smart_query_mock(
            control_center_1.clone(),
            control_center_subvaults_mock(vec![inflow_vault_1.clone(), inflow_vault_2.clone()]),
        ),
        setup_contract_smart_query_mock(inflow_vault_1.clone(), inflow_config_mock(vault_config_1)),
        setup_contract_smart_query_mock(inflow_vault_2.clone(), inflow_config_mock(vault_config_2)),
    ]));

    let querier_for_deps = wasm_querier.clone();
    deps.querier
        .update_wasm(move |q| querier_for_deps.handler(q));

    let vault_shares_1_balance = Coin::new(1500u128, SHARES_DENOM_1);
    let vault_shares_3_balance = Coin::new(1500u128, SHARES_DENOM_3);
    let withdrawal_request = Coin::new(1700u128, SHARES_DENOM_1);

    // Only admin can execute this action
    let res = execute(
        deps.as_mut(),
        env.clone(),
        non_admin_info.clone(),
        ExecuteMsg::WithdrawFunds {
            address: recipient.to_string(),
            coin: withdrawal_request.clone(),
        },
    )
    .unwrap_err();
    assert!(res.to_string().to_lowercase().contains("unauthorized"));

    // Try to withdraw funds when 0 vault shares tokens are on the Proxy contract balance
    let res = execute(
        deps.as_mut(),
        env.clone(),
        admin_info.clone(),
        ExecuteMsg::WithdrawFunds {
            address: recipient.to_string(),
            coin: withdrawal_request.clone(),
        },
    )
    .unwrap_err();
    assert!(res
        .to_string()
        .to_lowercase()
        .contains(format!("failed to withdraw funds; zero balance of {SHARES_DENOM_1}").as_str()));

    // Try to withdraw funds for vault shares tokens that cannot be mapped to any Inflow vault
    deps.querier
        .bank
        .update_balance(&env.contract.address, vec![vault_shares_3_balance.clone()]);

    let res = execute(
        deps.as_mut(),
        env.clone(),
        admin_info.clone(),
        ExecuteMsg::WithdrawFunds {
            address: recipient.to_string(),
            coin: vault_shares_3_balance.clone(),
        },
    )
    .unwrap_err();
    assert!(res
        .to_string()
        .contains(format!("no Inflow vault found for shares denom {SHARES_DENOM_3}").as_str()));

    // Mock Proxy contract balance of vault shares tokens 1, in order to test withdraw_funds_test()
    deps.querier
        .bank
        .update_balance(&env.contract.address, vec![vault_shares_1_balance.clone()]);

    let res = execute(
        deps.as_mut(),
        env.clone(),
        admin_info.clone(),
        ExecuteMsg::WithdrawFunds {
            address: recipient.to_string(),
            coin: withdrawal_request.clone(),
        },
    )
    .unwrap();
    assert_eq!(1, res.messages.len());

    match res.messages[0].clone().msg {
        CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr,
            msg,
            funds,
        }) => {
            assert_eq!(contract_addr, inflow_vault_1.to_string());

            let inflow_execute_msg = from_json::<InflowExecuteMsg>(msg).unwrap();
            match inflow_execute_msg {
                InflowExecuteMsg::Withdraw { on_behalf_of } => {
                    assert_eq!(on_behalf_of, Some(recipient.to_string()));
                }
                _ => panic!("unexpected msg"),
            }

            assert_eq!(funds.len(), 1);
            assert_eq!(funds[0].clone().denom, vault_shares_1_balance.denom);
            assert_eq!(funds[0].clone().amount, vault_shares_1_balance.amount);
        }
        _ => panic!("unexpected msg"),
    }
}
