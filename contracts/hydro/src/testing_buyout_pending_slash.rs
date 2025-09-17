use std::{collections::HashMap, str::FromStr};

use cosmwasm_std::{testing::mock_env, Coin, Decimal, Uint128};

use crate::{
    contract::{execute, instantiate},
    msg::{ExecuteMsg, TokenInfoProviderInstantiateMsg},
    state::LOCKS_PENDING_SLASHES,
    testing::{
        get_default_instantiate_msg, get_default_lsm_token_info_provider_init_msg,
        get_message_info, get_st_atom_denom_info_mock_data, get_validator_info_mock_data,
        setup_lsm_token_info_provider_mock, setup_multiple_token_info_provider_mocks,
        DERIVATIVE_TOKEN_PROVIDER_ADDR, IBC_DENOM_1, IBC_DENOM_2, LSM_TOKEN_PROVIDER_ADDR,
        ONE_MONTH_IN_NANO_SECONDS, ST_ATOM_ON_NEUTRON, VALIDATOR_1, VALIDATOR_1_LST_DENOM_1,
        VALIDATOR_2, VALIDATOR_2_LST_DENOM_1,
    },
    testing_mocks::{denom_trace_grpc_query_mock, grpc_query_diff_paths_mock, mock_dependencies},
};

pub const ATOM_ON_NEUTRON: &str =
    "ibc/C4CFF46FD6DE35CA4CF4CE031E643C8FDC9BA4B99AE598E9B0ED98FE3A2319F9";

// slash denom: lsm, buyout_denom: lsm , exact amount
#[test]
fn buyout_pending_slash_same_denom_exact_amount_test() {
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
    );
    let (mut deps, env) = (mock_dependencies(grpc_query), mock_env());
    let info = get_message_info(&deps.api, "addr0000", &[]);
    let msg = get_default_instantiate_msg(&deps.api);

    let res = instantiate(deps.as_mut(), env.clone(), info, msg.clone());
    assert!(res.is_ok());

    let lsm_token_info_provider_addr = deps.api.addr_make(LSM_TOKEN_PROVIDER_ADDR);
    setup_lsm_token_info_provider_mock(
        &mut deps,
        lsm_token_info_provider_addr.clone(),
        vec![(0, vec![(VALIDATOR_1.to_string(), Decimal::one())])],
        true,
    );

    let lockup_amount: u128 = 1000;
    let info = get_message_info(
        &deps.api,
        "addr0000",
        &[Coin::new(lockup_amount, IBC_DENOM_1.to_string())],
    );
    let msg = ExecuteMsg::LockTokens {
        lock_duration: 3 * ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok());

    let _ = LOCKS_PENDING_SLASHES.save(&mut deps.storage, 0, &Uint128::new(500));

    let buyout_amount: u128 = 500;
    let buyout_info = get_message_info(
        &deps.api,
        "addr0000",
        &[Coin::new(buyout_amount, IBC_DENOM_1.to_string())],
    );

    let msg = ExecuteMsg::BuyoutPendingSlash { lock_id: 0 };
    let res = execute(deps.as_mut(), env.clone(), buyout_info.clone(), msg).unwrap();
    // only forward the used amount to the slash tokens receiver
    assert_eq!(res.messages.len(), 1);
    let result = LOCKS_PENDING_SLASHES.may_load(&deps.storage, 1);
    assert!(result.unwrap().is_none());
}

