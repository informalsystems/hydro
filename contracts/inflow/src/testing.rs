use std::marker::PhantomData;

use crate::{
    contract::{execute, instantiate},
    msg::{DenomMetadata, ExecuteMsg, InstantiateMsg},
    state::{DEPLOYED_AMOUNT, VAULT_SHARES_DENOM},
};
use cosmwasm_std::{
    testing::{mock_env, MockApi, MockQuerier, MockStorage},
    Addr, BankMsg, Coin, CosmosMsg, Env, MemoryStorage, MessageInfo, OwnedDeps, Uint128,
};
use neutron_sdk::bindings::{msg::NeutronMsg, query::NeutronQuery};

const DEPOSIT_DENOM: &str = "ibc/C4CFF46FD6DE35CA4CF4CE031E643C8FDC9BA4B99AE598E9B0ED98FE3A2319F9";
const WHITELIST_ADDR: &str = "whitelist1";
const INFLOW: &str = "inflow";
const USER1: &str = "user1";
const USER2: &str = "user2";
const USER3: &str = "user3";

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

// Helper to set up the querier to return a specific balance for the given address
fn mock_address_balance(
    deps: &mut OwnedDeps<MockStorage, MockApi, MockQuerier, NeutronQuery>,
    address: &str,
    denom: &str,
    amount: Uint128,
) {
    deps.querier.bank.update_balance(
        address,
        vec![Coin {
            denom: denom.to_string(),
            amount,
        }],
    );
}

