use std::marker::PhantomData;

use cosmwasm_std::{
    coin, from_json,
    testing::{mock_env, MockApi, MockQuerier, MockStorage},
    Coin, MessageInfo, OwnedDeps, Uint128,
};
use neutron_sdk::bindings::query::NeutronQuery;

use crate::{
    contract::{execute, instantiate, query},
    msg::{DenomMetadata, ExecuteMsg, InstantiateMsg},
    query::QueryMsg,
    state::VAULT_SHARES_DENOM,
};

const DEPOSIT_DENOM: &str = "ibc/C4CFF46FD6DE35CA4CF4CE031E643C8FDC9BA4B99AE598E9B0ED98FE3A2319F9";
const WHITELIST_ADDR: &str = "whitelist1";
const INFLOW: &str = "inflow";
const USER1: &str = "user1";

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
fn submit_deployed_amount_test() {
    let (mut deps, env) = (mock_dependencies(), mock_env());

    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let user1_address = deps.api.addr_make(USER1);

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

    let exec_msg = ExecuteMsg::SubmitDeployedAmount {
        amount: Uint128::from(100u128),
    };
    let info_user_1 = get_message_info(&deps.api, user1_address.as_ref(), &[]);

    //try submitting deployed amount with a user that is not whitelisted
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info_user_1.clone(),
        exec_msg.clone(),
    );
    assert!(res.is_err());
    assert!(res.err().unwrap().to_string().contains("Unauthorized"),);

    let query_msg = QueryMsg::DeployedAmount {};
    //deployed amount should be zero before any submission
    let query_res = query(deps.as_ref(), env.clone(), query_msg.clone());
    assert!(query_res.is_ok());
    let value: Uint128 = from_json(query_res.unwrap()).unwrap();
    assert_eq!(value, Uint128::zero());

    let info_whitelisted = get_message_info(&deps.api, WHITELIST_ADDR, &[]);

    let res = execute(
        deps.as_mut(),
        env.clone(),
        info_whitelisted.clone(),
        exec_msg,
    );
    assert!(res.is_ok());

    // deployed amount should be updated after whitelisted user submits it
    let query_res = query(deps.as_ref(), env.clone(), query_msg);
    assert!(query_res.is_ok());
    let value: Uint128 = from_json(query_res.unwrap()).unwrap();
    assert_eq!(value, Uint128::from(100u128));
}
#[test]
fn queries_test() {
    let (mut deps, mut env) = (mock_dependencies(), mock_env());

    let inflow_contract_addr = deps.api.addr_make(INFLOW);
    let whitelist_addr = deps.api.addr_make(WHITELIST_ADDR);
    let user1_address = deps.api.addr_make(USER1);

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

    let deposit_amount = Uint128::from(500_000u128);

    let info_user_funds = get_message_info(
        &deps.api,
        user1_address.as_str(),
        &[coin(deposit_amount.into(), DEPOSIT_DENOM)], // 0.5
    );

    let exec_msg = ExecuteMsg::Deposit {};

    mock_address_balance(
        &mut deps,
        inflow_contract_addr.as_ref(),
        DEPOSIT_DENOM,
        deposit_amount,
    );

    let res = execute(
        deps.as_mut(),
        env.clone(),
        info_user_funds.clone(),
        exec_msg,
    );
    assert!(res.is_ok());

    let query_msg = QueryMsg::TotalPoolValue {};
    let query_res = query(deps.as_ref(), env.clone(), query_msg);
    assert!(query_res.is_ok());

    let total: Uint128 = from_json(query_res.unwrap()).unwrap();
    assert_eq!(total, deposit_amount);

    mock_address_balance(
        &mut deps,
        user1_address.as_ref(),
        &vault_shares_denom_str,
        deposit_amount,
    );

    let query_msg = QueryMsg::UserSharesEquivalentValue {
        address: user1_address.to_string(),
    };
    let query_res = query(deps.as_ref(), env.clone(), query_msg);
    assert!(query_res.is_ok());

    let eq_value: Uint128 = from_json(query_res.unwrap()).unwrap();
    assert_eq!(eq_value, deposit_amount);

    let query_msg = QueryMsg::SharesEquivalentValue {
        shares: deposit_amount,
    };
    let query_res = query(deps.as_ref(), env, query_msg);
    assert!(query_res.is_ok());

    let eq_value: Uint128 = from_json(query_res.unwrap()).unwrap();
    assert_eq!(eq_value, deposit_amount);
}