// slash denom: lsm, buyout_denom: lsm , partial buyout
#[test]
fn buyout_pending_slash_same_denom_partial_amount_test() {
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
    );
    let (mut deps, env) = (mock_dependencies(grpc_query), mock_env());
    let info = get_message_info(&deps.api, "addr0000", &[]);
    let msg = get_default_instantiate_msg(&deps.api);

    let res = instantiate(deps.as_mut(), env.clone(), info, msg.clone());
    assert!(res.is_ok());

    let lsm_token_info_provider_addr = deps.api.addr_make(LSM_TOKEN_PROVIDER_ADDR);
    setup_lsm_token_info_provider_mock(
        &mut deps,
        lsm_token_info_provider_addr.clone(),
        vec![(0, vec![(VALIDATOR_1.to_string(), Decimal::one())])],
        true,
    );

    let lockup_amount: u128 = 1000;
    let info = get_message_info(
        &deps.api,
        "addr0000",
        &[Coin::new(lockup_amount, IBC_DENOM_1.to_string())],
    );
    let msg = ExecuteMsg::LockTokens {
        lock_duration: 3 * ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok());

    let _ = LOCKS_PENDING_SLASHES.save(&mut deps.storage, 0, &Uint128::new(500));

    let buyout_amount: u128 = 400;
    let buyout_info = get_message_info(
        &deps.api,
        "addr0000",
        &[Coin::new(buyout_amount, IBC_DENOM_1.to_string())],
    );

    let msg = ExecuteMsg::BuyoutPendingSlash { lock_id: 0 };
    let res = execute(deps.as_mut(), env.clone(), buyout_info.clone(), msg).unwrap();
    // only forward the used amount to the slash tokens receiver
    assert_eq!(res.messages.len(), 1);
    let result = LOCKS_PENDING_SLASHES.may_load(&deps.storage, 0);
    assert_eq!(result.unwrap(), Some(Uint128::new(100))); // 500 - 400 = 100 remaining slash amount
}

// slash denom: lsm, buyout_denom: lsm , overpay - needs return
#[test]
fn buyout_pending_slash_same_denom_overpay_amount_test() {
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
    );
    let (mut deps, env) = (mock_dependencies(grpc_query), mock_env());
    let info = get_message_info(&deps.api, "addr0000", &[]);
    let msg = get_default_instantiate_msg(&deps.api);

    let res = instantiate(deps.as_mut(), env.clone(), info, msg.clone());
    assert!(res.is_ok());

    let lsm_token_info_provider_addr = deps.api.addr_make(LSM_TOKEN_PROVIDER_ADDR);
    setup_lsm_token_info_provider_mock(
        &mut deps,
        lsm_token_info_provider_addr.clone(),
        vec![(0, vec![(VALIDATOR_1.to_string(), Decimal::one())])],
        true,
    );

    let lockup_amount: u128 = 1000;
    let info = get_message_info(
        &deps.api,
        "addr0000",
        &[Coin::new(lockup_amount, IBC_DENOM_1.to_string())],
    );
    let msg = ExecuteMsg::LockTokens {
        lock_duration: 3 * ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok());

    let _ = LOCKS_PENDING_SLASHES.save(&mut deps.storage, 0, &Uint128::new(500));

    let buyout_amount: u128 = 600;
    let buyout_info = get_message_info(
        &deps.api,
        "addr0000",
        &[Coin::new(buyout_amount, IBC_DENOM_1.to_string())],
    );

    let msg = ExecuteMsg::BuyoutPendingSlash { lock_id: 0 };
    let res = execute(deps.as_mut(), env.clone(), buyout_info.clone(), msg).unwrap();
    // forward the used amount to the slash tokens receiver and return the excess
    assert_eq!(res.messages.len(), 2);
    let result = LOCKS_PENDING_SLASHES.may_load(&deps.storage, 0);
    assert!(result.unwrap().is_none());
}