#[test]
fn deposit_withdrawal_test() {
    let (mut deps, mut env) = (mock_dependencies(), mock_env());

    let inflow_contract_addr = deps.api.addr_make(INFLOW);
    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let user1_address = deps.api.addr_make(USER1);
    let user2_address = deps.api.addr_make(USER2);
    let user3_address = deps.api.addr_make(USER3);

    env.contract.address = inflow_contract_addr.clone();

    let instantiate_msg = InstantiateMsg {
        deposit_denom: DEPOSIT_DENOM.to_string(),
        whitelist: vec![whitelist_addr.to_string()],
        subdenom: "hydro_inflow_uatom".to_string(),
        token_metadata: DenomMetadata {
            display: "hydro_inflow_atom".to_string(),
            exponent: 6,
            name: "Hydro Inflow ATOM".to_string(),
            description: "Hydro Inflow ATOM".to_string(),
            symbol: "hydro_inflow_atom".to_string(),
            uri: None,
            uri_hash: None,
        },
    };

    let info = get_message_info(&deps.api, "creator", &[]);
    instantiate(deps.as_mut(), env.clone(), info, instantiate_msg).unwrap();

    let vault_shares_denom_str: String =
        format!("factory/{inflow_contract_addr}/hydro_inflow_uatom");

    VAULT_SHARES_DENOM
        .save(deps.as_mut().storage, &vault_shares_denom_str.clone())
        .unwrap();

    let user1_deposit1 = Uint128::new(1000);
    let user1_expected_shares1 = Uint128::new(1000);

    // User1 deposits 1000 tokens -> mock this increase in the contract bank balance
    mock_address_balance(
        &mut deps,
        inflow_contract_addr.as_ref(),
        DEPOSIT_DENOM,
        user1_deposit1,
    );

    execute_deposit(
        &mut deps,
        &env,
        USER1,
        &user1_address,
        &vault_shares_denom_str,
        user1_deposit1,
        user1_expected_shares1,
        user1_expected_shares1,
    );

    let user2_deposit1 = Uint128::new(3000);
    let user2_expected_shares1 = Uint128::new(3000);

    // User2 deposits 3000 tokens -> mock this increase in the contract bank balance
    mock_address_balance(
        &mut deps,
        inflow_contract_addr.as_ref(),
        DEPOSIT_DENOM,
        user1_deposit1 + user2_deposit1,
    );

    execute_deposit(
        &mut deps,
        &env,
        USER2,
        &user2_address,
        &vault_shares_denom_str,
        user2_deposit1,
        user2_expected_shares1,
        user2_expected_shares1,
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

    // Mock that the Inflow contract doesn't have any tokens left on its bank balance
    mock_address_balance(
        &mut deps,
        inflow_contract_addr.as_ref(),
        DEPOSIT_DENOM,
        Uint128::zero(),
    );

    // Verify DEPLOYED_AMOUNT is equal to 4000 tokens
    let deployed = DEPLOYED_AMOUNT.load(deps.as_ref().storage).unwrap();
    assert_eq!(deployed, withdrawal_amount);

    let user1_deposit2 = Uint128::new(2000);
    let user1_expected_shares2 = Uint128::new(2000);

    // User1 deposits 2000 tokens -> mock this increase in the contract bank balance
    mock_address_balance(
        &mut deps,
        inflow_contract_addr.as_ref(),
        DEPOSIT_DENOM,
        user1_deposit2,
    );

    execute_deposit(
        &mut deps,
        &env,
        USER1,
        &user1_address,
        &vault_shares_denom_str,
        user1_deposit2,
        user1_expected_shares2,
        user1_expected_shares1 + user1_expected_shares2,
    );

    // Set DEPLOYED_AMOUNT to 4100
    DEPLOYED_AMOUNT
        .save(deps.as_mut().storage, &Uint128::new(4100), env.block.height)
        .unwrap();

    let user3_deposit1 = Uint128::new(1000);
    let user3_expected_shares1 = Uint128::new(983);

    // User3 deposits 1000 tokens -> mock this increase in the contract bank balance
    mock_address_balance(
        &mut deps,
        inflow_contract_addr.as_ref(),
        DEPOSIT_DENOM,
        user1_deposit2 + user3_deposit1,
    );

    execute_deposit(
        &mut deps,
        &env,
        USER3,
        &user3_address,
        &vault_shares_denom_str,
        user3_deposit1,
        user3_expected_shares1,
        user3_expected_shares1,
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

    // Mock that the Inflow contract doesn't have any tokens left on its bank balance
    mock_address_balance(
        &mut deps,
        inflow_contract_addr.as_ref(),
        DEPOSIT_DENOM,
        Uint128::zero(),
    );

    // Verify DEPLOYED_AMOUNT is now equal to 7100 tokens
    let deployed_amount = DEPLOYED_AMOUNT.load(deps.as_ref().storage).unwrap();
    assert_eq!(deployed_amount, Uint128::new(7100));

    // Set DEPLOYED_AMOUNT to 7300 to verify that on next deposit even less shares are issued
    DEPLOYED_AMOUNT
        .save(deps.as_mut().storage, &Uint128::new(7300), env.block.height)
        .unwrap();

    let user3_deposit2 = Uint128::new(1000);
    let user3_expected_shares2 = Uint128::new(956);

    // User3 deposits 1000 tokens -> mock this increase in the contract bank balance
    mock_address_balance(
        &mut deps,
        inflow_contract_addr.as_ref(),
        DEPOSIT_DENOM,
        user3_deposit2,
    );

    execute_deposit(
        &mut deps,
        &env,
        USER3,
        &user3_address,
        &vault_shares_denom_str,
        user3_deposit2,
        user3_expected_shares2,
        user3_expected_shares1 + user3_expected_shares2,
    );
}

#[test]
fn whitelist_add_remove_test() {
    let (mut deps, mut env) = (mock_dependencies(), mock_env());

    let inflow_contract_addr = deps.api.addr_make(INFLOW);
    let user1_address = deps.api.addr_make(USER1);
    let user2_address = deps.api.addr_make(USER2);
    let user3_address = deps.api.addr_make(USER3);

    env.contract.address = inflow_contract_addr.clone();

    let instantiate_msg = InstantiateMsg {
        deposit_denom: DEPOSIT_DENOM.to_string(),
        whitelist: vec![user1_address.to_string()],
        subdenom: "hydro_inflow_uatom".to_string(),
        token_metadata: DenomMetadata {
            display: "hydro_inflow_atom".to_string(),
            exponent: 6,
            name: "Hydro Inflow ATOM".to_string(),
            description: "Hydro Inflow ATOM".to_string(),
            symbol: "hydro_inflow_atom".to_string(),
            uri: None,
            uri_hash: None,
        },
    };

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

#[allow(clippy::too_many_arguments)]
fn execute_deposit(
    deps: &mut OwnedDeps<MemoryStorage, MockApi, MockQuerier, NeutronQuery>,
    env: &Env,
    user_str: &str,
    user_address: &Addr,
    vault_shares_denom_str: &String,
    deposit_amount: Uint128,
    expected_shares_minted: Uint128,
    expected_shares_total: Uint128,
) {
    let info = get_message_info(
        &deps.api,
        user_str,
        &[Coin {
            denom: DEPOSIT_DENOM.to_string(),
            amount: deposit_amount,
        }],
    );

    let deposit_res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::Deposit {},
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
            assert_eq!(mint_to_address, user_address.as_ref());
        }
        _ => panic!("Expected MintTokens message"),
    }

    // Mock that the user received vault shares tokens on its bank balance
    mock_address_balance(
        deps,
        user_address.as_ref(),
        vault_shares_denom_str,
        expected_shares_total,
    );
}
