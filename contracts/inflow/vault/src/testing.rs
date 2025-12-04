use std::{collections::HashMap, marker::PhantomData};

use crate::{
    contract::{
        execute, instantiate, query, query_amount_to_fund_pending_withdrawals,
        query_available_for_deployment, query_user_payouts_history, query_user_withdrawal_requests,
        query_withdrawal_queue_info,
    },
    error::ContractError,
    msg::{DenomMetadata, ExecuteMsg, InstantiateMsg, UpdateConfigData},
    query::QueryMsg,
    state::{CONFIG, LAST_FUNDED_WITHDRAWAL_ID, WITHDRAWAL_QUEUE_INFO},
    testing_mocks::{
        mock_address_balance, setup_control_center_mock, setup_default_control_center_mock,
        setup_token_info_provider_mock, update_contract_mock, MockWasmQuerier,
    },
};
use cosmwasm_std::{
    from_json,
    testing::{mock_env, MockApi, MockQuerier, MockStorage},
    Addr, Api, BalanceResponse, BankMsg, BankQuery, Coin, CosmosMsg, Decimal, Env, MemoryStorage,
    MessageInfo, Order, OwnedDeps, Uint128,
};
use interface::inflow_vault::PoolInfoResponse;
use neutron_sdk::bindings::{msg::NeutronMsg, query::NeutronQuery};

const DEPOSIT_DENOM: &str =
    "factory/neutron1k6hr0f83e7un2wjf29cspk7j69jrnskk65k3ek2nj9dztrlzpj6q00rtsa/udatom";
const WHITELIST_ADDR: &str = "whitelist1";
const INFLOW: &str = "inflow";
const CONTROL_CENTER: &str = "control_center";
pub const CONTROL_CENTER_ADDR: &str =
    "cosmwasm195ay4pn6v07zenrafuhm5mnkklsj7kxa7gaz9djc9gjmkp0ehayszlp362";
const TOKEN_INFO_PROVIDER: &str = "token_info_provider";
const USER1: &str = "user1";
const USER2: &str = "user2";
const USER3: &str = "user3";
pub const DEFAULT_DEPOSIT_CAP: Uint128 = Uint128::new(10000000);
const DATOM_DEFAULT_RATIO: Decimal = Decimal::raw(1_200_000_000_000_000_000u128); // 1.2 in 18 decimal places

pub fn mock_dependencies() -> OwnedDeps<MockStorage, MockApi, MockQuerier, NeutronQuery> {
    OwnedDeps {
        storage: MockStorage::default(),
        api: MockApi::default(),
        querier: MockQuerier::new(&[]),
        custom_query_type: PhantomData,
    }
}

pub fn get_message_info(api: &MockApi, sender: &str, funds: &[Coin]) -> MessageInfo {
    MessageInfo {
        sender: api.addr_make(sender),
        funds: funds.to_vec(),
    }
}

// Assuming dATOM as a deposit token
fn get_default_instantiate_msg(
    deposit_denom: &str,
    whitelist_addr: Addr,
    control_center_contract: Addr,
    token_info_provider_contract: Addr,
) -> InstantiateMsg {
    InstantiateMsg {
        deposit_denom: deposit_denom.to_string(),
        whitelist: vec![whitelist_addr.to_string()],
        subdenom: "hydro_inflow_udatom".to_string(),
        token_metadata: DenomMetadata {
            display: "hydro_inflow_datom".to_string(),
            exponent: 6,
            name: "Hydro Inflow dATOM".to_string(),
            description: "Hydro Inflow dATOM".to_string(),
            symbol: "hydro_inflow_datom".to_string(),
            uri: None,
            uri_hash: None,
        },
        control_center_contract: control_center_contract.to_string(),
        token_info_provider_contract: Some(token_info_provider_contract.to_string()),
        max_withdrawals_per_user: 10,
    }
}

fn set_vault_shares_denom(
    deps: &mut OwnedDeps<MemoryStorage, MockApi, MockQuerier, NeutronQuery>,
    vault_shares_denom_str: String,
) {
    CONFIG
        .update(
            &mut deps.storage,
            |mut config| -> Result<_, ContractError> {
                config.vault_shares_denom = vault_shares_denom_str;

                Ok(config)
            },
        )
        .unwrap();
}

#[test]
fn deposit_withdrawal_for_deployment_test() {
    let (mut deps, mut env) = (mock_dependencies(), mock_env());

    let vault_contract_addr = deps.api.addr_make(INFLOW);
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);
    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);

    env.contract.address = vault_contract_addr.clone();

    let instantiate_msg = get_default_instantiate_msg(
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );

    let info = get_message_info(&deps.api, "creator", &[]);
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    let vault_shares_denom_str: String =
        format!("factory/{vault_contract_addr}/hydro_inflow_udatom");

    set_vault_shares_denom(&mut deps, vault_shares_denom_str.clone());

    // Setup initial mocks: total pool value and shares issued are 0
    let wasm_querier = MockWasmQuerier::new(HashMap::from_iter([
        setup_control_center_mock(
            control_center_contract_addr.clone(),
            DEFAULT_DEPOSIT_CAP,
            Uint128::zero(),
            Uint128::zero(),
        ),
        setup_token_info_provider_mock(
            token_info_provider_contract_addr,
            DEPOSIT_DENOM.to_string(),
            DATOM_DEFAULT_RATIO,
        ),
    ]));

    let querier_for_deps = wasm_querier.clone();
    deps.querier
        .update_wasm(move |q| querier_for_deps.handler(q));

    let user1_deposit1 = Uint128::new(1000);
    let user1_expected_shares1 = Uint128::new(1200);

    let mut total_pool_value = 1200;
    let mut total_shares_issued_before = 0;
    let mut total_shares_issued_after = 1200;

    // User1 deposits 1000 tokens
    execute_deposit(
        &mut deps,
        &env,
        &wasm_querier,
        &vault_contract_addr,
        USER1,
        &vault_shares_denom_str,
        user1_deposit1,
        user1_expected_shares1,
        user1_expected_shares1,
        user1_deposit1,
        total_pool_value,
        total_shares_issued_before,
        total_shares_issued_after,
    );

    let user2_deposit1 = Uint128::new(3000);
    let user2_expected_shares1 = Uint128::new(3600);

    total_pool_value = 4800;
    total_shares_issued_before = 1200;
    total_shares_issued_after = 4800;

    // User2 deposits 3000 tokens
    execute_deposit(
        &mut deps,
        &env,
        &wasm_querier,
        &vault_contract_addr,
        USER2,
        &vault_shares_denom_str,
        user2_deposit1,
        user2_expected_shares1,
        user2_expected_shares1,
        user1_deposit1 + user2_deposit1,
        total_pool_value,
        total_shares_issued_before,
        total_shares_issued_after,
    );

    // Whitelisted address withdraws 4000 tokens for deployment
    let info = get_message_info(&deps.api, WHITELIST_ADDR, &[]);
    let withdrawal_amount = Uint128::new(4000);
    let withdraw_res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::WithdrawForDeployment {
            amount: withdrawal_amount,
        },
    )
    .unwrap();

    // Verify BankSend message for 4000 tokens
    let bank_msg = &withdraw_res.messages[0].msg;
    match bank_msg {
        CosmosMsg::Bank(BankMsg::Send { to_address, amount }) => {
            assert_eq!(to_address, &whitelist_addr.to_string());
            assert_eq!(amount[0].amount, withdrawal_amount);
            assert_eq!(amount[0].denom, DEPOSIT_DENOM);
        }
        _ => panic!("Expected BankMsg::Send"),
    }

    // Mock that the vault contract doesn't have any tokens left on its bank balance
    mock_address_balance(
        &mut deps,
        vault_contract_addr.as_ref(),
        DEPOSIT_DENOM,
        Uint128::zero(),
    );

    let user1_deposit2 = Uint128::new(2000);
    let user1_expected_shares2 = Uint128::new(2400);

    total_pool_value = 7200;
    total_shares_issued_before = 4800;
    total_shares_issued_after = 7200;

    // User1 deposits 2000 tokens
    execute_deposit(
        &mut deps,
        &env,
        &wasm_querier,
        &vault_contract_addr,
        USER1,
        &vault_shares_denom_str,
        user1_deposit2,
        user1_expected_shares2,
        user1_expected_shares1 + user1_expected_shares2,
        user1_deposit2,
        total_pool_value,
        total_shares_issued_before,
        total_shares_issued_after,
    );

    // Mock that the deployed funds earned 100 tokens. Then add 1000 tokens that User1 is about to deposit.
    total_pool_value = 8500;
    total_shares_issued_before = 7200;
    total_shares_issued_after = 8383;

    let user3_deposit1 = Uint128::new(1000);
    let user3_expected_shares1 = Uint128::new(1183);

    // User3 deposits 1000 tokens
    execute_deposit(
        &mut deps,
        &env,
        &wasm_querier,
        &vault_contract_addr,
        USER3,
        &vault_shares_denom_str,
        user3_deposit1,
        user3_expected_shares1,
        user3_expected_shares1,
        user1_deposit2 + user3_deposit1,
        total_pool_value,
        total_shares_issued_before,
        total_shares_issued_after,
    );

    // Whitelisted address withdraws additional 3000 tokens
    let info = get_message_info(&deps.api, WHITELIST_ADDR, &[]);
    let withdrawal_amount = Uint128::new(3000);
    let withdraw_res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::WithdrawForDeployment {
            amount: withdrawal_amount,
        },
    )
    .unwrap();

    // Verify BankSend message for 3000 tokens
    let bank_msg = &withdraw_res.messages[0].msg;
    match bank_msg {
        CosmosMsg::Bank(BankMsg::Send { to_address, amount }) => {
            assert_eq!(to_address, &whitelist_addr.to_string());
            assert_eq!(amount[0].amount, withdrawal_amount);
            assert_eq!(amount[0].denom, DEPOSIT_DENOM);
        }
        _ => panic!("Expected BankMsg::Send"),
    }

    // Mock that the vault contract doesn't have any tokens left on its bank balance
    mock_address_balance(
        &mut deps,
        vault_contract_addr.as_ref(),
        DEPOSIT_DENOM,
        Uint128::zero(),
    );

    // Mock that the deployed funds earned 200 more tokens. Then add 1000 tokens that User1 is about to deposit.
    total_pool_value = 9900;
    total_shares_issued_before = 8383;
    total_shares_issued_after = 9539;

    let user3_deposit2 = Uint128::new(1000);
    let user3_expected_shares2 = Uint128::new(1156);

    // User3 deposits 1000 tokens
    execute_deposit(
        &mut deps,
        &env,
        &wasm_querier,
        &vault_contract_addr,
        USER3,
        &vault_shares_denom_str,
        user3_deposit2,
        user3_expected_shares2,
        user3_expected_shares1 + user3_expected_shares2,
        user3_deposit2,
        total_pool_value,
        total_shares_issued_before,
        total_shares_issued_after,
    );
}

