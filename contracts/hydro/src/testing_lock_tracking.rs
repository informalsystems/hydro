use cosmwasm_std::{testing::mock_env, Coin, Decimal, Uint128};
use std::collections::HashMap;

use crate::{
    contract::{execute, get_current_lock_composition, get_lock_ancestor_depth, instantiate},
    msg::ExecuteMsg,
    state::{LOCK_ID_EXPIRY, LOCK_ID_TRACKING, REVERSE_LOCK_ID_TRACKING},
    testing::{
        get_address_as_str, get_default_instantiate_msg, get_message_info,
        setup_lsm_token_info_provider_mock, IBC_DENOM_1, LSM_TOKEN_PROVIDER_ADDR,
        ONE_MONTH_IN_NANO_SECONDS, VALIDATOR_1, VALIDATOR_1_LST_DENOM_1,
    },
    testing_mocks::denom_trace_grpc_query_mock,
};

#[test]
fn test_get_current_lock_composition() {
    let user_address = "addr0000";
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
    );
    let (mut deps, env) = (
        crate::testing_mocks::mock_dependencies(grpc_query),
        mock_env(),
    );
    let info = get_message_info(&deps.api, user_address, &[]);
    let instantiate_msg = get_default_instantiate_msg(&deps.api);
    instantiate(deps.as_mut(), env.clone(), info.clone(), instantiate_msg).unwrap();

    // First split
    let starting_lock_entry_1 = 1;
    let resulting_lock_amount_1 = Uint128::from(50u128);
    let amount_1 = Uint128::from(50u128);

    let total_1 = resulting_lock_amount_1 + amount_1;
    let frac_1 = Decimal::from_ratio(resulting_lock_amount_1, total_1);
    let frac_2 = Decimal::one() - frac_1;

    let new_lock_id_1 = 2;
    let new_lock_id_2 = 3;
    let _ = LOCK_ID_TRACKING.save(
        &mut deps.storage,
        starting_lock_entry_1,
        &vec![(new_lock_id_1, frac_1), (new_lock_id_2, frac_2)],
    );

    // Second split
    let starting_lock_entry_2 = 2;
    let resulting_lock_amount_2 = Uint128::from(70u128);
    let amount_2 = Uint128::from(30u128);

    let total_2 = resulting_lock_amount_2 + amount_2;
    let frac_1 = Decimal::from_ratio(resulting_lock_amount_2, total_2);
    let frac_2 = Decimal::one() - frac_1;

    let new_lock_id_1 = 4;
    let new_lock_id_2 = 5;
    let _ = LOCK_ID_TRACKING.save(
        &mut deps.storage,
        starting_lock_entry_2,
        &vec![(new_lock_id_1, frac_1), (new_lock_id_2, frac_2)],
    );

    // Merge
    let from_id_first = 3;
    let from_id_second = 4;
    let into_lock_id = 6;
    let _ = LOCK_ID_TRACKING.save(
        &mut deps.storage,
        from_id_first,
        &vec![(into_lock_id, Decimal::one())],
    );

    let _ = LOCK_ID_TRACKING.save(
        &mut deps.storage,
        from_id_second,
        &vec![(into_lock_id, Decimal::one())],
    );

    let res = get_current_lock_composition(&deps.as_ref(), starting_lock_entry_1);
    assert!(res.is_ok());
    let expected = vec![
        (5, Decimal::percent(15)), // 0.15
        (6, Decimal::percent(85)), // 0.85
    ];
    assert_eq!(res.unwrap(), expected);
}

