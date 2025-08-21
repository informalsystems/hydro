use std::{collections::HashMap, str::FromStr};

use cosmwasm_std::{testing::mock_env, Coin, Decimal, Uint128};

use crate::{
    contract::{execute, instantiate},
    msg::{ExecuteMsg, TokenInfoProviderInstantiateMsg},
    state::{LockEntryV2, LOCKS_MAP_V2, LOCKS_PENDING_SLASHES, USER_LOCKS},
    testing::{
        get_default_instantiate_msg, get_message_info, set_default_validator_for_rounds,
        setup_st_atom_token_info_provider_mock, IBC_DENOM_1, IBC_DENOM_2,
        ONE_MONTH_IN_NANO_SECONDS, ST_ATOM_ON_NEUTRON, VALIDATOR_1, VALIDATOR_1_LST_DENOM_1,
        VALIDATOR_2, VALIDATOR_2_LST_DENOM_1,
    },
    testing_lockup_conversion_dtoken::setup_d_atom_token_info_provider_mock,
    testing_lsm_integration::{
        set_validator_infos_for_round, set_validators_constant_power_ratios_for_rounds,
    },
    testing_mocks::{denom_trace_grpc_query_mock, grpc_query_diff_paths_mock, mock_dependencies},
    token_manager::TokenInfoProviderLSM,
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
    let user_address = deps.api.addr_make("addr0000");
    let info = get_message_info(&deps.api, "addr0000", &[]);
    let mut msg = get_default_instantiate_msg(&deps.api);
    let lsm_token_info_provider = TokenInfoProviderLSM {
        max_validator_shares_participating: 100,
        hub_connection_id: "connection-0".to_string(),
        hub_transfer_channel_id: "channel-0".to_string(),
        icq_update_period: 100,
    };

    msg.token_info_providers = vec![TokenInfoProviderInstantiateMsg::LSM {
        max_validator_shares_participating: lsm_token_info_provider
            .max_validator_shares_participating,
        hub_connection_id: lsm_token_info_provider.hub_connection_id.clone(),
        hub_transfer_channel_id: lsm_token_info_provider.hub_transfer_channel_id.clone(),
        icq_update_period: lsm_token_info_provider.icq_update_period,
    }];

    let res = instantiate(deps.as_mut(), env.clone(), info, msg.clone());
    assert!(res.is_ok());

    set_default_validator_for_rounds(deps.as_mut(), 0, 100);

    let result = set_validator_infos_for_round(
        &mut deps.storage,
        0,
        vec![VALIDATOR_1.to_string(), VALIDATOR_2.to_string()],
    );
    assert!(result.is_ok());

    set_validators_constant_power_ratios_for_rounds(
        deps.as_mut(),
        0,
        100,
        vec![VALIDATOR_1.to_string()],
        vec![Decimal::one(), Decimal::one()],
    );

    // Setup ST_ATOM token info provider
    let token_info_provider_addr = deps.api.addr_make("token_info_provider_1");
    setup_st_atom_token_info_provider_mock(&mut deps, token_info_provider_addr, Decimal::one());

    let contract_address = deps.api.addr_make("dtoken_info_provider");
    let current_ratio = Decimal::from_str("1.15").unwrap();
    setup_d_atom_token_info_provider_mock(&mut deps, contract_address.clone(), current_ratio);

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

    let ids: Vec<u64> = vec![1];

    USER_LOCKS
        .save(
            &mut deps.storage,
            user_address.clone(),
            &ids,
            env.block.height,
        )
        .unwrap();

    let res_lock = LOCKS_MAP_V2.save(
        &mut deps.storage,
        1,
        &LockEntryV2 {
            lock_id: 1,
            funds: Coin::new(Uint128::from(1000u128), IBC_DENOM_1.to_string()),
            owner: user_address.clone(),
            lock_start: env.block.time,
            lock_end: env.block.time.plus_nanos(3 * ONE_MONTH_IN_NANO_SECONDS),
        },
        env.block.height,
    );
    assert!(res_lock.is_ok(), "failed to save lock");

    let _ = LOCKS_PENDING_SLASHES.save(&mut deps.storage, 1, &Uint128::new(500));

    let buyout_amount: u128 = 500;
    let buyout_info = get_message_info(
        &deps.api,
        "addr0000",
        &[Coin::new(buyout_amount, IBC_DENOM_1.to_string())],
    );

    let msg = ExecuteMsg::BuyoutPendingSlash { lock_id: 1 };
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
    let user_address = deps.api.addr_make("addr0000");
    let info = get_message_info(&deps.api, "addr0000", &[]);
    let mut msg = get_default_instantiate_msg(&deps.api);
    let lsm_token_info_provider = TokenInfoProviderLSM {
        max_validator_shares_participating: 100,
        hub_connection_id: "connection-0".to_string(),
        hub_transfer_channel_id: "channel-0".to_string(),
        icq_update_period: 100,
    };

    msg.token_info_providers = vec![TokenInfoProviderInstantiateMsg::LSM {
        max_validator_shares_participating: lsm_token_info_provider
            .max_validator_shares_participating,
        hub_connection_id: lsm_token_info_provider.hub_connection_id.clone(),
        hub_transfer_channel_id: lsm_token_info_provider.hub_transfer_channel_id.clone(),
        icq_update_period: lsm_token_info_provider.icq_update_period,
    }];

    let res = instantiate(deps.as_mut(), env.clone(), info, msg.clone());
    assert!(res.is_ok());

    set_default_validator_for_rounds(deps.as_mut(), 0, 100);

    let result = set_validator_infos_for_round(
        &mut deps.storage,
        0,
        vec![VALIDATOR_1.to_string(), VALIDATOR_2.to_string()],
    );
    assert!(result.is_ok());

    set_validators_constant_power_ratios_for_rounds(
        deps.as_mut(),
        0,
        100,
        vec![VALIDATOR_1.to_string()],
        vec![Decimal::one(), Decimal::one()],
    );

    // Setup ST_ATOM token info provider
    let token_info_provider_addr = deps.api.addr_make("token_info_provider_1");
    setup_st_atom_token_info_provider_mock(&mut deps, token_info_provider_addr, Decimal::one());

    let contract_address = deps.api.addr_make("dtoken_info_provider");
    let current_ratio = Decimal::from_str("1.15").unwrap();
    setup_d_atom_token_info_provider_mock(&mut deps, contract_address.clone(), current_ratio);

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

    let ids: Vec<u64> = vec![1];

    USER_LOCKS
        .save(
            &mut deps.storage,
            user_address.clone(),
            &ids,
            env.block.height,
        )
        .unwrap();

    let res_lock = LOCKS_MAP_V2.save(
        &mut deps.storage,
        1,
        &LockEntryV2 {
            lock_id: 1,
            funds: Coin::new(Uint128::from(1000u128), IBC_DENOM_1.to_string()),
            owner: user_address.clone(),
            lock_start: env.block.time,
            lock_end: env.block.time.plus_nanos(3 * ONE_MONTH_IN_NANO_SECONDS),
        },
        env.block.height,
    );
    assert!(res_lock.is_ok(), "failed to save lock");

    let _ = LOCKS_PENDING_SLASHES.save(&mut deps.storage, 1, &Uint128::new(500));

    let buyout_amount: u128 = 400;
    let buyout_info = get_message_info(
        &deps.api,
        "addr0000",
        &[Coin::new(buyout_amount, IBC_DENOM_1.to_string())],
    );

    let msg = ExecuteMsg::BuyoutPendingSlash { lock_id: 1 };
    let res = execute(deps.as_mut(), env.clone(), buyout_info.clone(), msg).unwrap();
    // only forward the used amount to the slash tokens receiver
    assert_eq!(res.messages.len(), 1);
    let result = LOCKS_PENDING_SLASHES.may_load(&deps.storage, 1);
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
    let user_address = deps.api.addr_make("addr0000");
    let info = get_message_info(&deps.api, "addr0000", &[]);
    let mut msg = get_default_instantiate_msg(&deps.api);
    let lsm_token_info_provider = TokenInfoProviderLSM {
        max_validator_shares_participating: 100,
        hub_connection_id: "connection-0".to_string(),
        hub_transfer_channel_id: "channel-0".to_string(),
        icq_update_period: 100,
    };

    msg.token_info_providers = vec![TokenInfoProviderInstantiateMsg::LSM {
        max_validator_shares_participating: lsm_token_info_provider
            .max_validator_shares_participating,
        hub_connection_id: lsm_token_info_provider.hub_connection_id.clone(),
        hub_transfer_channel_id: lsm_token_info_provider.hub_transfer_channel_id.clone(),
        icq_update_period: lsm_token_info_provider.icq_update_period,
    }];

    let res = instantiate(deps.as_mut(), env.clone(), info, msg.clone());
    assert!(res.is_ok());

    set_default_validator_for_rounds(deps.as_mut(), 0, 100);

    let result = set_validator_infos_for_round(
        &mut deps.storage,
        0,
        vec![VALIDATOR_1.to_string(), VALIDATOR_2.to_string()],
    );
    assert!(result.is_ok());

    set_validators_constant_power_ratios_for_rounds(
        deps.as_mut(),
        0,
        100,
        vec![VALIDATOR_1.to_string()],
        vec![Decimal::one(), Decimal::one()],
    );

    // Setup ST_ATOM token info provider
    let token_info_provider_addr = deps.api.addr_make("token_info_provider_1");
    setup_st_atom_token_info_provider_mock(&mut deps, token_info_provider_addr, Decimal::one());

    let contract_address = deps.api.addr_make("dtoken_info_provider");
    let current_ratio = Decimal::from_str("1.15").unwrap();
    setup_d_atom_token_info_provider_mock(&mut deps, contract_address.clone(), current_ratio);

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

    let ids: Vec<u64> = vec![1];

    USER_LOCKS
        .save(
            &mut deps.storage,
            user_address.clone(),
            &ids,
            env.block.height,
        )
        .unwrap();

    let res_lock = LOCKS_MAP_V2.save(
        &mut deps.storage,
        1,
        &LockEntryV2 {
            lock_id: 1,
            funds: Coin::new(Uint128::from(1000u128), IBC_DENOM_1.to_string()),
            owner: user_address.clone(),
            lock_start: env.block.time,
            lock_end: env.block.time.plus_nanos(3 * ONE_MONTH_IN_NANO_SECONDS),
        },
        env.block.height,
    );
    assert!(res_lock.is_ok(), "failed to save lock");

    let _ = LOCKS_PENDING_SLASHES.save(&mut deps.storage, 1, &Uint128::new(500));

    let buyout_amount: u128 = 600;
    let buyout_info = get_message_info(
        &deps.api,
        "addr0000",
        &[Coin::new(buyout_amount, IBC_DENOM_1.to_string())],
    );

    let msg = ExecuteMsg::BuyoutPendingSlash { lock_id: 1 };
    let res = execute(deps.as_mut(), env.clone(), buyout_info.clone(), msg).unwrap();
    // forward the used amount to the slash tokens receiver and return the excess
    assert_eq!(res.messages.len(), 2);
    let result = LOCKS_PENDING_SLASHES.may_load(&deps.storage, 1);
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
    let user_address = deps.api.addr_make("addr0000");
    let info = get_message_info(&deps.api, "addr0000", &[]);
    let mut msg = get_default_instantiate_msg(&deps.api);
    let lsm_token_info_provider = TokenInfoProviderLSM {
        max_validator_shares_participating: 100,
        hub_connection_id: "connection-0".to_string(),
        hub_transfer_channel_id: "channel-0".to_string(),
        icq_update_period: 100,
    };

    msg.token_info_providers = vec![TokenInfoProviderInstantiateMsg::LSM {
        max_validator_shares_participating: lsm_token_info_provider
            .max_validator_shares_participating,
        hub_connection_id: lsm_token_info_provider.hub_connection_id.clone(),
        hub_transfer_channel_id: lsm_token_info_provider.hub_transfer_channel_id.clone(),
        icq_update_period: lsm_token_info_provider.icq_update_period,
    }];

    let res = instantiate(deps.as_mut(), env.clone(), info, msg.clone());
    assert!(res.is_ok());

    set_default_validator_for_rounds(deps.as_mut(), 0, 100);

    let result = set_validator_infos_for_round(
        &mut deps.storage,
        0,
        vec![VALIDATOR_1.to_string(), VALIDATOR_2.to_string()],
    );
    assert!(result.is_ok());

    set_validators_constant_power_ratios_for_rounds(
        deps.as_mut(),
        0,
        100,
        vec![VALIDATOR_1.to_string(), VALIDATOR_2.to_string()],
        vec![Decimal::percent(95), Decimal::one()],
    );

    // Setup ST_ATOM token info provider
    let token_info_provider_addr = deps.api.addr_make("token_info_provider_1");
    setup_st_atom_token_info_provider_mock(&mut deps, token_info_provider_addr, Decimal::one());

    let contract_address = deps.api.addr_make("dtoken_info_provider");
    let current_ratio = Decimal::from_str("1.15").unwrap();
    setup_d_atom_token_info_provider_mock(&mut deps, contract_address.clone(), current_ratio);

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

    let ids: Vec<u64> = vec![1];

    USER_LOCKS
        .save(
            &mut deps.storage,
            user_address.clone(),
            &ids,
            env.block.height,
        )
        .unwrap();

    let res_lock = LOCKS_MAP_V2.save(
        &mut deps.storage,
        1,
        &LockEntryV2 {
            lock_id: 1,
            funds: Coin::new(Uint128::from(1000u128), IBC_DENOM_2.to_string()),
            owner: user_address.clone(),
            lock_start: env.block.time,
            lock_end: env.block.time.plus_nanos(3 * ONE_MONTH_IN_NANO_SECONDS),
        },
        env.block.height,
    );
    assert!(res_lock.is_ok(), "failed to save lock");

    let _ = LOCKS_PENDING_SLASHES.save(&mut deps.storage, 1, &Uint128::new(500));

    let buyout_amount: u128 = 500;
    let buyout_info = get_message_info(
        &deps.api,
        "addr0000",
        &[Coin::new(buyout_amount, IBC_DENOM_1.to_string())],
    );

    let msg = ExecuteMsg::BuyoutPendingSlash { lock_id: 1 };
    let res = execute(deps.as_mut(), env.clone(), buyout_info.clone(), msg).unwrap();
    // only forward the used amount to the slash tokens receiver
    assert_eq!(res.messages.len(), 1);
    let result = LOCKS_PENDING_SLASHES.may_load(&deps.storage, 1);
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
    let user_address = deps.api.addr_make("addr0000");
    let info = get_message_info(&deps.api, "addr0000", &[]);
    let mut msg = get_default_instantiate_msg(&deps.api);
    let lsm_token_info_provider = TokenInfoProviderLSM {
        max_validator_shares_participating: 100,
        hub_connection_id: "connection-0".to_string(),
        hub_transfer_channel_id: "channel-0".to_string(),
        icq_update_period: 100,
    };

    msg.token_info_providers = vec![TokenInfoProviderInstantiateMsg::LSM {
        max_validator_shares_participating: lsm_token_info_provider
            .max_validator_shares_participating,
        hub_connection_id: lsm_token_info_provider.hub_connection_id.clone(),
        hub_transfer_channel_id: lsm_token_info_provider.hub_transfer_channel_id.clone(),
        icq_update_period: lsm_token_info_provider.icq_update_period,
    }];

    let res = instantiate(deps.as_mut(), env.clone(), info, msg.clone());
    assert!(res.is_ok());

    set_default_validator_for_rounds(deps.as_mut(), 0, 100);

    let result = set_validator_infos_for_round(
        &mut deps.storage,
        0,
        vec![VALIDATOR_1.to_string(), VALIDATOR_2.to_string()],
    );
    assert!(result.is_ok());

    set_validators_constant_power_ratios_for_rounds(
        deps.as_mut(),
        0,
        100,
        vec![VALIDATOR_1.to_string()],
        vec![Decimal::one(), Decimal::one()],
    );

    // Setup ST_ATOM token info provider
    let token_info_provider_addr = deps.api.addr_make("token_info_provider_1");
    setup_st_atom_token_info_provider_mock(
        &mut deps,
        token_info_provider_addr,
        Decimal::from_str("1.30").unwrap(),
    );

    let _contract_address = deps.api.addr_make("dtoken_info_provider");
    let _current_ratio = Decimal::from_str("1.15").unwrap();
    //setup_d_atom_token_info_provider_mock(&mut deps, contract_address.clone(), current_ratio);

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

    let ids: Vec<u64> = vec![1];

    USER_LOCKS
        .save(
            &mut deps.storage,
            user_address.clone(),
            &ids,
            env.block.height,
        )
        .unwrap();

    let res_lock = LOCKS_MAP_V2.save(
        &mut deps.storage,
        1,
        &LockEntryV2 {
            lock_id: 1,
            funds: Coin::new(Uint128::from(1000u128), IBC_DENOM_1.to_string()),
            owner: user_address.clone(),
            lock_start: env.block.time,
            lock_end: env.block.time.plus_nanos(3 * ONE_MONTH_IN_NANO_SECONDS),
        },
        env.block.height,
    );
    assert!(res_lock.is_ok(), "failed to save lock");

    let _ = LOCKS_PENDING_SLASHES.save(&mut deps.storage, 1, &Uint128::new(500));

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

    let msg = ExecuteMsg::BuyoutPendingSlash { lock_id: 1 };
    let res = execute(deps.as_mut(), env.clone(), buyout_info.clone(), msg).unwrap();
    // forward the used amount to the slash tokens receiver and return the excess of other denom
    assert_eq!(res.messages.len(), 2);
    let result = LOCKS_PENDING_SLASHES.may_load(&deps.storage, 1);
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
    let user_address = deps.api.addr_make("addr0000");
    let info = get_message_info(&deps.api, "addr0000", &[]);
    let mut msg = get_default_instantiate_msg(&deps.api);
    let lsm_token_info_provider = TokenInfoProviderLSM {
        max_validator_shares_participating: 100,
        hub_connection_id: "connection-0".to_string(),
        hub_transfer_channel_id: "channel-0".to_string(),
        icq_update_period: 100,
    };

    msg.token_info_providers = vec![TokenInfoProviderInstantiateMsg::LSM {
        max_validator_shares_participating: lsm_token_info_provider
            .max_validator_shares_participating,
        hub_connection_id: lsm_token_info_provider.hub_connection_id.clone(),
        hub_transfer_channel_id: lsm_token_info_provider.hub_transfer_channel_id.clone(),
        icq_update_period: lsm_token_info_provider.icq_update_period,
    }];

    let res = instantiate(deps.as_mut(), env.clone(), info, msg.clone());
    assert!(res.is_ok());

    set_default_validator_for_rounds(deps.as_mut(), 0, 100);

    let result = set_validator_infos_for_round(
        &mut deps.storage,
        0,
        vec![VALIDATOR_1.to_string(), VALIDATOR_2.to_string()],
    );
    assert!(result.is_ok());

    set_validators_constant_power_ratios_for_rounds(
        deps.as_mut(),
        0,
        100,
        vec![VALIDATOR_1.to_string()],
        vec![Decimal::one(), Decimal::one()],
    );

    // Setup ST_ATOM token info provider
    let token_info_provider_addr = deps.api.addr_make("token_info_provider_1");
    setup_st_atom_token_info_provider_mock(
        &mut deps,
        token_info_provider_addr,
        Decimal::from_str("1.30").unwrap(),
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

    let ids: Vec<u64> = vec![1];

    USER_LOCKS
        .save(
            &mut deps.storage,
            user_address.clone(),
            &ids,
            env.block.height,
        )
        .unwrap();

    let res_lock = LOCKS_MAP_V2.save(
        &mut deps.storage,
        1,
        &LockEntryV2 {
            lock_id: 1,
            funds: Coin::new(Uint128::from(1000u128), ST_ATOM_ON_NEUTRON.to_string()),
            owner: user_address.clone(),
            lock_start: env.block.time,
            lock_end: env.block.time.plus_nanos(3 * ONE_MONTH_IN_NANO_SECONDS),
        },
        env.block.height,
    );
    assert!(res_lock.is_ok(), "failed to save lock");

    let _ = LOCKS_PENDING_SLASHES.save(&mut deps.storage, 1, &Uint128::new(500));

    let buyout_amount_lsm: u128 = 500;
    let buyout_info = get_message_info(
        &deps.api,
        "addr0000",
        &[Coin::new(buyout_amount_lsm, IBC_DENOM_1.to_string())],
    );

    let msg = ExecuteMsg::BuyoutPendingSlash { lock_id: 1 };
    let res = execute(deps.as_mut(), env.clone(), buyout_info.clone(), msg).unwrap();
    // only forward the used amount to the slash tokens receiver
    assert_eq!(res.messages.len(), 1);
    let result = LOCKS_PENDING_SLASHES.may_load(&deps.storage, 1);
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
    let user_address = deps.api.addr_make("addr0000");
    let info = get_message_info(&deps.api, "addr0000", &[]);
    let mut msg = get_default_instantiate_msg(&deps.api);
    let lsm_token_info_provider = TokenInfoProviderLSM {
        max_validator_shares_participating: 100,
        hub_connection_id: "connection-0".to_string(),
        hub_transfer_channel_id: "channel-0".to_string(),
        icq_update_period: 100,
    };

    msg.token_info_providers = vec![
        TokenInfoProviderInstantiateMsg::LSM {
            max_validator_shares_participating: lsm_token_info_provider
                .max_validator_shares_participating,
            hub_connection_id: lsm_token_info_provider.hub_connection_id.clone(),
            hub_transfer_channel_id: lsm_token_info_provider.hub_transfer_channel_id.clone(),
            icq_update_period: lsm_token_info_provider.icq_update_period,
        },
        TokenInfoProviderInstantiateMsg::Base {
            token_group_id: "atom".to_string(),
            denom: "ibc/C4CFF46FD6DE35CA4CF4CE031E643C8FDC9BA4B99AE598E9B0ED98FE3A2319F9"
                .to_string(),
        },
    ];

    let res = instantiate(deps.as_mut(), env.clone(), info, msg.clone());
    assert!(res.is_ok());

    set_default_validator_for_rounds(deps.as_mut(), 0, 100);

    let result = set_validator_infos_for_round(
        &mut deps.storage,
        0,
        vec![VALIDATOR_1.to_string(), VALIDATOR_2.to_string()],
    );
    assert!(result.is_ok());

    set_validators_constant_power_ratios_for_rounds(
        deps.as_mut(),
        0,
        100,
        vec![VALIDATOR_1.to_string()],
        vec![Decimal::one(), Decimal::one()],
    );

    // Setup ST_ATOM token info provider
    let token_info_provider_addr = deps.api.addr_make("token_info_provider_1");
    setup_st_atom_token_info_provider_mock(&mut deps, token_info_provider_addr, Decimal::one());

    let contract_address = deps.api.addr_make("dtoken_info_provider");
    let current_ratio = Decimal::from_str("1.15").unwrap();
    setup_d_atom_token_info_provider_mock(&mut deps, contract_address.clone(), current_ratio);

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

    let ids: Vec<u64> = vec![1];

    USER_LOCKS
        .save(
            &mut deps.storage,
            user_address.clone(),
            &ids,
            env.block.height,
        )
        .unwrap();

    let res_lock = LOCKS_MAP_V2.save(
        &mut deps.storage,
        1,
        &LockEntryV2 {
            lock_id: 1,
            funds: Coin::new(Uint128::from(1000u128), IBC_DENOM_1.to_string()),
            owner: user_address.clone(),
            lock_start: env.block.time,
            lock_end: env.block.time.plus_nanos(3 * ONE_MONTH_IN_NANO_SECONDS),
        },
        env.block.height,
    );
    assert!(res_lock.is_ok(), "failed to save lock");

    let _ = LOCKS_PENDING_SLASHES.save(&mut deps.storage, 1, &Uint128::new(500));

    let buyout_amount: u128 = 500;
    let buyout_info = get_message_info(
        &deps.api,
        "addr0000",
        &[Coin::new(buyout_amount, ATOM_ON_NEUTRON.to_string())],
    );

    let msg = ExecuteMsg::BuyoutPendingSlash { lock_id: 1 };
    let res = execute(deps.as_mut(), env.clone(), buyout_info.clone(), msg).unwrap();
    // only forward the used amount to the slash tokens receiver
    assert_eq!(res.messages.len(), 1);
    let result = LOCKS_PENDING_SLASHES.may_load(&deps.storage, 1);
    assert!(result.unwrap().is_none());
}