#[test]
fn deposit_mints_shares_for_on_behalf_recipient() {
    let (mut deps, mut env) = (mock_dependencies(), mock_env());

    let vault_contract_addr = deps.api.addr_make(INFLOW);
    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);

    env.contract.address = vault_contract_addr.clone();

    let instantiate_msg = get_default_instantiate_msg(
        DEPOSIT_DENOM,
        whitelist_addr,
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );
    let info = get_message_info(&deps.api, "creator", &[]);
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    let vault_shares_denom_str = format!("factory/{vault_contract_addr}/hydro_inflow_uatom");
    set_vault_shares_denom(&mut deps, vault_shares_denom_str.clone());

    // Setup initial mocks: total pool value and shares issued are 0
    let wasm_querier = MockWasmQuerier::new(HashMap::from_iter([
        setup_control_center_mock(
            control_center_contract_addr.clone(),
            DEFAULT_DEPOSIT_CAP,
            Uint128::zero(),
            Uint128::zero(),
        ),
        setup_token_info_provider_mock(
            token_info_provider_contract_addr.clone(),
            DEPOSIT_DENOM.to_string(),
            Decimal::one(),
        ),
    ]));

    let querier_for_deps = wasm_querier.clone();
    deps.querier
        .update_wasm(move |q| querier_for_deps.handler(q));

    let deposit_amount = Uint128::new(1_000);
    let expected_shares = Uint128::new(1_000);
    let beneficiary_addr = deps.api.addr_make(USER2);

    let total_pool_value = 1000;
    let total_shares_issued_before = 0;
    let total_shares_issued_after = 1000;

    execute_deposit_with_recipient(
        &mut deps,
        &env,
        &wasm_querier,
        &vault_contract_addr,
        USER1,
        Some(beneficiary_addr.as_str()),
        &vault_shares_denom_str,
        deposit_amount,
        expected_shares,
        expected_shares,
        deposit_amount,
        total_pool_value,
        total_shares_issued_before,
        total_shares_issued_after,
    );
}

#[test]
fn withdraw_pays_on_behalf_recipient() {
    let (mut deps, mut env) = (mock_dependencies(), mock_env());

    let vault_contract_addr = deps.api.addr_make(INFLOW);
    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);

    env.contract.address = vault_contract_addr.clone();

    let instantiate_msg = get_default_instantiate_msg(
        DEPOSIT_DENOM,
        whitelist_addr,
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );
    let info = get_message_info(&deps.api, "creator", &[]);
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    let vault_shares_denom_str = format!("factory/{vault_contract_addr}/hydro_inflow_uatom");
    set_vault_shares_denom(&mut deps, vault_shares_denom_str.clone());

    let wasm_querier = MockWasmQuerier::new(HashMap::from_iter([
        setup_control_center_mock(
            control_center_contract_addr.clone(),
            DEFAULT_DEPOSIT_CAP,
            Uint128::zero(),
            Uint128::zero(),
        ),
        setup_token_info_provider_mock(
            token_info_provider_contract_addr.clone(),
            DEPOSIT_DENOM.to_string(),
            Decimal::one(),
        ),
    ]));

    let querier_for_deps = wasm_querier.clone();
    deps.querier
        .update_wasm(move |q| querier_for_deps.handler(q));

    let deposit_amount = Uint128::new(1_000);
    let expected_shares = deposit_amount;
    let mut total_pool_value = deposit_amount.u128();
    let total_shares_issued_before = 0;
    let mut total_shares_issued_after = deposit_amount.u128();

    execute_deposit(
        &mut deps,
        &env,
        &wasm_querier,
        &vault_contract_addr,
        USER1,
        &vault_shares_denom_str,
        deposit_amount,
        expected_shares,
        expected_shares,
        deposit_amount,
        total_pool_value,
        total_shares_issued_before,
        total_shares_issued_after,
    );

    let beneficiary_addr = deps.api.addr_make(USER2);

    total_pool_value = 0;
    total_shares_issued_after = 0;

    execute_withdraw(
        &mut deps,
        &env,
        &wasm_querier,
        vault_contract_addr.as_ref(),
        USER1,
        Some(beneficiary_addr.as_str()),
        &vault_shares_denom_str,
        expected_shares,
        true,
        deposit_amount,
        Uint128::zero(),
        Uint128::zero(),
        total_pool_value,
        total_shares_issued_after,
    );

    verify_users_payouts_history(
        &deps,
        vec![(beneficiary_addr, vec![(expected_shares, deposit_amount)])],
    );
}

#[test]
fn withdraw_queue_uses_on_behalf_withdrawer() {
    let (mut deps, mut env) = (mock_dependencies(), mock_env());

    let vault_contract_addr = deps.api.addr_make(INFLOW);
    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);

    env.contract.address = vault_contract_addr.clone();

    let instantiate_msg = get_default_instantiate_msg(
        DEPOSIT_DENOM,
        whitelist_addr,
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );
    let info = get_message_info(&deps.api, "creator", &[]);
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    let vault_shares_denom_str = format!("factory/{vault_contract_addr}/hydro_inflow_uatom");
    set_vault_shares_denom(&mut deps, vault_shares_denom_str.clone());

    let wasm_querier = MockWasmQuerier::new(HashMap::from_iter([
        setup_control_center_mock(
            control_center_contract_addr.clone(),
            DEFAULT_DEPOSIT_CAP,
            Uint128::zero(),
            Uint128::zero(),
        ),
        setup_token_info_provider_mock(
            token_info_provider_contract_addr.clone(),
            DEPOSIT_DENOM.to_string(),
            Decimal::one(),
        ),
    ]));

    let querier_for_deps = wasm_querier.clone();
    deps.querier
        .update_wasm(move |q| querier_for_deps.handler(q));

    let deposit_amount = Uint128::new(500);
    let expected_shares = deposit_amount;
    let mut total_pool_value = deposit_amount.u128();
    let total_shares_issued_before = 0;
    let mut total_shares_issued_after = deposit_amount.u128();

    execute_deposit(
        &mut deps,
        &env,
        &wasm_querier,
        &vault_contract_addr,
        USER1,
        &vault_shares_denom_str,
        deposit_amount,
        expected_shares,
        expected_shares,
        deposit_amount,
        total_pool_value,
        total_shares_issued_before,
        total_shares_issued_after,
    );

    // Simulate that all funds have been deployed so withdrawals must queue.
    mock_address_balance(
        &mut deps,
        vault_contract_addr.as_ref(),
        DEPOSIT_DENOM,
        Uint128::zero(),
    );

    let beneficiary_addr = deps.api.addr_make(USER2);
    let sender_addr = deps.api.addr_make(USER1);

    total_pool_value = 0;
    total_shares_issued_after = 0;

    execute_withdraw(
        &mut deps,
        &env,
        &wasm_querier,
        vault_contract_addr.as_ref(),
        USER1,
        Some(beneficiary_addr.as_str()),
        &vault_shares_denom_str,
        expected_shares,
        false,
        Uint128::zero(),
        Uint128::zero(),
        Uint128::zero(),
        total_pool_value,
        total_shares_issued_after,
    );

    verify_user_withdrawal_requests(
        &deps,
        &beneficiary_addr,
        vec![(expected_shares, deposit_amount, false)],
    );
    verify_user_withdrawal_requests(&deps, &sender_addr, vec![]);
}

