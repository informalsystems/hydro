use cosmwasm_std::{testing::mock_env, Coin, Decimal, Uint128};
use std::collections::HashMap;

use crate::{
    contract::{execute, get_current_lock_composition, get_lock_ancestor_depth, instantiate},
    msg::ExecuteMsg,
    state::{LOCK_ID_EXPIRY, LOCK_ID_TRACKING, REVERSE_LOCK_ID_TRACKING},
    testing::{
        get_address_as_str, get_default_instantiate_msg, get_message_info,
        set_default_validator_for_rounds, IBC_DENOM_1, ONE_MONTH_IN_NANO_SECONDS,
        VALIDATOR_1_LST_DENOM_1,
    },
    testing_mocks::{denom_trace_grpc_query_mock, mock_dependencies},
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

    let depth = get_lock_ancestor_depth(&deps.as_ref(), env, 6, lock_expiry_duration_seconds);
    assert!(depth.is_ok());
    let depth_value = depth.unwrap();
    assert!(depth_value <= lock_depth_limit);
    assert!(depth_value == 4);
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
    set_default_validator_for_rounds(deps.as_mut(), 0, 3);

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
    );

    assert!(depth.is_ok());
    assert_eq!(depth.unwrap(), 4);

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
    );

    assert_eq!(depth.unwrap(), 3);

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
    );

    assert_eq!(depth.unwrap(), 2);

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
    );

    assert_eq!(depth.unwrap(), 1);

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
    );

    assert_eq!(depth.unwrap(), 0);
}

#[test]
fn test_infinite_loop_in_get_lock_ancestor_depth() {
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
    );
    let mut deps = mock_dependencies(grpc_query);
    let env = mock_env();

    // Simulate a cycle in REVERSE_LOCK_ID_TRACKING
    REVERSE_LOCK_ID_TRACKING
        .save(deps.as_mut().storage, 1, &vec![2])
        .unwrap();
    REVERSE_LOCK_ID_TRACKING
        .save(deps.as_mut().storage, 2, &vec![1])
        .unwrap();

    // Call the function with a lock ID involved in the cycle
    let result = get_lock_ancestor_depth(&deps.as_ref(), env, 1, 1000);

    // Verify there is no infinite loop
    assert!(result.is_ok());
}
