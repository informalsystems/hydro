use cosmwasm_std::{testing::mock_env, Coin, Decimal, Order, Uint128};
use std::collections::HashMap;

use crate::{
    contract::{
        execute, get_current_lock_composition, get_lock_ancestor_depth, instantiate,
        LOCK_EXPIRY_DURATION_SECONDS,
    },
    msg::ExecuteMsg,
    state::{LOCK_ID_EXPIRY, LOCK_ID_TRACKING, REVERSE_LOCK_ID_TRACKING},
    testing::{
        get_address_as_str, get_default_instantiate_msg, get_message_info,
        set_default_validator_for_rounds, IBC_DENOM_1, ONE_MONTH_IN_NANO_SECONDS,
        VALIDATOR_1_LST_DENOM_1,
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
    let mut instantiate_msg = get_default_instantiate_msg(&deps.api);
    instantiate_msg.round_length = ONE_MONTH_IN_NANO_SECONDS;
    instantiate_msg.whitelist_admins = vec![get_address_as_str(&deps.api, user_address)];
    instantiate(deps.as_mut(), env.clone(), info.clone(), instantiate_msg).unwrap();
    set_default_validator_for_rounds(deps.as_mut(), 0, 3);

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

    let tracking_entries = LOCK_ID_TRACKING.range(&deps.storage, None, None, Order::Ascending);

    println!("--- LOCK_ID_TRACKING contents ---");
    for entry in tracking_entries {
        let (key, value) = entry.unwrap();
        println!("lock_id {} => {:?}", key, value);
    }

    let res = get_current_lock_composition(&deps.as_ref(), starting_lock_entry_1);
    assert!(res.is_ok());
    println!("Composition: {:?}", res);
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
    let mut instantiate_msg = get_default_instantiate_msg(&deps.api);
    instantiate_msg.round_length = ONE_MONTH_IN_NANO_SECONDS;
    instantiate_msg.whitelist_admins = vec![get_address_as_str(&deps.api, user_address)];
    instantiate(deps.as_mut(), env.clone(), info.clone(), instantiate_msg).unwrap();
    set_default_validator_for_rounds(deps.as_mut(), 0, 3);

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

    // Print REVERSE_LOCK_ID_TRACKING before calling get_lock_ancestor_depth
    REVERSE_LOCK_ID_TRACKING
        .range(&deps.storage, None, None, cosmwasm_std::Order::Ascending)
        .for_each(|item| {
            let (key, parents) = item.unwrap();
            println!("REVERSE_TRACKING[{}] = {:?}", key, parents);
        });

    // Call and print result of get_lock_ancestor_depth
    let depth = get_lock_ancestor_depth(&deps.as_ref(), env, 6);
    println!("Ancestor depth for {} = {:?}", into_lock_id, depth);
    assert!(depth.is_ok());
    let depth_value = depth.unwrap();
    assert!(depth_value <= crate::contract::LOCK_DEPTH_LIMIT);
    assert!(depth_value == 3);
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
    let mut merge_ids = vec![];
    merge_ids.push(2);
    merge_ids.push(3);

    let merge_res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::MergeLocks {
            lock_ids: merge_ids.clone(),
        },
    );
    assert!(merge_res.is_ok());
    // Print LOCK_ID_TRACKING
    println!("--- LOCK_ID_TRACKING ---");
    for item in LOCK_ID_TRACKING.range(&deps.storage, None, None, cosmwasm_std::Order::Ascending) {
        let (lock_id, children) = item.unwrap();
        println!("Lock ID {} => {:?}", lock_id, children);
    }

    // Print REVERSE_LOCK_ID_TRACKING
    println!("--- REVERSE_LOCK_ID_TRACKING ---");
    for item in
        REVERSE_LOCK_ID_TRACKING.range(&deps.storage, None, None, cosmwasm_std::Order::Ascending)
    {
        let (lock_id, parents) = item.unwrap();
        println!("Lock ID {} => {:?}", lock_id, parents);
    }

    // Print LOCK_ID_EXPIRY
    println!("--- LOCK_ID_EXPIRY ---");
    for item in LOCK_ID_EXPIRY.range(&deps.storage, None, None, cosmwasm_std::Order::Ascending) {
        let (lock_id, timestamp) = item.unwrap();
        println!("Lock ID {} => {}", lock_id, timestamp);
    }

    let res = get_current_lock_composition(&deps.as_ref(), first_lock_id);
    assert!(res.is_ok());
    let expected = vec![
        (4u64, Decimal::percent(40)), // 0.4
        (5u64, Decimal::percent(60)), // 0.6
    ];

    assert_eq!(res.unwrap(), expected);

    let lock_id = 5;
    let depth = get_lock_ancestor_depth(&deps.as_ref(), env.clone(), lock_id);
    println!("Ancestor depth for {} = {:?}", lock_id, depth);
    assert!(depth.is_ok());
    assert_eq!(depth.unwrap(), 3);

    // Simulate lock 1 is expired
    let fake_expiry = env
        .clone()
        .block
        .time
        .minus_seconds(LOCK_EXPIRY_DURATION_SECONDS + 1);
    let _ = LOCK_ID_EXPIRY.save(&mut deps.storage, 1, &fake_expiry);

    // Call the method
    let lock_id = 5;
    let depth = get_lock_ancestor_depth(&deps.as_ref(), env.clone(), lock_id);
    println!("Ancestor depth for {} = {:?}", lock_id, depth);
    assert_eq!(depth.unwrap(), 2); // Because ancestor 1 is expired

    // Simulate lock 3 is expired
    let fake_expiry = env
        .clone()
        .block
        .time
        .minus_seconds(LOCK_EXPIRY_DURATION_SECONDS + 1);
    let _ = LOCK_ID_EXPIRY.save(&mut deps.storage, 3, &fake_expiry);

    // Call the method
    let lock_id = 5;
    let depth = get_lock_ancestor_depth(&deps.as_ref(), env.clone(), lock_id);
    println!("Ancestor depth for {} = {:?}", lock_id, depth);
    assert_eq!(depth.unwrap(), 0); // Because ancestor 3 is expired
}