#[test]
fn withdrawal_test() {
    let (mut deps, mut env) = (mock_dependencies(), mock_env());

    let vault_contract_addr = deps.api.addr_make(INFLOW);
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);
    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let user1_addr = deps.api.addr_make(USER1);

    env.contract.address = vault_contract_addr.clone();

    let instantiate_msg = get_default_instantiate_msg(
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );

    let info = get_message_info(&deps.api, "creator", &[]);
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    let vault_shares_denom_str: String =
        format!("factory/{vault_contract_addr}/hydro_inflow_udatom");

    set_vault_shares_denom(&mut deps, vault_shares_denom_str.clone());

    // Setup initial mocks: total pool value and shares issued are 0
    let wasm_querier = MockWasmQuerier::new(HashMap::from_iter([
        setup_control_center_mock(
            control_center_contract_addr.clone(),
            DEFAULT_DEPOSIT_CAP,
            Uint128::zero(),
            Uint128::zero(),
        ),
        setup_token_info_provider_mock(
            token_info_provider_contract_addr,
            DEPOSIT_DENOM.to_string(),
            DATOM_DEFAULT_RATIO,
        ),
    ]));

    let querier_for_deps = wasm_querier.clone();
    deps.querier
        .update_wasm(move |q| querier_for_deps.handler(q));

    let mut total_pool_value = 1200;

    let mut total_shares_issued_before = 0;
    let mut total_shares_issued_after = 1200;

    let user1_deposit1 = Uint128::new(1000);
    let user1_expected_shares1 = Uint128::new(1200);

    // Have User1 deposit 1000 tokens
    execute_deposit(
        &mut deps,
        &env,
        &wasm_querier,
        &vault_contract_addr,
        USER1,
        &vault_shares_denom_str,
        user1_deposit1,
        user1_expected_shares1,
        user1_expected_shares1,
        user1_deposit1,
        total_pool_value,
        total_shares_issued_before,
        total_shares_issued_after,
    );

    total_pool_value = 3600;
    total_shares_issued_before = 1200;
    total_shares_issued_after = 3600;

    let user2_deposit1 = Uint128::new(2000);
    let user2_expected_shares1 = Uint128::new(2400);

    // Have User2 deposit 2000 tokens
    execute_deposit(
        &mut deps,
        &env,
        &wasm_querier,
        &vault_contract_addr,
        USER2,
        &vault_shares_denom_str,
        user2_deposit1,
        user2_expected_shares1,
        user2_expected_shares1,
        user1_deposit1 + user2_deposit1,
        total_pool_value,
        total_shares_issued_before,
        total_shares_issued_after,
    );

    assert_eq!(
        query_available_for_deployment(&deps.as_ref(), &env).unwrap(),
        user1_deposit1 + user2_deposit1
    );

    // User1 withdraws 600 shares. They should receive 500 deposit tokens.
    let user1_withdraw_shares_1 = Uint128::new(600);
    let user1_withdraw_tokens_1 = Uint128::new(500);

    total_pool_value = 3000;
    total_shares_issued_after = 3000;

    execute_withdraw(
        &mut deps,
        &env,
        &wasm_querier,
        vault_contract_addr.as_ref(),
        USER1,
        None,
        &vault_shares_denom_str,
        user1_withdraw_shares_1,
        true,
        user1_withdraw_tokens_1,
        user1_expected_shares1 - user1_withdraw_shares_1,
        Uint128::new(2500),
        total_pool_value,
        total_shares_issued_after,
    );

    // User1 withdraws additional 600 shares. They should receive 500 deposit tokens.

    total_pool_value = 2400;
    total_shares_issued_after = 2400;

    execute_withdraw(
        &mut deps,
        &env,
        &wasm_querier,
        vault_contract_addr.as_ref(),
        USER1,
        None,
        &vault_shares_denom_str,
        user1_withdraw_shares_1,
        true,
        user1_withdraw_tokens_1,
        Uint128::zero(),
        Uint128::new(2000),
        total_pool_value,
        total_shares_issued_after,
    );

    // User2 withdraws all 2400 shares at once. They should receive 2000 deposit tokens.

    total_pool_value = 0;
    total_shares_issued_after = 0;

    execute_withdraw(
        &mut deps,
        &env,
        &wasm_querier,
        vault_contract_addr.as_ref(),
        USER2,
        None,
        &vault_shares_denom_str,
        user2_expected_shares1,
        true,
        user2_deposit1,
        Uint128::zero(),
        Uint128::zero(),
        total_pool_value,
        total_shares_issued_after,
    );

    verify_withdrawal_queue_info(&deps, Uint128::zero(), Uint128::zero(), Uint128::zero());

    let user1_deposit2 = Uint128::new(15000);
    let user1_expected_shares2 = Uint128::new(18000);

    total_pool_value = 18000;
    total_shares_issued_before = 0;
    total_shares_issued_after = 18000;

    // Have User1 deposit 15000 tokens
    execute_deposit(
        &mut deps,
        &env,
        &wasm_querier,
        &vault_contract_addr,
        USER1,
        &vault_shares_denom_str,
        user1_deposit2,
        user1_expected_shares2,
        user1_expected_shares2,
        user1_deposit2,
        total_pool_value,
        total_shares_issued_before,
        total_shares_issued_after,
    );

    // Whitelisted address withdraws 15000 tokens for deployment
    let withdrawal_amount = Uint128::new(15000);
    execute_withdraw_for_deployment(
        &mut deps,
        &env,
        vault_contract_addr.as_ref(),
        WHITELIST_ADDR,
        withdrawal_amount,
        Uint128::zero(),
    );

    // User1 tries to withdraw 6000 shares. Contract doesn't have any tokens available,
    // so they enter the withdrawal queue.
    let user1_withdraw_shares_2 = Uint128::new(6000);
    let user1_withdraw_tokens_2 = Uint128::new(5000);

    total_pool_value = 12000;
    total_shares_issued_after = 12000;

    execute_withdraw(
        &mut deps,
        &env,
        &wasm_querier,
        vault_contract_addr.as_ref(),
        USER1,
        None,
        &vault_shares_denom_str,
        user1_withdraw_shares_2,
        false,
        Uint128::zero(),
        Uint128::new(10000),
        Uint128::zero(),
        total_pool_value,
        total_shares_issued_after,
    );

    verify_withdrawal_queue_info(
        &deps,
        user1_withdraw_shares_2,
        user1_withdraw_tokens_2,
        user1_withdraw_tokens_2,
    );

    assert_eq!(
        query_available_for_deployment(&deps.as_ref(), &env).unwrap(),
        Uint128::zero()
    );

    let user2_deposit2 = Uint128::new(4000);
    let user2_shares2 = Uint128::new(4800);

    total_pool_value = 16800;
    total_shares_issued_before = 12000;
    total_shares_issued_after = 16800;

    // Have User2 deposit 4000 tokens
    execute_deposit(
        &mut deps,
        &env,
        &wasm_querier,
        &vault_contract_addr,
        USER2,
        &vault_shares_denom_str,
        user2_deposit2,
        user2_shares2,
        user2_shares2,
        user2_deposit2,
        total_pool_value,
        total_shares_issued_before,
        total_shares_issued_after,
    );

    // Amount available for deployment remains zero, since User1 has pending
    // withdrawal worth 5000 tokens, and User2 deposited only 4000 tokens.
    assert_eq!(
        query_available_for_deployment(&deps.as_ref(), &env).unwrap(),
        Uint128::zero()
    );

    verify_user_withdrawal_requests(
        &deps,
        &user1_addr,
        vec![(user1_withdraw_shares_2, user1_withdraw_tokens_2, false)],
    );

    let user2_deposit3 = Uint128::new(4000);
    let user2_shares3 = Uint128::new(4800);

    total_pool_value = 21600;
    total_shares_issued_before = 16800;
    total_shares_issued_after = 21600;

    // Have User2 deposit additional 4000 tokens that will cover User1's
    // pending withdrawal and leave 3000 tokens available for deployment.
    execute_deposit(
        &mut deps,
        &env,
        &wasm_querier,
        &vault_contract_addr,
        USER2,
        &vault_shares_denom_str,
        user2_deposit3,
        user2_shares3,
        user2_shares2 + user2_shares3,
        user2_deposit2 + user2_deposit3,
        total_pool_value,
        total_shares_issued_before,
        total_shares_issued_after,
    );

    // Verify that 3000 tokens are now available for deployment
    assert_eq!(
        query_available_for_deployment(&deps.as_ref(), &env).unwrap(),
        Uint128::new(3000),
    );

    // User1 initiates withdrawal of 2400 shares. Since User2 deposited enough tokens to cover both
    // User1's previous and new withdrawal request, the new request should be paid out immediately.
    let user1_withdraw_shares_3 = Uint128::new(2400);
    let user1_withdraw_tokens_3 = Uint128::new(2000);

    total_pool_value = 19200;
    total_shares_issued_after = 19200;

    execute_withdraw(
        &mut deps,
        &env,
        &wasm_querier,
        vault_contract_addr.as_ref(),
        USER1,
        None,
        &vault_shares_denom_str,
        user1_withdraw_shares_3,
        true,
        user1_withdraw_tokens_3,
        Uint128::new(8000),
        Uint128::new(6000),
        total_pool_value,
        total_shares_issued_after,
    );

    // Verify that User1 has one (old) remaining withdrawal request in the queue
    verify_withdrawal_queue_info(
        &deps,
        user1_withdraw_shares_2,
        user1_withdraw_tokens_2,
        user1_withdraw_tokens_2,
    );

    // Now the contract has 5000 tokens reserved for User1's withdrawal request,
    // so the amount available for deployment is 1000 tokens.
    assert_eq!(
        query_available_for_deployment(&deps.as_ref(), &env).unwrap(),
        Uint128::new(1000)
    );
}

#[test]
fn cancel_withdrawal_test() {
    let (mut deps, mut env) = (mock_dependencies(), mock_env());

    let vault_contract_addr = deps.api.addr_make(INFLOW);
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);
    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let user1_addr = deps.api.addr_make(USER1);
    let user2_addr = deps.api.addr_make(USER2);

    env.contract.address = vault_contract_addr.clone();

    let instantiate_msg = get_default_instantiate_msg(
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );

    let info = get_message_info(&deps.api, "creator", &[]);
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    let vault_shares_denom_str: String =
        format!("factory/{vault_contract_addr}/hydro_inflow_udatom");

    set_vault_shares_denom(&mut deps, vault_shares_denom_str.clone());

    // Setup initial mocks: total pool value and shares issued are 0
    let wasm_querier = MockWasmQuerier::new(HashMap::from_iter([
        setup_control_center_mock(
            control_center_contract_addr.clone(),
            DEFAULT_DEPOSIT_CAP,
            Uint128::zero(),
            Uint128::zero(),
        ),
        setup_token_info_provider_mock(
            token_info_provider_contract_addr,
            DEPOSIT_DENOM.to_string(),
            DATOM_DEFAULT_RATIO,
        ),
    ]));

    let querier_for_deps = wasm_querier.clone();
    deps.querier
        .update_wasm(move |q| querier_for_deps.handler(q));

    let user1_deposit1 = Uint128::new(1000);
    let user1_deposit_shares1 = Uint128::new(1200);

    let mut total_pool_value = 1200;
    let mut total_shares_issued_before = 0;
    let mut total_shares_issued_after = 1200;

    // Have User1 deposit 1000 tokens
    execute_deposit(
        &mut deps,
        &env,
        &wasm_querier,
        &vault_contract_addr,
        USER1,
        &vault_shares_denom_str,
        user1_deposit1,
        user1_deposit_shares1,
        user1_deposit_shares1,
        user1_deposit1,
        total_pool_value,
        total_shares_issued_before,
        total_shares_issued_after,
    );

    // Whitelisted address withdraws all tokens for deployment
    execute_withdraw_for_deployment(
        &mut deps,
        &env,
        vault_contract_addr.as_ref(),
        WHITELIST_ADDR,
        user1_deposit1,
        Uint128::zero(),
    );

    // User1 tries to withdraw 1200 shares. Contract doesn't have any tokens available,
    // so they enter the withdrawal queue (withdrawal request ID = 0).
    total_pool_value = 0;
    total_shares_issued_after = 0;

    execute_withdraw(
        &mut deps,
        &env,
        &wasm_querier,
        vault_contract_addr.as_ref(),
        USER1,
        None,
        &vault_shares_denom_str,
        user1_deposit_shares1,
        false,
        Uint128::zero(),
        Uint128::zero(),
        Uint128::zero(),
        total_pool_value,
        total_shares_issued_after,
    );

    verify_user_withdrawal_requests(
        &deps,
        &user1_addr,
        vec![(user1_deposit_shares1, user1_deposit1, false)],
    );

    // User1 cancels their withdrawal request
    total_pool_value = 1200;
    total_shares_issued_after = 1200;

    execute_cancel_withdrawal(
        &mut deps,
        &env,
        &wasm_querier,
        USER1,
        vec![0u64],
        &vault_shares_denom_str,
        Some((user1_deposit_shares1, user1_deposit_shares1)),
        total_pool_value,
        total_shares_issued_after,
    );

    // Verify that User1 has no remaining withdrawal requests
    verify_user_withdrawal_requests(&deps, &user1_addr, vec![]);

    // Have User2 deposit 500 tokens
    let user2_deposit1 = Uint128::new(500);
    let user2_deposit_shares1 = Uint128::new(600);

    total_pool_value = 1800;
    total_shares_issued_before = 1200;
    total_shares_issued_after = 1800;

    execute_deposit(
        &mut deps,
        &env,
        &wasm_querier,
        &vault_contract_addr,
        USER2,
        &vault_shares_denom_str,
        user2_deposit1,
        user2_deposit_shares1,
        user2_deposit_shares1,
        user2_deposit1,
        total_pool_value,
        total_shares_issued_before,
        total_shares_issued_after,
    );

    // Whitelisted address withdraws all tokens for deployment
    execute_withdraw_for_deployment(
        &mut deps,
        &env,
        vault_contract_addr.as_ref(),
        WHITELIST_ADDR,
        user2_deposit1,
        Uint128::zero(),
    );

    // Have User1 create 3 withdrawal requests with 240 shares each
    let user1_withdraw_shares = Uint128::new(240);
    let user1_withdraw_tokens = Uint128::new(200);
    let mut user1_shares_after = user1_deposit_shares1;

    for _ in 0..3 {
        user1_shares_after = user1_shares_after
            .checked_sub(user1_withdraw_shares)
            .unwrap();

        total_pool_value -= user1_withdraw_shares.u128();
        total_shares_issued_after -= user1_withdraw_shares.u128();

        execute_withdraw(
            &mut deps,
            &env,
            &wasm_querier,
            vault_contract_addr.as_ref(),
            USER1,
            None,
            &vault_shares_denom_str,
            user1_withdraw_shares,
            false,
            Uint128::zero(),
            user1_shares_after,
            Uint128::zero(),
            total_pool_value,
            total_shares_issued_after,
        );
    }

    verify_user_withdrawal_requests(
        &deps,
        &user1_addr,
        vec![
            (user1_withdraw_shares, user1_withdraw_tokens, false),
            (user1_withdraw_shares, user1_withdraw_tokens, false),
            (user1_withdraw_shares, user1_withdraw_tokens, false),
        ],
    );

    // Have User2 create 2 withdrawal requests with 300 shares each
    let user2_withdraw_shares = Uint128::new(300);
    let user2_withdraw_tokens = Uint128::new(250);
    let mut user2_shares_after = user2_deposit_shares1;

    for _ in 0..2 {
        user2_shares_after = user2_shares_after
            .checked_sub(user2_withdraw_shares)
            .unwrap();

        total_pool_value -= user2_withdraw_shares.u128();
        total_shares_issued_after -= user2_withdraw_shares.u128();

        execute_withdraw(
            &mut deps,
            &env,
            &wasm_querier,
            vault_contract_addr.as_ref(),
            USER2,
            None,
            &vault_shares_denom_str,
            user2_withdraw_shares,
            false,
            Uint128::zero(),
            user2_shares_after,
            Uint128::zero(),
            total_pool_value,
            total_shares_issued_after,
        );
    }

    verify_user_withdrawal_requests(
        &deps,
        &user2_addr,
        vec![
            (user2_withdraw_shares, user2_withdraw_tokens, false),
            (user2_withdraw_shares, user2_withdraw_tokens, false),
        ],
    );

    verify_withdrawal_queue_info(
        &deps,
        Uint128::new(1320),
        Uint128::new(1100),
        Uint128::new(1100),
    );

    // Mock as if the last funded withdrawal ID is 1
    LAST_FUNDED_WITHDRAWAL_ID
        .save(deps.as_mut().storage, &1u64)
        .unwrap();

    // User1 tries to cancel:
    // - withdrawal request ID 0 (should be skipped, doesn't exist)
    // - withdrawal request ID 1 (should be skipped, already funded)
    // - withdrawal request ID 2 (should succeed, not funded yet)
    // - withdrawal request ID 2 (should be filtered out, duplicate)
    // - withdrawal request ID 3 (should succeed, not funded yet)
    // - withdrawal request ID 4 (should be skipped, belongs to User2)
    total_pool_value = 480;
    total_shares_issued_after = 480;

    execute_cancel_withdrawal(
        &mut deps,
        &env,
        &wasm_querier,
        USER1,
        vec![0, 1, 2, 2, 3, 4],
        &vault_shares_denom_str,
        Some((
            user1_withdraw_shares.checked_mul(Uint128::new(2)).unwrap(),
            Uint128::new(600),
        )),
        total_pool_value,
        total_shares_issued_after,
    );

    // Verify that User1 has one remaining withdrawal request
    verify_user_withdrawal_requests(
        &deps,
        &user1_addr,
        vec![(user1_withdraw_shares, user1_withdraw_tokens, false)],
    );

    // Verify that User2's withdrawal requests are unaffected
    verify_user_withdrawal_requests(
        &deps,
        &user2_addr,
        vec![
            (user2_withdraw_shares, user2_withdraw_tokens, false),
            (user2_withdraw_shares, user2_withdraw_tokens, false),
        ],
    );

    // Verify that the withdrawal queue info is updated correctly
    verify_withdrawal_queue_info(
        &deps,
        Uint128::new(840),
        Uint128::new(700),
        Uint128::new(700),
    );
}