// slash denom: lsm, buyout_denom: lsm , exact amount, validator slashed
#[test]
fn buyout_pending_slash_same_denom_validator_slashed_test() {
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([
            (IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string()),
            (IBC_DENOM_2.to_string(), VALIDATOR_2_LST_DENOM_1.to_string()),
        ]),
    );
    let (mut deps, env) = (mock_dependencies(grpc_query), mock_env());
    let info = get_message_info(&deps.api, "addr0000", &[]);
    let msg = get_default_instantiate_msg(&deps.api);

    let res = instantiate(deps.as_mut(), env.clone(), info, msg.clone());
    assert!(res.is_ok());

    let lsm_token_info_provider_addr = deps.api.addr_make(LSM_TOKEN_PROVIDER_ADDR);
    setup_lsm_token_info_provider_mock(
        &mut deps,
        lsm_token_info_provider_addr.clone(),
        vec![(
            0,
            vec![
                (VALIDATOR_1.to_string(), Decimal::percent(95)),
                (VALIDATOR_2.to_string(), Decimal::one()),
            ],
        )],
        true,
    );

    let lockup_amount: u128 = 1000;
    let info = get_message_info(
        &deps.api,
        "addr0000",
        &[Coin::new(lockup_amount, IBC_DENOM_2.to_string())],
    );
    let msg = ExecuteMsg::LockTokens {
        lock_duration: 3 * ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok());

    let _ = LOCKS_PENDING_SLASHES.save(&mut deps.storage, 0, &Uint128::new(500));

    let buyout_amount: u128 = 500;
    let buyout_info = get_message_info(
        &deps.api,
        "addr0000",
        &[Coin::new(buyout_amount, IBC_DENOM_1.to_string())],
    );

    let msg = ExecuteMsg::BuyoutPendingSlash { lock_id: 0 };
    let res = execute(deps.as_mut(), env.clone(), buyout_info.clone(), msg).unwrap();
    // only forward the used amount to the slash tokens receiver
    assert_eq!(res.messages.len(), 1);
    let result = LOCKS_PENDING_SLASHES.may_load(&deps.storage, 0);
    assert_eq!(result.unwrap(), Some(Uint128::new(25))); // 500 - 475 = 25 remaining slash amount
}
// slash denom: lsm, buyout_denom: lsm and statom
#[test]
fn buyout_pending_slash_lsm_statom_exact_amount_test() {
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
    );
    let (mut deps, env) = (mock_dependencies(grpc_query), mock_env());
    let info = get_message_info(&deps.api, "addr0000", &[]);
    let msg = get_default_instantiate_msg(&deps.api);

    let res = instantiate(deps.as_mut(), env.clone(), info, msg.clone());
    assert!(res.is_ok());

    let st_token_info_provider_addr = deps.api.addr_make(DERIVATIVE_TOKEN_PROVIDER_ADDR);
    let derivative_providers = HashMap::from([get_st_atom_denom_info_mock_data(
        st_token_info_provider_addr.to_string(),
        vec![(0, Decimal::from_str("1.30").unwrap())],
    )]);

    let lsm_token_info_provider_addr = deps.api.addr_make(LSM_TOKEN_PROVIDER_ADDR);
    let lsm_provider = Some((
        lsm_token_info_provider_addr.to_string(),
        HashMap::from_iter((0..=2).map(|round_id: u64| {
            (
                round_id,
                HashMap::from([get_validator_info_mock_data(
                    VALIDATOR_1.to_string(),
                    Decimal::one(),
                )]),
            )
        })),
    ));

    setup_multiple_token_info_provider_mocks(
        &mut deps,
        derivative_providers.clone(),
        lsm_provider.clone(),
        true,
    );

    let lockup_amount: u128 = 1000;
    let info = get_message_info(
        &deps.api,
        "addr0000",
        &[Coin::new(lockup_amount, IBC_DENOM_1.to_string())],
    );
    let msg = ExecuteMsg::LockTokens {
        lock_duration: 3 * ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok());

    let _ = LOCKS_PENDING_SLASHES.save(&mut deps.storage, 0, &Uint128::new(500));

    let buyout_amount_lsm: u128 = 500;
    let buyout_amount_st: u128 = 500;
    let buyout_info = get_message_info(
        &deps.api,
        "addr0000",
        &[
            Coin::new(buyout_amount_lsm, IBC_DENOM_1.to_string()),
            Coin::new(buyout_amount_st, ST_ATOM_ON_NEUTRON.to_string()),
        ],
    );

    let msg = ExecuteMsg::BuyoutPendingSlash { lock_id: 0 };
    let res = execute(deps.as_mut(), env.clone(), buyout_info.clone(), msg).unwrap();
    // forward the used amount to the slash tokens receiver and return the excess of other denom
    assert_eq!(res.messages.len(), 2);
    let result = LOCKS_PENDING_SLASHES.may_load(&deps.storage, 0);
    assert!(result.unwrap().is_none());
}