#[test]
fn test_get_lock_ancestor_depth() {
    let user_address = "addr0000";
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
    );
    let (mut deps, env) = (
        crate::testing_mocks::mock_dependencies(grpc_query),
        mock_env(),
    );
    let info = get_message_info(&deps.api, user_address, &[]);
    let instantiate_msg = get_default_instantiate_msg(&deps.api);
    instantiate(deps.as_mut(), env.clone(), info.clone(), instantiate_msg).unwrap();

    let lock_depth_limit = 5;
    let lock_expiry_duration_seconds = 60 * 60 * 24 * 30 * 6; // 6 months

    // First split
    let starting_lock_entry_1 = 1;
    let new_lock_id_1 = 2;
    let new_lock_id_2 = 3;

    // Reverse tracking
    let _ = REVERSE_LOCK_ID_TRACKING.save(
        &mut deps.storage,
        new_lock_id_1,
        &vec![starting_lock_entry_1],
    );
    let _ = REVERSE_LOCK_ID_TRACKING.save(
        &mut deps.storage,
        new_lock_id_2,
        &vec![starting_lock_entry_1],
    );

    // Second split
    let starting_lock_entry_2 = 2;
    let new_lock_id_1 = 4;
    let new_lock_id_2 = 5;

    // Reverse tracking
    let _ = REVERSE_LOCK_ID_TRACKING.save(
        &mut deps.storage,
        new_lock_id_1,
        &vec![starting_lock_entry_2],
    );
    let _ = REVERSE_LOCK_ID_TRACKING.save(
        &mut deps.storage,
        new_lock_id_2,
        &vec![starting_lock_entry_2],
    );

    // Merge
    let from_id_first = 3;
    let from_id_second = 4;
    let into_lock_id = 6;
    let parents = vec![from_id_first, from_id_second];
    let _ = REVERSE_LOCK_ID_TRACKING.save(&mut deps.storage, into_lock_id, &parents);

    let depth = get_lock_ancestor_depth(&deps.as_ref(), env, 6, lock_expiry_duration_seconds, None);
    assert!(depth.is_ok());
    let (depth_value, cache) = depth.unwrap();
    assert!(depth_value <= lock_depth_limit);
    assert!(depth_value == 4);

    let expected_cache = vec![(1, 1), (2, 2), (3, 2), (4, 3), (6, 4)];

    // Ensure all expected entries exist in the cache
    for (lock_id, expected_depth) in expected_cache {
        let actual = cache.get(&lock_id).copied();
        assert_eq!(
            actual,
            Some(expected_depth),
            "Cache missing or wrong for lock {lock_id}"
        );
    }
}
#[test]
fn test_split_merge_composition_and_depth() {
    let user_address = "addr0000";
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
    );
    let (mut deps, mut env) = (
        crate::testing_mocks::mock_dependencies(grpc_query),
        mock_env(),
    );
    let info = get_message_info(&deps.api, user_address, &[]);
    let mut instantiate_msg = get_default_instantiate_msg(&deps.api);
    instantiate_msg.round_length = ONE_MONTH_IN_NANO_SECONDS;
    instantiate_msg.whitelist_admins = vec![get_address_as_str(&deps.api, user_address)];
    instantiate(deps.as_mut(), env.clone(), info.clone(), instantiate_msg).unwrap();

    let lsm_token_info_provider_addr = deps.api.addr_make(LSM_TOKEN_PROVIDER_ADDR);
    setup_lsm_token_info_provider_mock(
        &mut deps,
        lsm_token_info_provider_addr.clone(),
        vec![
            (0, vec![(VALIDATOR_1.to_string(), Decimal::one())]),
            (1, vec![(VALIDATOR_1.to_string(), Decimal::one())]),
            (2, vec![(VALIDATOR_1.to_string(), Decimal::one())]),
        ],
        true,
    );

    let lock_expiry_duration_seconds = 60 * 60 * 24 * 30 * 6; // 6 months

    // Lock tokens
    let initial_lock_amount = Uint128::from(50000u128);
    let funds = vec![Coin::new(initial_lock_amount.u128(), IBC_DENOM_1)];
    let lock_info = get_message_info(&deps.api, user_address, &funds);
    env.block.time = env.block.time.plus_nanos(1);

    let info = get_message_info(&deps.api, user_address, &[]);
    let first_lock_id = 0;

    let lock_res = execute(
        deps.as_mut(),
        env.clone(),
        lock_info.clone(),
        ExecuteMsg::LockTokens {
            lock_duration: 3 * ONE_MONTH_IN_NANO_SECONDS,
            proof: None,
        },
    );
    assert!(lock_res.is_ok());

    // Split the lockup
    let split_amount_1 = Uint128::from(10000u128);
    let split_res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::SplitLock {
            lock_id: first_lock_id,
            amount: split_amount_1,
        },
    );
    assert!(split_res.is_ok());
    // Split the lockup 2
    let split_amount_2 = Uint128::from(20000u128);
    let second_lock_id = 1;
    let split_res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::SplitLock {
            lock_id: second_lock_id,
            amount: split_amount_2,
        },
    );
    assert!(split_res.is_ok());

    // Merge lockups into new lockup
    let merge_ids = vec![2, 3];

    let merge_res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::MergeLocks {
            lock_ids: merge_ids.clone(),
        },
    );
    assert!(merge_res.is_ok());

    let res = get_current_lock_composition(&deps.as_ref(), first_lock_id);
    assert!(res.is_ok());
    let expected = vec![
        (4u64, Decimal::percent(40)), // 0.4
        (5u64, Decimal::percent(60)), // 0.6
    ];

    assert_eq!(res.unwrap(), expected);

    let lock_id = 5;
    let depth = get_lock_ancestor_depth(
        &deps.as_ref(),
        env.clone(),
        lock_id,
        lock_expiry_duration_seconds,
        None,
    );

    assert!(depth.is_ok());
    let (depth_value, _) = depth.unwrap();
    assert_eq!(depth_value, 4);

    // Simulate lock 0 expired
    let fake_expiry = env
        .clone()
        .block
        .time
        .minus_seconds(lock_expiry_duration_seconds + 1);
    let _ = LOCK_ID_EXPIRY.save(&mut deps.storage, 0, &fake_expiry);

    let lock_id = 5;
    let depth = get_lock_ancestor_depth(
        &deps.as_ref(),
        env.clone(),
        lock_id,
        lock_expiry_duration_seconds,
        None,
    );
    let (depth_value, _) = depth.unwrap();

    assert_eq!(depth_value, 3);

    // Simulate lock 1 and 2 expired
    let fake_expiry = env
        .clone()
        .block
        .time
        .minus_seconds(lock_expiry_duration_seconds + 1);
    let _ = LOCK_ID_EXPIRY.save(&mut deps.storage, 1, &fake_expiry);

    let fake_expiry = env
        .clone()
        .block
        .time
        .minus_seconds(lock_expiry_duration_seconds + 1);
    let _ = LOCK_ID_EXPIRY.save(&mut deps.storage, 2, &fake_expiry);

    let lock_id = 5;
    let depth = get_lock_ancestor_depth(
        &deps.as_ref(),
        env.clone(),
        lock_id,
        lock_expiry_duration_seconds,
        None,
    );

    let (depth_value, _) = depth.unwrap();

    assert_eq!(depth_value, 2);

    // Simulate lock 3 is expired
    let fake_expiry = env
        .clone()
        .block
        .time
        .minus_seconds(lock_expiry_duration_seconds + 1);
    let _ = LOCK_ID_EXPIRY.save(&mut deps.storage, 3, &fake_expiry);

    let lock_id = 5;
    let depth = get_lock_ancestor_depth(
        &deps.as_ref(),
        env.clone(),
        lock_id,
        lock_expiry_duration_seconds,
        None,
    );
    let (depth_value, _) = depth.unwrap();

    assert_eq!(depth_value, 1);

    // Simulate lock 5 is expired
    let fake_expiry = env
        .clone()
        .block
        .time
        .minus_seconds(lock_expiry_duration_seconds + 1);
    let _ = LOCK_ID_EXPIRY.save(&mut deps.storage, 5, &fake_expiry);

    let lock_id = 5;
    let depth = get_lock_ancestor_depth(
        &deps.as_ref(),
        env.clone(),
        lock_id,
        lock_expiry_duration_seconds,
        None,
    );

    let (depth_value, _) = depth.unwrap();

    assert_eq!(depth_value, 0);
}