#[test]
fn fulfill_pending_withdrawals_test() {
    let (mut deps, mut env) = (mock_dependencies(), mock_env());

    let vault_contract_addr = deps.api.addr_make(INFLOW);
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);
    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let user1_addr = deps.api.addr_make(USER1);
    let user2_addr = deps.api.addr_make(USER2);

    env.contract.address = vault_contract_addr.clone();

    let instantiate_msg = get_default_instantiate_msg(
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );

    let info = get_message_info(&deps.api, "creator", &[]);

    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    let vault_shares_denom_str: String =
        format!("factory/{vault_contract_addr}/hydro_inflow_udatom");

    set_vault_shares_denom(&mut deps, vault_shares_denom_str.clone());

    // Setup initial mocks: total pool value and shares issued are 0
    let wasm_querier = MockWasmQuerier::new(HashMap::from_iter([
        setup_control_center_mock(
            control_center_contract_addr.clone(),
            DEFAULT_DEPOSIT_CAP,
            Uint128::zero(),
            Uint128::zero(),
        ),
        setup_token_info_provider_mock(
            token_info_provider_contract_addr,
            DEPOSIT_DENOM.to_string(),
            DATOM_DEFAULT_RATIO,
        ),
    ]));

    let querier_for_deps = wasm_querier.clone();
    deps.querier
        .update_wasm(move |q| querier_for_deps.handler(q));

    let mut total_pool_value = 0;
    let mut total_shares_issued_before = 0;
    let mut total_shares_issued_after = 0;

    // User1 deposits 10000 tokens
    // User2 deposits 20000 tokens
    let user1_deposit1 = Uint128::new(10000);
    let user1_deposit_shares1 = Uint128::new(12000);
    let user2_deposit1 = Uint128::new(20000);
    let user2_deposit_shares1 = Uint128::new(24000);

    let mut mock_inflow_balance_total = Uint128::zero();
    for user_deposit in &[
        (USER1, user1_deposit1, user1_deposit_shares1),
        (USER2, user2_deposit1, user2_deposit_shares1),
    ] {
        mock_inflow_balance_total = mock_inflow_balance_total
            .checked_add(user_deposit.1)
            .unwrap();

        total_pool_value += user_deposit.2.u128();
        total_shares_issued_after += user_deposit.2.u128();

        execute_deposit(
            &mut deps,
            &env,
            &wasm_querier,
            &vault_contract_addr,
            user_deposit.0,
            &vault_shares_denom_str,
            user_deposit.1,
            user_deposit.2,
            user_deposit.2,
            mock_inflow_balance_total,
            total_pool_value,
            total_shares_issued_before,
            total_shares_issued_after,
        );

        total_shares_issued_before = total_shares_issued_after;
    }

    // Whitelisted address withdraws all 30000 tokens for deployment
    execute_withdraw_for_deployment(
        &mut deps,
        &env,
        vault_contract_addr.as_ref(),
        WHITELIST_ADDR,
        user1_deposit1 + user2_deposit1,
        Uint128::zero(),
    );

    // User1 requests withdrawal of 3600 shares (enters queue)
    // User1 requests withdrawal of 4800 shares (enters queue)
    // User2 requests withdrawal of 6000 shares (enters queue)
    let user1_withdrawal_shares1 = Uint128::new(3600);
    let user1_withdrawal_shares2 = Uint128::new(4800);
    let user2_withdrawal_shares1 = Uint128::new(6000);

    for user_withdrawal in &[
        (
            USER1,
            user1_withdrawal_shares1,
            user1_deposit_shares1 - user1_withdrawal_shares1,
        ),
        (
            USER1,
            user1_withdrawal_shares2,
            user1_deposit_shares1 - user1_withdrawal_shares1 - user1_withdrawal_shares2,
        ),
        (
            USER2,
            user2_withdrawal_shares1,
            user2_deposit_shares1 - user2_withdrawal_shares1,
        ),
    ] {
        total_pool_value -= user_withdrawal.1.u128();
        total_shares_issued_after -= user_withdrawal.1.u128();

        execute_withdraw(
            &mut deps,
            &env,
            &wasm_querier,
            vault_contract_addr.as_ref(),
            user_withdrawal.0,
            None,
            &vault_shares_denom_str,
            user_withdrawal.1,
            false,
            Uint128::zero(),
            user_withdrawal.2,
            Uint128::zero(),
            total_pool_value,
            total_shares_issued_after,
        );
    }

    // Verify withdrawal requests for both users and withdrawal queue info
    verify_user_withdrawal_requests(
        &deps,
        &user1_addr,
        vec![
            (user1_withdrawal_shares1, Uint128::new(3000), false),
            (user1_withdrawal_shares2, Uint128::new(4000), false),
        ],
    );
    verify_user_withdrawal_requests(
        &deps,
        &user2_addr,
        vec![(user2_withdrawal_shares1, Uint128::new(5000), false)],
    );

    verify_withdrawal_queue_info(
        &deps,
        user1_withdrawal_shares1 + user1_withdrawal_shares2 + user2_withdrawal_shares1,
        Uint128::new(12000),
        Uint128::new(12000),
    );

    // User3 deposits 3000 tokens
    let user3_deposit1 = Uint128::new(3000);
    let user3_deposit_shares1 = Uint128::new(3600);

    total_pool_value = 25200;
    total_shares_issued_before = 21600;
    total_shares_issued_after = 25200;

    execute_deposit(
        &mut deps,
        &env,
        &wasm_querier,
        &vault_contract_addr,
        USER3,
        &vault_shares_denom_str,
        user3_deposit1,
        user3_deposit_shares1,
        user3_deposit_shares1,
        user3_deposit1,
        total_pool_value,
        total_shares_issued_before,
        total_shares_issued_after,
    );

    // Verify how much is needed to fulfill pending withdrawals (9000 tokens)
    assert_eq!(
        query_amount_to_fund_pending_withdrawals(&deps.as_ref(), &env).unwrap(),
        Uint128::new(9000)
    );

    // Provide 9000 extra tokens to the contract for all pending withdrawals to be fulfilled
    execute_return_from_deployment(&mut deps, vault_contract_addr.as_str(), Uint128::new(9000));

    // Execute fulfillment of pending withdrawals
    execute_fulfill_pending_withdrawals(&mut deps, &env, WHITELIST_ADDR);

    // Verify withdrawal requests for both users and withdrawal queue info
    verify_user_withdrawal_requests(
        &deps,
        &user1_addr,
        vec![
            (user1_withdrawal_shares1, Uint128::new(3000), true),
            (user1_withdrawal_shares2, Uint128::new(4000), true),
        ],
    );
    verify_user_withdrawal_requests(
        &deps,
        &user2_addr,
        vec![(user2_withdrawal_shares1, Uint128::new(5000), true)],
    );

    verify_withdrawal_queue_info(
        &deps,
        user1_withdrawal_shares1 + user1_withdrawal_shares2 + user2_withdrawal_shares1,
        Uint128::new(12000),
        Uint128::zero(),
    );

    // User1 requests withdrawal of 3600 shares (enters queue)
    // User2 requests withdrawal of 6000 shares (enters queue)
    let user1_withdrawal_shares3 = Uint128::new(3600);
    let user2_withdrawal_shares2 = Uint128::new(6000);

    for user_withdrawal in &[
        (USER1, user1_withdrawal_shares3, Uint128::zero()),
        (
            USER2,
            user2_withdrawal_shares2,
            user2_deposit_shares1 - user2_withdrawal_shares1 - user2_withdrawal_shares2,
        ),
    ] {
        total_pool_value -= user_withdrawal.1.u128();
        total_shares_issued_after -= user_withdrawal.1.u128();

        execute_withdraw(
            &mut deps,
            &env,
            &wasm_querier,
            vault_contract_addr.as_ref(),
            user_withdrawal.0,
            None,
            &vault_shares_denom_str,
            user_withdrawal.1,
            false,
            Uint128::zero(),
            user_withdrawal.2,
            Uint128::zero(),
            total_pool_value,
            total_shares_issued_after,
        );
    }

    // Verify withdrawal requests for both users and withdrawal queue info
    verify_user_withdrawal_requests(
        &deps,
        &user1_addr,
        vec![
            (user1_withdrawal_shares1, Uint128::new(3000), true),
            (user1_withdrawal_shares2, Uint128::new(4000), true),
            (user1_withdrawal_shares3, Uint128::new(3000), false),
        ],
    );
    verify_user_withdrawal_requests(
        &deps,
        &user2_addr,
        vec![
            (user2_withdrawal_shares1, Uint128::new(5000), true),
            (user2_withdrawal_shares2, Uint128::new(5000), false),
        ],
    );

    let total_withdrawal_queue_shares = user1_withdrawal_shares1
        + user1_withdrawal_shares2
        + user1_withdrawal_shares3
        + user2_withdrawal_shares1
        + user2_withdrawal_shares2;

    verify_withdrawal_queue_info(
        &deps,
        total_withdrawal_queue_shares,
        Uint128::new(20000),
        Uint128::new(8000),
    );

    // Verify how much is needed to fulfill pending withdrawals (8000 tokens)
    assert_eq!(
        query_amount_to_fund_pending_withdrawals(&deps.as_ref(), &env).unwrap(),
        Uint128::new(8000)
    );

    // Provide 7000 tokens (on top of existing 12000) to the contract for User1's pending withdrawal to be fulfilled
    execute_return_from_deployment(&mut deps, vault_contract_addr.as_str(), Uint128::new(7000));

    // User1 executes claim_unbonded_withdrawals() for withdrawal request ID 1
    execute_claim_unbonded_withdrawals(
        &mut deps,
        &env,
        vault_contract_addr.as_ref(),
        USER1,
        vec![1],
        None,
        vec![(user1_addr.clone(), Uint128::new(4000))],
        Uint128::new(15000),
    );

    // Execute fulfillment of pending withdrawals
    execute_fulfill_pending_withdrawals(&mut deps, &env, WHITELIST_ADDR);

    // Verify withdrawal requests for both users and withdrawal queue info
    verify_user_withdrawal_requests(
        &deps,
        &user1_addr,
        vec![
            (user1_withdrawal_shares1, Uint128::new(3000), true),
            (user1_withdrawal_shares3, Uint128::new(3000), true),
        ],
    );

    // 7000 tokens were provided, which isn't enough to fulfill User2's withdrawal request
    // so the second withdrawal request will not be marked as funded
    verify_user_withdrawal_requests(
        &deps,
        &user2_addr,
        vec![
            (user2_withdrawal_shares1, Uint128::new(5000), true),
            (user2_withdrawal_shares2, Uint128::new(5000), false),
        ],
    );

    let total_withdrawal_queue_shares = user1_withdrawal_shares1
        + user1_withdrawal_shares3
        + user2_withdrawal_shares1
        + user2_withdrawal_shares2;

    verify_withdrawal_queue_info(
        &deps,
        total_withdrawal_queue_shares,
        Uint128::new(16000),
        Uint128::new(5000),
    );
}

