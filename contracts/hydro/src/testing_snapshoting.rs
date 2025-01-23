use std::collections::HashMap;

use cosmwasm_std::{testing::mock_env, Coin, Storage, Timestamp};

use crate::{
    contract::{execute, instantiate},
    msg::{ExecuteMsg, InstantiateMsg},
    state::{HEIGHT_TO_ROUND, ROUND_TO_HEIGHT_RANGE, USER_LOCKS},
    testing::{
        get_default_instantiate_msg, get_message_info, IBC_DENOM_1, ONE_DAY_IN_NANO_SECONDS,
        VALIDATOR_1, VALIDATOR_1_LST_DENOM_1,
    },
    testing_lsm_integration::set_validator_infos_for_round,
    testing_mocks::{denom_trace_grpc_query_mock, mock_dependencies},
};

#[test]
fn test_user_locks_snapshoting() {
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
    );

    let user = "addr0000";
    let initial_block_time = Timestamp::from_nanos(1737540000000000000);
    let initial_block_height = 19_185_000;
    let (mut deps, mut env) = (mock_dependencies(grpc_query), mock_env());
    let user_addr = deps.api.addr_make(user);

    env.block.time = initial_block_time;
    env.block.height = initial_block_height;

    let info = get_message_info(&deps.api, user, &[]);
    let instantiate_msg = InstantiateMsg {
        first_round_start: env.block.time,
        round_length: 30 * ONE_DAY_IN_NANO_SECONDS,
        lock_epoch_length: 30 * ONE_DAY_IN_NANO_SECONDS,
        ..get_default_instantiate_msg(&deps.api)
    };

    let res = instantiate(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        instantiate_msg.clone(),
    );
    assert!(res.is_ok());

    let res = set_validator_infos_for_round(&mut deps.storage, 0, vec![VALIDATOR_1.to_string()]);
    assert!(res.is_ok());

    env.block.time = env.block.time.plus_days(1);
    env.block.height += 35000;

    let info = get_message_info(
        &deps.api,
        user,
        &[Coin::new(1000u64, IBC_DENOM_1.to_string())],
    );
    let msg = ExecuteMsg::LockTokens {
        lock_duration: instantiate_msg.lock_epoch_length,
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok(), "error: {:?}", res);

    let current_round = 0;
    let current_round_expected_initial_height = env.block.height;
    verify_round_height_mappings(
        &deps.storage,
        current_round,
        (current_round_expected_initial_height, env.block.height),
        env.block.height,
    );

    env.block.time = env.block.time.plus_days(1);
    env.block.height += 35000;

    let msg = ExecuteMsg::LockTokens {
        lock_duration: instantiate_msg.lock_epoch_length,
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok(), "error: {:?}", res);

    verify_round_height_mappings(
        &deps.storage,
        current_round,
        (current_round_expected_initial_height, env.block.height),
        env.block.height,
    );

    let mut expected_user_locks = vec![(env.block.height + 1, vec![0, 1])];

    env.block.time = env.block.time.plus_days(1);
    env.block.height += 35000;

    let msg = ExecuteMsg::RefreshLockDuration {
        lock_ids: vec![0],
        lock_duration: 3 * instantiate_msg.lock_epoch_length,
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok(), "error: {:?}", res);

    verify_round_height_mappings(
        &deps.storage,
        current_round,
        (current_round_expected_initial_height, env.block.height),
        env.block.height,
    );

    env.block.time = env.block.time.plus_days(1);
    env.block.height += 35000;

    let msg = ExecuteMsg::LockTokens {
        lock_duration: instantiate_msg.lock_epoch_length,
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok(), "error: {:?}", res);

    expected_user_locks.push((env.block.height + 1, vec![0, 1, 2]));

    // Advance the chain by 35 days from initial time so that the user can unlock locks 1 and 2
    env.block.time = initial_block_time.plus_nanos(35 * ONE_DAY_IN_NANO_SECONDS + 1);
    env.block.height = initial_block_height + 35 * 35000;

    let current_round = 1;
    let current_round_expected_initial_height = env.block.height;

    let msg = ExecuteMsg::UnlockTokens {
        lock_ids: Some(vec![1, 2]),
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok(), "error: {:?}", res);

    expected_user_locks.push((env.block.height + 1, vec![0]));

    verify_round_height_mappings(
        &deps.storage,
        current_round,
        (current_round_expected_initial_height, env.block.height),
        env.block.height,
    );

    // Advance the chain by 95 days from initial time so that the user can unlock lock 0
    env.block.time = initial_block_time.plus_nanos(95 * ONE_DAY_IN_NANO_SECONDS + 1);
    env.block.height = initial_block_height + 95 * 35000;

    let current_round = 3;
    let current_round_expected_initial_height = env.block.height;

    let msg = ExecuteMsg::UnlockTokens {
        lock_ids: Some(vec![0]),
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok(), "error: {:?}", res);

    expected_user_locks.push((env.block.height + 1, vec![]));

    verify_round_height_mappings(
        &deps.storage,
        current_round,
        (current_round_expected_initial_height, env.block.height),
        env.block.height,
    );

    // Verify that USER_LOCKS return expected values at a given heights
    for expected_locks in expected_user_locks {
        // unwrap() on purpose- it should never fail
        let user_locks = USER_LOCKS
            .may_load_at_height(&deps.storage, user_addr.clone(), expected_locks.0)
            .unwrap()
            .unwrap();
        assert_eq!(expected_locks.1, user_locks);
    }
}

fn verify_round_height_mappings(
    storage: &impl Storage,
    round_id: u64,
    expected_round_height_range: (u64, u64),
    height_to_check: u64,
) {
    let height_range = ROUND_TO_HEIGHT_RANGE
        .load(storage, round_id)
        .unwrap_or_default();
    assert_eq!(
        height_range.lowest_known_height,
        expected_round_height_range.0
    );
    assert_eq!(
        height_range.highest_known_height,
        expected_round_height_range.1
    );

    let height_round = HEIGHT_TO_ROUND
        .load(storage, height_to_check)
        .unwrap_or_default();
    assert_eq!(height_round, round_id);
}