// slash denom: statom, buyout_denom: lsm
#[test]
fn buyout_pending_slash_statom_lsm_test() {
    let grpc_map = HashMap::from([
        (
            "transfer/channel-0".to_string(),
            HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
        ),
        (
            "transfer/channel-8".to_string(),
            HashMap::from([(ST_ATOM_ON_NEUTRON.to_string(), "stATOM".to_string())]),
        ),
    ]);
    let grpc_query = grpc_query_diff_paths_mock(grpc_map);

    let (mut deps, env) = (mock_dependencies(grpc_query), mock_env());
    let info = get_message_info(&deps.api, "addr0000", &[]);
    let msg = get_default_instantiate_msg(&deps.api);

    let res = instantiate(deps.as_mut(), env.clone(), info, msg.clone());
    assert!(res.is_ok());

    let st_token_info_provider_addr = deps.api.addr_make(DERIVATIVE_TOKEN_PROVIDER_ADDR);
    let derivative_providers = HashMap::from([get_st_atom_denom_info_mock_data(
        st_token_info_provider_addr.to_string(),
        vec![(0, Decimal::from_str("1.30").unwrap())],
    )]);

    let lsm_token_info_provider_addr = deps.api.addr_make(LSM_TOKEN_PROVIDER_ADDR);
    let lsm_provider = Some((
        lsm_token_info_provider_addr.to_string(),
        HashMap::from_iter((0..=2).map(|round_id: u64| {
            (
                round_id,
                HashMap::from([get_validator_info_mock_data(
                    VALIDATOR_1.to_string(),
                    Decimal::one(),
                )]),
            )
        })),
    ));

    setup_multiple_token_info_provider_mocks(
        &mut deps,
        derivative_providers.clone(),
        lsm_provider.clone(),
        true,
    );

    let lockup_amount: u128 = 1000;
    let info = get_message_info(
        &deps.api,
        "addr0000",
        &[Coin::new(lockup_amount, ST_ATOM_ON_NEUTRON.to_string())],
    );
    let msg = ExecuteMsg::LockTokens {
        lock_duration: 3 * ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok());

    let _ = LOCKS_PENDING_SLASHES.save(&mut deps.storage, 0, &Uint128::new(500));

    let buyout_amount_lsm: u128 = 500;
    let buyout_info = get_message_info(
        &deps.api,
        "addr0000",
        &[Coin::new(buyout_amount_lsm, IBC_DENOM_1.to_string())],
    );

    let msg = ExecuteMsg::BuyoutPendingSlash { lock_id: 0 };
    let res = execute(deps.as_mut(), env.clone(), buyout_info.clone(), msg).unwrap();
    // only forward the used amount to the slash tokens receiver
    assert_eq!(res.messages.len(), 1);
    let result = LOCKS_PENDING_SLASHES.may_load(&deps.storage, 0);
    assert!(result.unwrap().is_some());
}
// slash denom: lsm, buyout_denom: atom
#[test]
fn buyout_pending_slash_with_atom_test() {
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
    );
    let (mut deps, env) = (mock_dependencies(grpc_query), mock_env());
    let info = get_message_info(&deps.api, "addr0000", &[]);
    let mut msg = get_default_instantiate_msg(&deps.api);

    msg.token_info_providers = vec![
        get_default_lsm_token_info_provider_init_msg(),
        TokenInfoProviderInstantiateMsg::Base {
            token_group_id: "atom".to_string(),
            denom: ATOM_ON_NEUTRON.to_string(),
        },
    ];

    let res = instantiate(deps.as_mut(), env.clone(), info, msg.clone());
    assert!(res.is_ok());

    let lsm_token_info_provider_addr = deps.api.addr_make(LSM_TOKEN_PROVIDER_ADDR);
    setup_lsm_token_info_provider_mock(
        &mut deps,
        lsm_token_info_provider_addr.clone(),
        vec![(0, vec![(VALIDATOR_1.to_string(), Decimal::one())])],
        true,
    );

    let lockup_amount: u128 = 1000;
    let info = get_message_info(
        &deps.api,
        "addr0000",
        &[Coin::new(lockup_amount, IBC_DENOM_1.to_string())],
    );
    let msg = ExecuteMsg::LockTokens {
        lock_duration: 3 * ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok());

    let _ = LOCKS_PENDING_SLASHES.save(&mut deps.storage, 0, &Uint128::new(500));

    let buyout_amount: u128 = 500;
    let buyout_info = get_message_info(
        &deps.api,
        "addr0000",
        &[Coin::new(buyout_amount, ATOM_ON_NEUTRON.to_string())],
    );

    let msg = ExecuteMsg::BuyoutPendingSlash { lock_id: 0 };
    let res = execute(deps.as_mut(), env.clone(), buyout_info.clone(), msg).unwrap();
    // only forward the used amount to the slash tokens receiver
    assert_eq!(res.messages.len(), 1);
    let result = LOCKS_PENDING_SLASHES.may_load(&deps.storage, 0);
    assert!(result.unwrap().is_none());
}