#[test]
fn claim_unbonded_withdrawals_test() {
    let (mut deps, mut env) = (mock_dependencies(), mock_env());

    let vault_contract_addr = deps.api.addr_make(INFLOW);
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);
    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let user1_addr = deps.api.addr_make(USER1);
    let user2_addr = deps.api.addr_make(USER2);

    env.contract.address = vault_contract_addr.clone();

    let instantiate_msg = get_default_instantiate_msg(
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );

    let info = get_message_info(&deps.api, "creator", &[]);

    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    let vault_shares_denom_str: String =
        format!("factory/{vault_contract_addr}/hydro_inflow_udatom");

    set_vault_shares_denom(&mut deps, vault_shares_denom_str.clone());

    // Setup initial mocks: total pool value and shares issued are 0
    let wasm_querier = MockWasmQuerier::new(HashMap::from_iter([
        setup_control_center_mock(
            control_center_contract_addr.clone(),
            DEFAULT_DEPOSIT_CAP,
            Uint128::zero(),
            Uint128::zero(),
        ),
        setup_token_info_provider_mock(
            token_info_provider_contract_addr,
            DEPOSIT_DENOM.to_string(),
            DATOM_DEFAULT_RATIO,
        ),
    ]));

    let querier_for_deps = wasm_querier.clone();
    deps.querier
        .update_wasm(move |q| querier_for_deps.handler(q));

    let mut total_pool_value = 0;
    let mut total_shares_issued_before = 0;
    let mut total_shares_issued_after = 0;

    // User1 deposits 10000 tokens
    // User2 deposits 20000 tokens
    let user1_deposit1 = Uint128::new(10000);
    let user1_deposit_shares1 = Uint128::new(12000);
    let user2_deposit1 = Uint128::new(20000);
    let user2_deposit_shares1 = Uint128::new(24000);

    let mut mock_inflow_balance_total = Uint128::zero();
    for user_deposit in &[
        (USER1, user1_deposit1, user1_deposit_shares1),
        (USER2, user2_deposit1, user2_deposit_shares1),
    ] {
        mock_inflow_balance_total = mock_inflow_balance_total
            .checked_add(user_deposit.1)
            .unwrap();

        total_pool_value += user_deposit.2.u128();
        total_shares_issued_after += user_deposit.2.u128();

        execute_deposit(
            &mut deps,
            &env,
            &wasm_querier,
            &vault_contract_addr,
            user_deposit.0,
            &vault_shares_denom_str,
            user_deposit.1,
            user_deposit.2,
            user_deposit.2,
            mock_inflow_balance_total,
            total_pool_value,
            total_shares_issued_before,
            total_shares_issued_after,
        );

        total_shares_issued_before = total_shares_issued_after;
    }

    // Whitelisted address withdraws 30000 tokens for deployment
    execute_withdraw_for_deployment(
        &mut deps,
        &env,
        vault_contract_addr.as_ref(),
        WHITELIST_ADDR,
        user1_deposit1 + user2_deposit1,
        Uint128::zero(),
    );

    // User1 requests withdrawal of 1200 shares (enters queue)
    // User1 requests withdrawal of 2400 shares (enters queue)
    // User2 requests withdrawal of 2400 shares (enters queue)
    // User2 requests withdrawal of 4800 shares (enters queue)
    // User1 requests withdrawal of 3600 shares (enters queue)
    // User2 requests withdrawal of 7200 shares (enters queue)
    let user1_withdrawal_shares1 = Uint128::new(1200);
    let user1_withdrawal_shares2 = Uint128::new(2400);
    let user1_withdrawal_shares3 = Uint128::new(3600);
    let user2_withdrawal_shares1 = Uint128::new(2400);
    let user2_withdrawal_shares2 = Uint128::new(4800);
    let user2_withdrawal_shares3 = Uint128::new(7200);

    for user_withdrawal in &[
        (
            USER1,
            user1_withdrawal_shares1,
            user1_deposit_shares1 - user1_withdrawal_shares1,
        ),
        (
            USER1,
            user1_withdrawal_shares2,
            user1_deposit_shares1 - user1_withdrawal_shares1 - user1_withdrawal_shares2,
        ),
        (
            USER2,
            user2_withdrawal_shares1,
            user2_deposit_shares1 - user2_withdrawal_shares1,
        ),
        (
            USER2,
            user2_withdrawal_shares2,
            user2_deposit_shares1 - user2_withdrawal_shares1 - user2_withdrawal_shares2,
        ),
        (
            USER1,
            user1_withdrawal_shares3,
            user1_deposit_shares1
                - user1_withdrawal_shares1
                - user1_withdrawal_shares2
                - user1_withdrawal_shares3,
        ),
        (
            USER2,
            user2_withdrawal_shares3,
            user2_deposit_shares1
                - user2_withdrawal_shares1
                - user2_withdrawal_shares2
                - user2_withdrawal_shares3,
        ),
    ] {
        total_pool_value -= user_withdrawal.1.u128();
        total_shares_issued_before -= user_withdrawal.1.u128();
        total_shares_issued_after -= user_withdrawal.1.u128();

        execute_withdraw(
            &mut deps,
            &env,
            &wasm_querier,
            vault_contract_addr.as_ref(),
            user_withdrawal.0,
            None,
            &vault_shares_denom_str,
            user_withdrawal.1,
            false,
            Uint128::zero(),
            user_withdrawal.2,
            Uint128::zero(),
            total_pool_value,
            total_shares_issued_after,
        );
    }

    // Verify withdrawal requests for both users and withdrawal queue info
    verify_user_withdrawal_requests(
        &deps,
        &user1_addr,
        vec![
            (user1_withdrawal_shares1, Uint128::new(1000), false), // ID = 0
            (user1_withdrawal_shares2, Uint128::new(2000), false), // ID = 1
            (user1_withdrawal_shares3, Uint128::new(3000), false), // ID = 4
        ],
    );
    verify_user_withdrawal_requests(
        &deps,
        &user2_addr,
        vec![
            (user2_withdrawal_shares1, Uint128::new(2000), false), // ID = 2
            (user2_withdrawal_shares2, Uint128::new(4000), false), // ID = 3
            (user2_withdrawal_shares3, Uint128::new(6000), false), // ID = 5
        ],
    );

    verify_withdrawal_queue_info(
        &deps,
        user1_withdrawal_shares1
            + user1_withdrawal_shares2
            + user1_withdrawal_shares3
            + user2_withdrawal_shares1
            + user2_withdrawal_shares2
            + user2_withdrawal_shares3,
        Uint128::new(18000),
        Uint128::new(18000),
    );

    // Provide 9000 tokens to the contract to fulfill first 4 withdrawal requests
    execute_return_from_deployment(&mut deps, vault_contract_addr.as_str(), Uint128::new(9000));

    // Execute fulfillment of 4 pending withdrawals
    execute_fulfill_pending_withdrawals(&mut deps, &env, WHITELIST_ADDR);

    // Verify withdrawal requests for both users and withdrawal queue info
    verify_user_withdrawal_requests(
        &deps,
        &user1_addr,
        vec![
            (user1_withdrawal_shares1, Uint128::new(1000), true), // ID = 0
            (user1_withdrawal_shares2, Uint128::new(2000), true), // ID = 1
            (user1_withdrawal_shares3, Uint128::new(3000), false), // ID = 4
        ],
    );
    verify_user_withdrawal_requests(
        &deps,
        &user2_addr,
        vec![
            (user2_withdrawal_shares1, Uint128::new(2000), true), // ID = 2
            (user2_withdrawal_shares2, Uint128::new(4000), true), // ID = 3
            (user2_withdrawal_shares3, Uint128::new(6000), false), // ID = 5
        ],
    );

    verify_withdrawal_queue_info(
        &deps,
        user1_withdrawal_shares1
            + user1_withdrawal_shares2
            + user1_withdrawal_shares3
            + user2_withdrawal_shares1
            + user2_withdrawal_shares2
            + user2_withdrawal_shares3,
        Uint128::new(18000),
        Uint128::new(9000),
    );

    // Try to claim invalid withdrawals (e.g. non-existing IDs, or the ones that weren't funded, duplicates)
    // and verify that the correct error is returned.
    execute_claim_unbonded_withdrawals(
        &mut deps,
        &env,
        vault_contract_addr.as_ref(),
        USER1,
        vec![4, 5, 4, 9],
        Some("must provide at least one valid withdrawal id"),
        vec![],
        Uint128::zero(),
    );

    // Execute claim for withdrawal requests 0, 1 and 2, but also provide duplicate IDs and non-funded IDs
    // and verify that those are filtered out properly
    execute_claim_unbonded_withdrawals(
        &mut deps,
        &env,
        vault_contract_addr.as_ref(),
        USER1,
        vec![0, 1, 2, 0, 2, 4, 5],
        None,
        vec![
            (user1_addr.clone(), Uint128::new(3000)),
            (user2_addr.clone(), Uint128::new(2000)),
        ],
        Uint128::zero(),
    );

    // Verify withdrawal requests for both users and withdrawal queue info
    verify_user_withdrawal_requests(
        &deps,
        &user1_addr,
        vec![
            (user1_withdrawal_shares3, Uint128::new(3000), false), // ID = 4
        ],
    );
    verify_user_withdrawal_requests(
        &deps,
        &user2_addr,
        vec![
            (user2_withdrawal_shares2, Uint128::new(4000), true), // ID = 3
            (user2_withdrawal_shares3, Uint128::new(6000), false), // ID = 5
        ],
    );

    verify_withdrawal_queue_info(
        &deps,
        user1_withdrawal_shares3 + user2_withdrawal_shares2 + user2_withdrawal_shares3,
        Uint128::new(13000),
        Uint128::new(9000),
    );

    // Verify payout history for both users
    verify_users_payouts_history(
        &deps,
        vec![
            (
                user1_addr.clone(),
                vec![(Uint128::new(3600), Uint128::new(3000))],
            ),
            (
                user2_addr.clone(),
                vec![(Uint128::new(2400), Uint128::new(2000))],
            ),
        ],
    )
}

#[test]
fn whitelist_add_remove_test() {
    let (mut deps, mut env) = (mock_dependencies(), mock_env());

    let vault_contract_addr = deps.api.addr_make(INFLOW);
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);
    let user1_address = deps.api.addr_make(USER1);
    let user2_address = deps.api.addr_make(USER2);
    let user3_address = deps.api.addr_make(USER3);

    env.contract.address = vault_contract_addr.clone();

    let instantiate_msg = get_default_instantiate_msg(
        DEPOSIT_DENOM,
        user1_address.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );

    let info = get_message_info(&deps.api, "creator", &[]);
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    // Have a non-whitelisted address try to add new address to the whitelist
    let info = get_message_info(&deps.api, USER2, &[]);

    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::AddToWhitelist {
            address: user2_address.to_string(),
        },
    )
    .unwrap_err()
    .to_string()
    .contains("Unauthorized");

    // Have a whitelisted address add a new address to the whitelist
    let info = get_message_info(&deps.api, USER1, &[]);

    let res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::AddToWhitelist {
            address: user2_address.to_string(),
        },
    );
    assert!(res.is_ok());

    // Have a whitelisted address try to add new address to the whitelist when that address is already in the list
    let info = get_message_info(&deps.api, USER1, &[]);

    let res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::AddToWhitelist {
            address: user2_address.to_string(),
        },
    );
    assert!(res.unwrap_err().to_string().contains(&format!(
        "address {user2_address} is already in the whitelist"
    )));

    // Have a whitelisted address try to remove non-whitelisted address
    let info = get_message_info(&deps.api, USER2, &[]);

    let res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::RemoveFromWhitelist {
            address: user3_address.to_string(),
        },
    );
    assert!(res
        .unwrap_err()
        .to_string()
        .contains(&format!("address {user3_address} is not in the whitelist")));

    // Have a whitelisted address remove some whitelisted address
    let info = get_message_info(&deps.api, USER1, &[]);

    let res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::RemoveFromWhitelist {
            address: user1_address.to_string(),
        },
    );
    assert!(res.is_ok());

    // Have a whitelisted address try to remove the last whitelisted address
    let info = get_message_info(&deps.api, USER2, &[]);

    let res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::RemoveFromWhitelist {
            address: user2_address.to_string(),
        },
    );
    assert!(res
        .unwrap_err()
        .to_string()
        .contains("cannot remove last outstanding whitelisted address"));
}

#[test]
fn reporting_balance_queries_test() {
    let (mut deps, mut env) = (mock_dependencies(), mock_env());

    let vault_contract_addr = deps.api.addr_make(INFLOW);
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);
    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let user1_address = deps.api.addr_make(USER1);

    env.contract.address = vault_contract_addr.clone();

    let instantiate_msg = get_default_instantiate_msg(
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );

    let info = get_message_info(&deps.api, "creator", &[]);
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    let vault_shares_denom_str: String =
        format!("factory/{vault_contract_addr}/hydro_inflow_udatom");

    set_vault_shares_denom(&mut deps, vault_shares_denom_str.clone());

    // Setup initial mocks: total pool value and shares issued are 0
    let wasm_querier = MockWasmQuerier::new(HashMap::from_iter([
        setup_control_center_mock(
            control_center_contract_addr.clone(),
            DEFAULT_DEPOSIT_CAP,
            Uint128::zero(),
            Uint128::zero(),
        ),
        setup_token_info_provider_mock(
            token_info_provider_contract_addr,
            DEPOSIT_DENOM.to_string(),
            DATOM_DEFAULT_RATIO,
        ),
    ]));

    let querier_for_deps = wasm_querier.clone();
    deps.querier
        .update_wasm(move |q| querier_for_deps.handler(q));

    let user1_deposit1 = Uint128::new(500_000);
    let user1_deposit1_base_tokens = Uint128::new(600_000);
    let user1_expected_shares1 = Uint128::new(600_000);

    let total_pool_value = 600000;
    let total_shares_issued_before = 0;
    let total_shares_issued_after = user1_expected_shares1.u128();

    execute_deposit(
        &mut deps,
        &env,
        &wasm_querier,
        &vault_contract_addr,
        USER1,
        &vault_shares_denom_str,
        user1_deposit1,
        user1_expected_shares1,
        user1_expected_shares1,
        user1_deposit1,
        total_pool_value,
        total_shares_issued_before,
        total_shares_issued_after,
    );

    // Mock as if the total pool value increased to 601200 (in base tokens) and verify that everythin belongs to user1
    let deployed_amount = Uint128::new(1200);

    update_contract_mock(
        &mut deps,
        &wasm_querier,
        setup_default_control_center_mock(
            user1_deposit1_base_tokens + deployed_amount,
            user1_expected_shares1,
        ),
    );

    let query_msg = QueryMsg::PoolInfo {};
    let query_res = query(deps.as_ref(), env.clone(), query_msg);
    assert!(query_res.is_ok());

    let pool_info: PoolInfoResponse = from_json(query_res.unwrap()).unwrap();
    assert_eq!(pool_info.balance_base_tokens, user1_deposit1_base_tokens);

    let query_msg = QueryMsg::UserSharesEquivalentValue {
        address: user1_address.to_string(),
    };
    let query_res = query(deps.as_ref(), env.clone(), query_msg);
    assert!(query_res.is_ok());

    let eq_value: Uint128 = from_json(query_res.unwrap()).unwrap();
    assert_eq!(eq_value, Uint128::new(501_000));

    let query_msg = QueryMsg::SharesEquivalentValue {
        shares: user1_expected_shares1,
    };
    let query_res = query(deps.as_ref(), env, query_msg);
    assert!(query_res.is_ok());

    let eq_value: Uint128 = from_json(query_res.unwrap()).unwrap();
    assert_eq!(eq_value, Uint128::new(501_000));
}

#[test]
fn withdrawal_with_config_update_test() {
    let (mut deps, mut env) = (mock_dependencies(), mock_env());

    let vault_contract_addr = deps.api.addr_make(INFLOW);
    let control_center_contract_addr = deps.api.addr_make(CONTROL_CENTER);
    let token_info_provider_contract_addr = deps.api.addr_make(TOKEN_INFO_PROVIDER);
    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let user1_addr = deps.api.addr_make(USER1);

    env.contract.address = vault_contract_addr.clone();

    let instantiate_msg = get_default_instantiate_msg(
        DEPOSIT_DENOM,
        whitelist_addr.clone(),
        control_center_contract_addr.clone(),
        token_info_provider_contract_addr.clone(),
    );

    let info = get_message_info(&deps.api, "creator", &[]);

    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg.clone()).unwrap();

    let vault_shares_denom_str: String =
        format!("factory/{vault_contract_addr}/hydro_inflow_udatom");

    set_vault_shares_denom(&mut deps, vault_shares_denom_str.clone());

    // Setup initial mocks: total pool value and shares issued are 0
    let wasm_querier = MockWasmQuerier::new(HashMap::from_iter([
        setup_control_center_mock(
            control_center_contract_addr.clone(),
            DEFAULT_DEPOSIT_CAP,
            Uint128::zero(),
            Uint128::zero(),
        ),
        setup_token_info_provider_mock(
            token_info_provider_contract_addr,
            DEPOSIT_DENOM.to_string(),
            DATOM_DEFAULT_RATIO,
        ),
    ]));

    let querier_for_deps = wasm_querier.clone();
    deps.querier
        .update_wasm(move |q| querier_for_deps.handler(q));

    let mut total_pool_value = 12000;
    let total_shares_issued_before = 0;
    let mut total_shares_issued_after = 12000;

    // User1 deposits 10000 tokens
    let user1_deposit1 = Uint128::new(10000);
    let user1_deposit_shares1 = Uint128::new(12000);

    execute_deposit(
        &mut deps,
        &env,
        &wasm_querier,
        &vault_contract_addr,
        USER1,
        &vault_shares_denom_str,
        user1_deposit1,
        user1_deposit_shares1,
        user1_deposit_shares1,
        user1_deposit1,
        total_pool_value,
        total_shares_issued_before,
        total_shares_issued_after,
    );

    // Whitelisted address withdraws 10000 tokens for deployment
    execute_withdraw_for_deployment(
        &mut deps,
        &env,
        vault_contract_addr.as_ref(),
        WHITELIST_ADDR,
        user1_deposit1,
        Uint128::zero(),
    );

    // User1 requests withdrawal of 120 shares 10 times (enters the queue each time)
    let user1_withdrawal_shares1 = Uint128::new(120);
    let mut total_user_shares_after = user1_deposit_shares1;

    for _ in 0..10 {
        total_user_shares_after -= user1_withdrawal_shares1;

        total_pool_value -= user1_withdrawal_shares1.u128();
        total_shares_issued_after -= user1_withdrawal_shares1.u128();

        execute_withdraw(
            &mut deps,
            &env,
            &wasm_querier,
            vault_contract_addr.as_ref(),
            USER1,
            None,
            &vault_shares_denom_str,
            user1_withdrawal_shares1,
            false,
            Uint128::zero(),
            total_user_shares_after,
            Uint128::zero(),
            total_pool_value,
            total_shares_issued_after,
        );
    }

    // Verify withdrawal requests for both users and withdrawal queue info
    verify_user_withdrawal_requests(
        &deps,
        &user1_addr,
        vec![
            (user1_withdrawal_shares1, Uint128::new(100), false),
            (user1_withdrawal_shares1, Uint128::new(100), false),
            (user1_withdrawal_shares1, Uint128::new(100), false),
            (user1_withdrawal_shares1, Uint128::new(100), false),
            (user1_withdrawal_shares1, Uint128::new(100), false),
            (user1_withdrawal_shares1, Uint128::new(100), false),
            (user1_withdrawal_shares1, Uint128::new(100), false),
            (user1_withdrawal_shares1, Uint128::new(100), false),
            (user1_withdrawal_shares1, Uint128::new(100), false),
            (user1_withdrawal_shares1, Uint128::new(100), false),
        ],
    );

    verify_withdrawal_queue_info(
        &deps,
        user1_withdrawal_shares1 * Uint128::new(10),
        Uint128::new(1000),
        Uint128::new(1000),
    );

    let old_withdrawal_queue_info = WITHDRAWAL_QUEUE_INFO.load(&deps.storage).unwrap();

    // Try to create one more withdrawal request and verify the error returned
    let user_info = get_message_info(
        &deps.api,
        USER1,
        &[Coin {
            denom: vault_shares_denom_str.to_string(),
            amount: user1_withdrawal_shares1,
        }],
    );
    let withdraw_res = execute(
        deps.as_mut(),
        env.clone(),
        user_info.clone(),
        ExecuteMsg::Withdraw { on_behalf_of: None },
    )
    .unwrap_err();

    assert!(withdraw_res.to_string().contains(&format!(
        "user {user1_addr} has reached the maximum number of pending withdrawals: {}",
        instantiate_msg.max_withdrawals_per_user
    )));

    // Even though the withdrawal execute() failed, for some reason WITHDRAWAL_QUEUE_INFO changes
    // weren't reverted. Verify that this is the case, and revert to expected values, since the
    // next steps in test depend on this information being accurate.
    let info = WITHDRAWAL_QUEUE_INFO.load(&deps.storage).unwrap();
    assert_eq!(
        info.total_shares_burned,
        user1_withdrawal_shares1 * Uint128::new(11)
    );
    assert_eq!(
        info.total_withdrawal_amount,
        Uint128::new(100) * Uint128::new(11)
    );
    assert_eq!(
        info.non_funded_withdrawal_amount,
        Uint128::new(100) * Uint128::new(11)
    );
    WITHDRAWAL_QUEUE_INFO
        .save(&mut deps.storage, &old_withdrawal_queue_info)
        .unwrap();

    // Update config so that only 5 withdrawal requests per user are allowed
    let new_max_withdrawals_per_user = 5;
    let whitelisted_addr_info = get_message_info(&deps.api, WHITELIST_ADDR, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        whitelisted_addr_info.clone(),
        ExecuteMsg::UpdateConfig {
            config: UpdateConfigData {
                max_withdrawals_per_user: Some(new_max_withdrawals_per_user),
            },
        },
    )
    .unwrap();

    // User1 cancels their withdrawal requests 0, 1 and 2. This should be allowed since they created
    // 10 requests while it was allowed. After the cancelation they should have 7 pending requests.
    total_pool_value = 10800;
    total_shares_issued_after = 10800;

    execute_cancel_withdrawal(
        &mut deps,
        &env,
        &wasm_querier,
        USER1,
        vec![0, 1, 2],
        &vault_shares_denom_str,
        Some((
            user1_withdrawal_shares1 * Uint128::new(3),
            Uint128::new(9300),
        )),
        total_pool_value,
        total_shares_issued_after,
    );

    // Verify 7 remaining withdrawal requests for User1
    verify_user_withdrawal_requests(
        &deps,
        &user1_addr,
        vec![
            (user1_withdrawal_shares1, Uint128::new(100), false),
            (user1_withdrawal_shares1, Uint128::new(100), false),
            (user1_withdrawal_shares1, Uint128::new(100), false),
            (user1_withdrawal_shares1, Uint128::new(100), false),
            (user1_withdrawal_shares1, Uint128::new(100), false),
            (user1_withdrawal_shares1, Uint128::new(100), false),
            (user1_withdrawal_shares1, Uint128::new(100), false),
        ],
    );

    // User1 tries to create 8th withdrawal request which isn't allowed because the limit changed to 5.
    let withdraw_res = execute(
        deps.as_mut(),
        env.clone(),
        user_info.clone(),
        ExecuteMsg::Withdraw { on_behalf_of: None },
    )
    .unwrap_err();

    assert!(withdraw_res.to_string().contains(&format!(
        "user {user1_addr} has reached the maximum number of pending withdrawals: {new_max_withdrawals_per_user}",
    )));
}

#[allow(clippy::too_many_arguments)]
fn execute_deposit(
    deps: &mut OwnedDeps<MemoryStorage, MockApi, MockQuerier, NeutronQuery>,
    env: &Env,
    wasm_querier: &MockWasmQuerier,
    vault_contract_addr: &Addr,
    user_str: &str,
    vault_shares_denom_str: &String,
    deposit_amount: Uint128,
    expected_user_shares_minted: Uint128,
    mock_user_shares_total: Uint128,
    mock_inflow_deposit_tokens_total: Uint128,
    // when depositing, amount sent must be accounted for at the beginning of the execute call,
    // so there is no need to mock *before* and *after* values separately.
    mock_control_center_total_value: u128,
    mock_control_center_total_shares_before: u128,
    mock_control_center_total_shares_after: u128,
) {
    execute_deposit_with_recipient(
        deps,
        env,
        wasm_querier,
        vault_contract_addr,
        user_str,
        None,
        vault_shares_denom_str,
        deposit_amount,
        expected_user_shares_minted,
        mock_user_shares_total,
        mock_inflow_deposit_tokens_total,
        mock_control_center_total_value,
        mock_control_center_total_shares_before,
        mock_control_center_total_shares_after,
    );
}

#[allow(clippy::too_many_arguments)]
fn execute_deposit_with_recipient(
    deps: &mut OwnedDeps<MemoryStorage, MockApi, MockQuerier, NeutronQuery>,
    env: &Env,
    wasm_querier: &MockWasmQuerier,
    vault_contract_addr: &Addr,
    sender_str: &str,
    on_behalf_of: Option<&str>,
    vault_shares_denom_str: &String,
    deposit_amount: Uint128,
    expected_shares_minted: Uint128,
    mock_recipient_shares_total: Uint128,
    mock_inflow_deposit_tokens_total: Uint128,
    mock_control_center_total_value: u128,
    mock_control_center_total_shares_before: u128,
    mock_control_center_total_shares_after: u128,
) {
    // Mock that the vault contract has deposit tokens on its bank balance,
    // since in reallity this happens before execute() is called.
    mock_address_balance(
        deps,
        vault_contract_addr.as_ref(),
        DEPOSIT_DENOM,
        mock_inflow_deposit_tokens_total,
    );

    // Mock that the tokens sent by the user are accounted for in the Control Center query result
    update_contract_mock(
        deps,
        wasm_querier,
        setup_default_control_center_mock(
            Uint128::new(mock_control_center_total_value),
            Uint128::new(mock_control_center_total_shares_before),
        ),
    );

    let info = get_message_info(
        &deps.api,
        sender_str,
        &[Coin {
            denom: DEPOSIT_DENOM.to_string(),
            amount: deposit_amount,
        }],
    );

    let default_recipient = info.sender.to_string();
    let deposit_res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::Deposit {
            on_behalf_of: on_behalf_of.map(ToString::to_string),
        },
    )
    .unwrap();

    // Verify mint message for vault shares tokens
    let mint_msg = &deposit_res.messages[0].msg;
    match mint_msg {
        CosmosMsg::Custom(NeutronMsg::MintTokens {
            denom,
            amount,
            mint_to_address,
        }) => {
            assert_eq!(denom, vault_shares_denom_str);
            assert_eq!(amount, expected_shares_minted);
            assert_eq!(
                mint_to_address,
                &on_behalf_of
                    .map(ToString::to_string)
                    .unwrap_or(default_recipient.clone())
            );
        }
        _ => panic!("Expected MintTokens message"),
    }

    // Mock that the recipient received vault shares tokens on its bank balance
    mock_address_balance(
        deps,
        on_behalf_of.unwrap_or(&(default_recipient.clone())),
        vault_shares_denom_str,
        mock_recipient_shares_total,
    );

    // Mock that the Control Center query result reflects the new total value and shares after deposit
    update_contract_mock(
        deps,
        wasm_querier,
        setup_default_control_center_mock(
            Uint128::new(mock_control_center_total_value),
            Uint128::new(mock_control_center_total_shares_after),
        ),
    );
}

fn execute_withdraw_for_deployment(
    deps: &mut OwnedDeps<MemoryStorage, MockApi, MockQuerier, NeutronQuery>,
    env: &Env,
    vault_contract_addr: &str,
    whitelisted_user_str: &str,
    amount_to_withdraw: Uint128,
    mock_inflow_balance_after: Uint128,
) {
    let info = get_message_info(&deps.api, whitelisted_user_str, &[]);

    execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::WithdrawForDeployment {
            amount: amount_to_withdraw,
        },
    )
    .unwrap();

    mock_address_balance(
        deps,
        vault_contract_addr,
        DEPOSIT_DENOM,
        mock_inflow_balance_after,
    );
}

fn execute_return_from_deployment(
    deps: &mut OwnedDeps<MemoryStorage, MockApi, MockQuerier, NeutronQuery>,
    vault_contract_addr: &str,
    amount_to_return: Uint128,
) {
    let current_contract_balance: Uint128 = from_json::<BalanceResponse>(
        deps.querier
            .bank
            .query(&BankQuery::Balance {
                address: vault_contract_addr.to_owned(),
                denom: DEPOSIT_DENOM.to_owned(),
            })
            .unwrap()
            .unwrap(),
    )
    .unwrap()
    .amount
    .amount;

    mock_address_balance(
        deps,
        vault_contract_addr,
        DEPOSIT_DENOM,
        current_contract_balance + amount_to_return,
    );
}

#[allow(clippy::too_many_arguments)]
fn execute_withdraw(
    deps: &mut OwnedDeps<MemoryStorage, MockApi, MockQuerier, NeutronQuery>,
    env: &Env,
    wasm_querier: &MockWasmQuerier,
    vault_contract_addr: &str,
    user_str: &str,
    on_behalf_of: Option<&str>,
    vault_shares_denom_str: &String,
    withdraw_shares_amount: Uint128,
    should_receive_deposit_tokens: bool,
    expected_tokens_received: Uint128,
    total_user_shares_after: Uint128,
    contract_deposit_tokens_after: Uint128,
    mock_control_center_total_value_after: u128,
    mock_control_center_total_shares_after: u128,
) {
    let info = get_message_info(
        &deps.api,
        user_str,
        &[Coin {
            denom: vault_shares_denom_str.to_string(),
            amount: withdraw_shares_amount,
        }],
    );

    let user_address = info.sender.clone();
    let withdraw_res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::Withdraw {
            on_behalf_of: on_behalf_of.map(ToString::to_string),
        },
    )
    .unwrap();

    let withdrawer = on_behalf_of
        .map(|recipient| deps.api.addr_validate(recipient).unwrap())
        .unwrap_or_else(|| info.sender.clone());

    if should_receive_deposit_tokens {
        // Verify bank send message to receive deposit tokens
        let bank_send_msg = &withdraw_res.messages[0].msg;
        match bank_send_msg {
            CosmosMsg::Bank(BankMsg::Send { to_address, amount }) => {
                assert_eq!(to_address, withdrawer.as_ref());
                assert_eq!(amount[0].denom, DEPOSIT_DENOM);
                assert_eq!(amount[0].amount, expected_tokens_received);
            }
            _ => panic!("Expected BankSend message"),
        }

        // Verify burn message for vault shares tokens
        let burn_msg = &withdraw_res.messages[1].msg;
        match burn_msg {
            CosmosMsg::Custom(NeutronMsg::BurnTokens {
                denom,
                amount,
                burn_from_address: _,
            }) => {
                assert_eq!(denom, vault_shares_denom_str);
                assert_eq!(amount, withdraw_shares_amount);
            }
            _ => panic!("Expected MintTokens message"),
        }

        // Update vault contract deposit tokens balance
        mock_address_balance(
            deps,
            vault_contract_addr,
            DEPOSIT_DENOM,
            contract_deposit_tokens_after,
        );
    }

    // Update user's vault shares tokens balance
    mock_address_balance(
        deps,
        user_address.as_ref(),
        vault_shares_denom_str,
        total_user_shares_after,
    );

    // Mock that the Control Center query result reflects the new total value and shares after withdrawal
    update_contract_mock(
        deps,
        wasm_querier,
        setup_default_control_center_mock(
            Uint128::new(mock_control_center_total_value_after),
            Uint128::new(mock_control_center_total_shares_after),
        ),
    );
}

type CancelWithdrawalExpectedResult = (
    Uint128, // expected_vault_shares_received
    Uint128, // mock_user_shares_after
);

#[allow(clippy::too_many_arguments)]
fn execute_cancel_withdrawal(
    deps: &mut OwnedDeps<MemoryStorage, MockApi, MockQuerier, NeutronQuery>,
    env: &Env,
    wasm_querier: &MockWasmQuerier,
    user_str: &str,
    withdrawal_ids: Vec<u64>,
    vault_shares_denom_str: &String,
    expected_result: Option<CancelWithdrawalExpectedResult>,
    mock_control_center_total_value_after: u128,
    mock_control_center_total_shares_after: u128,
) {
    let info = get_message_info(&deps.api, user_str, &[]);

    let user_address = info.sender.clone();
    let cancel_withdrawal_res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::CancelWithdrawal { withdrawal_ids },
    )
    .unwrap();

    if let Some(expected_result) = expected_result {
        // Verify mint message for vault shares tokens
        let mint_msg = &cancel_withdrawal_res.messages[0].msg;
        match mint_msg {
            CosmosMsg::Custom(NeutronMsg::MintTokens {
                denom,
                amount,
                mint_to_address,
            }) => {
                assert_eq!(denom, vault_shares_denom_str);
                assert_eq!(amount, expected_result.0);
                assert_eq!(mint_to_address, user_address.as_ref());
            }
            _ => panic!("Expected MintTokens message"),
        }

        // Update user's vault shares tokens balance
        mock_address_balance(
            deps,
            user_address.as_ref(),
            vault_shares_denom_str,
            expected_result.1,
        );

        // Mock that the Control Center query result reflects the new total value and shares after withdrawal cancelation
        update_contract_mock(
            deps,
            wasm_querier,
            setup_default_control_center_mock(
                Uint128::new(mock_control_center_total_value_after),
                Uint128::new(mock_control_center_total_shares_after),
            ),
        );
    }
}

type ClaimUnbondedWithdrawalExpectedResult = (
    Addr,    // withdrawer address
    Uint128, // expected_tokens_received
);

#[allow(clippy::too_many_arguments)]
fn execute_claim_unbonded_withdrawals(
    deps: &mut OwnedDeps<MemoryStorage, MockApi, MockQuerier, NeutronQuery>,
    env: &Env,
    vault_contract_addr: &str,
    sender_str: &str,
    withdrawal_ids: Vec<u64>,
    expected_error: Option<&str>,
    expected_results: Vec<ClaimUnbondedWithdrawalExpectedResult>,
    mock_inflow_contract_total_tokens: Uint128,
) {
    let info = get_message_info(&deps.api, sender_str, &[]);
    let claim_unbodned_withdrawals_res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::ClaimUnbondedWithdrawals { withdrawal_ids },
    );

    if let Some(err) = expected_error {
        assert!(claim_unbodned_withdrawals_res
            .unwrap_err()
            .to_string()
            .contains(err));
        return;
    }

    let claim_unbodned_withdrawals_res = claim_unbodned_withdrawals_res.unwrap();

    assert_eq!(
        claim_unbodned_withdrawals_res.messages.len(),
        expected_results.len()
    );

    let expected_results: HashMap<String, Uint128> = expected_results
        .into_iter()
        .map(|expected_res| (expected_res.0.to_string(), expected_res.1))
        .collect();

    for i in 0..claim_unbodned_withdrawals_res.messages.len() {
        let bank_msg = claim_unbodned_withdrawals_res.messages[i].msg.clone();
        match bank_msg {
            CosmosMsg::Bank(BankMsg::Send { to_address, amount }) => {
                // Successful get verifies that the recipient is expected
                let expected_amount = expected_results.get(&to_address).unwrap();
                assert_eq!(amount[0].amount, expected_amount);
                assert_eq!(amount[0].denom, DEPOSIT_DENOM);
            }
            _ => panic!("Expected BankMsg::Send"),
        }

        mock_address_balance(
            deps,
            vault_contract_addr,
            DEPOSIT_DENOM,
            mock_inflow_contract_total_tokens,
        );
    }
}

fn execute_fulfill_pending_withdrawals(
    deps: &mut OwnedDeps<MemoryStorage, MockApi, MockQuerier, NeutronQuery>,
    env: &Env,
    user_str: &str,
) {
    let info = get_message_info(&deps.api, user_str, &[]);

    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::FulfillPendingWithdrawals { limit: 100 },
    )
    .unwrap();
}

fn verify_withdrawal_queue_info(
    deps: &OwnedDeps<MemoryStorage, MockApi, MockQuerier, NeutronQuery>,
    expected_total_shares_burned: Uint128,
    expected_total_withdrawal_amount: Uint128,
    expected_non_funded_withdrawal_amount: Uint128,
) {
    let queue_info = query_withdrawal_queue_info(&deps.as_ref()).unwrap();

    assert_eq!(
        queue_info.info.total_shares_burned,
        expected_total_shares_burned
    );
    assert_eq!(
        queue_info.info.total_withdrawal_amount,
        expected_total_withdrawal_amount
    );
    assert_eq!(
        queue_info.info.non_funded_withdrawal_amount,
        expected_non_funded_withdrawal_amount
    );
}

type ExpectedWithdrawalRequest = (
    Uint128, // shares_burned
    Uint128, // amount_to_receive
    bool,    // is_funded
);

fn verify_user_withdrawal_requests(
    deps: &OwnedDeps<MemoryStorage, MockApi, MockQuerier, NeutronQuery>,
    user_address: &Addr,
    expected_withdrawal_requests: Vec<ExpectedWithdrawalRequest>,
) {
    let user_withdrawals =
        query_user_withdrawal_requests(&deps.as_ref(), user_address.to_string(), 0, 100).unwrap();

    assert_eq!(
        user_withdrawals.withdrawals.len(),
        expected_withdrawal_requests.len()
    );

    for (i, expected) in expected_withdrawal_requests.iter().enumerate() {
        assert_eq!(user_withdrawals.withdrawals[i].shares_burned, expected.0);
        assert_eq!(
            user_withdrawals.withdrawals[i].amount_to_receive,
            expected.1
        );
        assert_eq!(user_withdrawals.withdrawals[i].is_funded, expected.2);
    }
}

type ExpectedPayoutHistory = (
    Addr,                    // recipient
    Vec<(Uint128, Uint128)>, // (shares_burned, amount_received)
);

fn verify_users_payouts_history(
    deps: &OwnedDeps<MemoryStorage, MockApi, MockQuerier, NeutronQuery>,
    expected_payouts_history: Vec<ExpectedPayoutHistory>,
) {
    for expected_user_payouts in expected_payouts_history {
        let user_payouts = query_user_payouts_history(
            &deps.as_ref(),
            expected_user_payouts.0.to_string(),
            0,
            100,
            Order::Ascending,
        )
        .unwrap()
        .payouts;

        assert_eq!(user_payouts.len(), expected_user_payouts.1.len());

        for (i, expected_payout) in expected_user_payouts.1.iter().enumerate() {
            assert_eq!(user_payouts[i].vault_shares_burned, expected_payout.0);
            assert_eq!(user_payouts[i].amount_received, expected_payout.1);
        }
    }
}
