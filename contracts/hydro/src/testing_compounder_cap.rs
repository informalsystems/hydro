use cosmos_sdk_proto::prost::Message;
use std::collections::HashMap;

use cosmwasm_std::{
    testing::{mock_env, MockApi, MockStorage},
    Addr, Binary, Coin, Env, OwnedDeps, Storage, Timestamp, Uint128,
};
use neutron_sdk::{
    bindings::{query::NeutronQuery, types::StorageValue},
    interchain_queries::{types::QueryType, v047::types::STAKING_STORE_KEY},
    sudo::msg::SudoMsg,
};

use crate::{
    contract::{execute, instantiate, sudo},
    lsm_integration::get_total_power_for_round,
    msg::ExecuteMsg,
    state::{EXTRA_LOCKED_TOKENS_CURRENT_USERS, EXTRA_LOCKED_TOKENS_ROUND_TOTAL, LOCKED_TOKENS},
    testing::{
        get_address_as_str, get_default_instantiate_msg, get_message_info, IBC_DENOM_1,
        IBC_DENOM_2, IBC_DENOM_3, ONE_DAY_IN_NANO_SECONDS, VALIDATOR_1, VALIDATOR_1_LST_DENOM_1,
        VALIDATOR_2, VALIDATOR_2_LST_DENOM_1, VALIDATOR_3, VALIDATOR_3_LST_DENOM_1,
    },
    testing_lsm_integration::set_validator_infos_for_round,
    testing_mocks::{
        custom_interchain_query_mock, denom_trace_grpc_query_mock, mock_dependencies, ICQMockData,
        MockQuerier,
    },
    testing_validators_icqs::get_mock_validator,
    validators_icqs::TOKENS_TO_SHARES_MULTIPLIER,
};

const ROUND_LENGTH: u64 = 30 * ONE_DAY_IN_NANO_SECONDS;
const LOCK_EPOCH_LENGTH: u64 = ROUND_LENGTH;
const TEN_DAYS_IN_NANOS: u64 = 10 * ONE_DAY_IN_NANO_SECONDS;
const FIRST_ROUND_START: Timestamp = Timestamp::from_nanos(1737540000000000000); // Wednesday, January 22, 2025 10:00:00 AM
const INITIAL_BLOCK_HEIGHT: u64 = 19_185_000;
const BLOCKS_PER_DAY: u64 = 35_000;

// 1.  Round 0: Have 3 users fill the total cap by locking 3 different tokens for different duration (1, 6, 12 rounds).
// 2.  Round 0: Update config to increase total_cap and set extra_cap starting from round 1.
// 3.  Round 0: Update config to close the extra_cap after some time in round 1.
// 4.  Round 0: Update all validator power ratios to verify that the total voting power changes, and users
//     voting power also gets updated proportinally.
// 5.  Round 1: Have the first known user unlock the expired lockup, to test voting power computation for previous round.
// 6.  Round 1: Have the first known user lock some tokens in public_cap, then a completely new user lock tokens
//     in public cap (try more than allowed, then lock below public_cap).
// 7.  Round 1: Have the known user from previous step lock more to fill the public_cap and some more into extra_cap.
// 8.  Round 1: Have the same known user try to lock in extra_cap more than it should be allowed.
// 9.  Round 1: Have the same known user lock the most it should be allowed in the extra_cap.
// 10. Round 1: Have other two known users lock as much as they should be allowed in the extra_cap.
// 11. Round 1: Update config to increase total_cap and set extra_cap starting from round 2.
// 12. Round 1: Update config to close the extra_cap after some time in round 2.
// 13. Round 2: Have a completely new user lock tokens to fill up the public_cap, then try to lock more.
// 14. Round 2: Have a known user lock maximum allowed in extra cap.
// 15. Round 2: Advance the chain to end the extra_cap duration and have a user from step #13 lock
//     additional amount that matches the entire amount previously reserved for extra_cap.
#[test]
fn test_compounder_cap() {
    let whitelist_admin = "addr0000";
    let user1 = "addr0001";
    let user2 = "addr0002";
    let user3 = "addr0003";
    let user4 = "addr0004";
    let user5 = "addr0005";

    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([
            (IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string()),
            (IBC_DENOM_2.to_string(), VALIDATOR_2_LST_DENOM_1.to_string()),
            (IBC_DENOM_3.to_string(), VALIDATOR_3_LST_DENOM_1.to_string()),
        ]),
    );
    let (mut deps, mut env) = (mock_dependencies(grpc_query), mock_env());

    env.block.time = FIRST_ROUND_START;
    env.block.height = INITIAL_BLOCK_HEIGHT;

    let user1_addr = deps.api.addr_make(user1);
    let user2_addr = deps.api.addr_make(user2);
    let user3_addr = deps.api.addr_make(user3);
    let user4_addr = deps.api.addr_make(user4);
    let user5_addr = deps.api.addr_make(user5);

    let mut msg = get_default_instantiate_msg(&deps.api);

    msg.lock_epoch_length = LOCK_EPOCH_LENGTH;
    msg.round_length = ROUND_LENGTH;
    msg.first_round_start = env.block.time;
    msg.max_locked_tokens = Uint128::new(30000);
    msg.whitelist_admins = vec![get_address_as_str(&deps.api, whitelist_admin)];

    let admin_msg_info = get_message_info(&deps.api, whitelist_admin, &[]);
    let res = instantiate(
        deps.as_mut(),
        env.clone(),
        admin_msg_info.clone(),
        msg.clone(),
    );
    assert!(res.is_ok());

    // Set all 3 validators power ratio in round 0 to 1
    let res = set_validator_infos_for_round(
        &mut deps.storage,
        0,
        vec![
            VALIDATOR_1.to_string(),
            VALIDATOR_2.to_string(),
            VALIDATOR_3.to_string(),
        ],
    );
    assert!(res.is_ok());

    // Advance the chain 1 day into round 0
    env.block.time = env.block.time.plus_days(1);
    env.block.height += BLOCKS_PER_DAY;

    // 1.  Round 0: Have 3 users fill the total cap by locking 3 different tokens for different duration (1, 6, 12 rounds).
    let locking_infos: Vec<(&str, u64, Coin, Option<&str>)> = vec![
        (
            user1,
            LOCK_EPOCH_LENGTH,
            Coin::new(10000u64, IBC_DENOM_1.to_string()),
            None,
        ),
        (
            user2,
            6 * LOCK_EPOCH_LENGTH,
            Coin::new(10000u64, IBC_DENOM_2.to_string()),
            None,
        ),
        (
            user3,
            12 * LOCK_EPOCH_LENGTH,
            Coin::new(10000u64, IBC_DENOM_3.to_string()),
            None,
        ),
    ];

    execute_locking_and_verify(&mut deps, &env, locking_infos);

    // Verify total voting power is as expected
    let expected_round_powers: Vec<(u64, u128)> = vec![
        (0, 70000),
        (1, 60000),
        (2, 60000),
        (3, 55000),
        (4, 52500),
        (5, 50000),
        (6, 20000),
        (7, 20000),
        (8, 20000),
        (9, 15000),
        (10, 12500),
        (11, 10000),
        (12, 0),
    ];
    for expected_round_power in expected_round_powers {
        let res = get_total_power_for_round(deps.as_ref(), expected_round_power.0);
        assert!(res.is_ok());
        assert_eq!(res.unwrap().to_uint_ceil().u128(), expected_round_power.1);
    }

    // 2.  Round 0: Update config to increase total_cap and set extra_cap starting from round 1.

    // Advance the chain by 1 day
    env.block.time = env.block.time.plus_days(1);
    env.block.height += BLOCKS_PER_DAY;

    let msg = ExecuteMsg::UpdateConfig {
        activate_at: FIRST_ROUND_START.plus_nanos(ROUND_LENGTH + 1),
        max_locked_tokens: Some(40000),
        current_users_extra_cap: Some(2000),
        max_deployment_duration: None,
    };

    let res = execute(deps.as_mut(), env.clone(), admin_msg_info.clone(), msg);
    assert!(res.is_ok(), "error: {:?}", res);

    // 3.  Round 0: Update config to close the extra_cap after some time in round 1.

    // Advance the chain by 1 day
    env.block.time = env.block.time.plus_days(1);
    env.block.height += BLOCKS_PER_DAY;

    let msg = ExecuteMsg::UpdateConfig {
        activate_at: FIRST_ROUND_START.plus_nanos(ROUND_LENGTH + TEN_DAYS_IN_NANOS + 1),
        max_locked_tokens: None,
        current_users_extra_cap: Some(0),
        max_deployment_duration: None,
    };

    let res = execute(deps.as_mut(), env.clone(), admin_msg_info.clone(), msg);
    assert!(res.is_ok(), "error: {:?}", res);

    // 4.  Round 0: Update all validator power ratios to verify that the total voting power changes, and users
    // voting power also gets updated proportinally.

    // Advance the chain by 1 day
    env.block.time = env.block.time.plus_days(1);
    env.block.height += BLOCKS_PER_DAY;

    let mock_shares = Uint128::new(1000) * TOKENS_TO_SHARES_MULTIPLIER;
    let mock_validator1 = get_mock_validator(VALIDATOR_1, Uint128::new(900), mock_shares);
    let mock_validator2 = get_mock_validator(VALIDATOR_2, Uint128::new(900), mock_shares);
    let mock_validator3 = get_mock_validator(VALIDATOR_3, Uint128::new(900), mock_shares);

    let mock_data = HashMap::from([
        (
            1,
            ICQMockData {
                query_type: QueryType::KV,
                should_query_return_error: false,
                should_query_result_return_error: false,
                kv_results: vec![StorageValue {
                    storage_prefix: STAKING_STORE_KEY.to_string(),
                    key: Binary::default(),
                    value: Binary::from(mock_validator1.encode_to_vec()),
                }],
            },
        ),
        (
            2,
            ICQMockData {
                query_type: QueryType::KV,
                should_query_return_error: false,
                should_query_result_return_error: false,
                kv_results: vec![StorageValue {
                    storage_prefix: STAKING_STORE_KEY.to_string(),
                    key: Binary::default(),
                    value: Binary::from(mock_validator2.encode_to_vec()),
                }],
            },
        ),
        (
            3,
            ICQMockData {
                query_type: QueryType::KV,
                should_query_return_error: false,
                should_query_result_return_error: false,
                kv_results: vec![StorageValue {
                    storage_prefix: STAKING_STORE_KEY.to_string(),
                    key: Binary::default(),
                    value: Binary::from(mock_validator3.encode_to_vec()),
                }],
            },
        ),
    ]);

    deps.querier = deps
        .querier
        .with_custom_handler(custom_interchain_query_mock(mock_data));

    for query_id in 1..=3 {
        let res = sudo(
            deps.as_mut(),
            env.clone(),
            SudoMsg::KVQueryResult { query_id },
        );
        assert!(res.is_ok());
    }

    // Verify total voting power is updated as expected
    let expected_round_powers: Vec<(u64, u128)> = vec![
        (0, 63000),
        (1, 54000),
        (2, 54000),
        (3, 49500),
        (4, 47250),
        (5, 45000),
        (6, 18000),
        (7, 18000),
        (8, 18000),
        (9, 13500),
        (10, 11250),
        (11, 9000),
        (12, 0),
    ];

    for expected_round_power in expected_round_powers {
        let res = get_total_power_for_round(deps.as_ref(), expected_round_power.0);
        assert!(res.is_ok());
        assert_eq!(res.unwrap().to_uint_ceil().u128(), expected_round_power.1);
    }

    // Advance the chain into the round 1 plus 1 day, so that user1 can unlock tokens
    env.block.time = FIRST_ROUND_START.plus_nanos(ROUND_LENGTH + 1 + ONE_DAY_IN_NANO_SECONDS);
    env.block.height = INITIAL_BLOCK_HEIGHT + BLOCKS_PER_DAY * 31;

    // 5.  Round 1: Have the first known user unlock the expired lockup.
    let info = get_message_info(&deps.api, user1, &[]);
    let msg = ExecuteMsg::UnlockTokens {};
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok(), "error: {:?}", res);
    assert_eq!(res.unwrap().messages.len(), 1);

    // 6.  Round 1: Have a known user lock some tokens in public_cap, then a completely new user lock tokens
    //     in public cap (try more than allowed, then lock below public_cap).

    // Advance the chain by 1 day
    env.block.time = env.block.time.plus_days(1);
    env.block.height += BLOCKS_PER_DAY;

    let locking_infos = vec![
        // After this action total locked tokens will be 30_000 again
        (
            user1,
            LOCK_EPOCH_LENGTH,
            Coin::new(10000u64, IBC_DENOM_1.to_string()),
            None,
        ),
        // Completely new user tries to lock more than it is available in public_cap
        (
            user4,
            LOCK_EPOCH_LENGTH,
            Coin::new(8001u64, IBC_DENOM_1.to_string()),
            Some("The limit for locking tokens has been reached. No more tokens can be locked."),
        ),
        // Completely new user locks 5_000 tokens in public_cap; the total locked tokens will be 35_000
        (
            user4,
            LOCK_EPOCH_LENGTH,
            Coin::new(5000u64, IBC_DENOM_1.to_string()),
            None,
        ),
    ];

    execute_locking_and_verify(&mut deps, &env, locking_infos);

    verify_locked_tokens_info(
        &deps.storage,
        1,
        35000,
        0,
        vec![(user1_addr.clone(), 0), (user4_addr.clone(), 0)],
    );

    // 7.  Round 1: Have the known user from previous step lock more to fill the public_cap and some more into extra_cap.
    // 8.  Round 1: Have the same user try to lock in extra_cap more than it should be allowed.
    // 9.  Round 1: Have the same user lock the most it should be allowed in the extra_cap.
    // 10. Round 1: Have other two known users lock as much as they should be allowed in the extra_cap.

    // Advance the chain by 1 day
    env.block.time = env.block.time.plus_days(1);
    env.block.height += BLOCKS_PER_DAY;

    let locking_infos = vec![
        // User 1 locks 3100 tokens, 3000 in public_cap, 100 in extra_cap
        // After this action the total locked tokens will be 38_100
        // 38_000 locked in public_cap and 100 locked in extra_cap
        (
            user1,
            LOCK_EPOCH_LENGTH,
            Coin::new(3100u64, IBC_DENOM_1.to_string()),
            None,
        ),
        // User 1 tries to lock in extra_cap more than it should be allowed.
        // By the voting power in previous round it should be allowed to
        // lock 285 tokens, and in previous step it already locked 100.
        (
            user1,
            LOCK_EPOCH_LENGTH,
            Coin::new(186u64, IBC_DENOM_1.to_string()),
            Some("The limit for locking tokens has been reached. No more tokens can be locked."),
        ),
        // User 1 locks in extra_cap the maximum it should be allowed (285)
        // After this action the total locked tokens will be 38_285
        (
            user1,
            LOCK_EPOCH_LENGTH,
            Coin::new(185u64, IBC_DENOM_1.to_string()),
            None,
        ),
        // User 2 locks in extra_cap the maximum it should be allowed (571)
        // After this action the total locked tokens will be 38_856
        (
            user2,
            LOCK_EPOCH_LENGTH,
            Coin::new(571u64, IBC_DENOM_1.to_string()),
            None,
        ),
        // User 3 locks in extra_cap the maximum it should be allowed (1142)
        // After this action the total locked tokens will be 39_998
        (
            user3,
            LOCK_EPOCH_LENGTH,
            Coin::new(1142u64, IBC_DENOM_1.to_string()),
            None,
        ),
    ];

    execute_locking_and_verify(&mut deps, &env, locking_infos);

    verify_locked_tokens_info(
        &deps.storage,
        1,
        39998,
        1998,
        vec![
            (user1_addr.clone(), 285),
            (user2_addr.clone(), 571),
            (user3_addr.clone(), 1142),
        ],
    );

    // 11. Round 1: Update config to increase total_cap and set extra_cap starting from round 2.

    // Advance the chain by 1 day
    env.block.time = env.block.time.plus_days(1);
    env.block.height += BLOCKS_PER_DAY;

    let msg = ExecuteMsg::UpdateConfig {
        activate_at: FIRST_ROUND_START.plus_nanos(2 * ROUND_LENGTH + 1),
        max_locked_tokens: Some(50000),
        current_users_extra_cap: Some(5000),
        max_deployment_duration: None,
    };

    let res = execute(deps.as_mut(), env.clone(), admin_msg_info.clone(), msg);
    assert!(res.is_ok(), "error: {:?}", res);

    // 12. Round 1: Update config to close the extra_cap after some time in round 2.
    let msg = ExecuteMsg::UpdateConfig {
        activate_at: FIRST_ROUND_START.plus_nanos(2 * ROUND_LENGTH + TEN_DAYS_IN_NANOS + 1),
        max_locked_tokens: None,
        current_users_extra_cap: Some(0),
        max_deployment_duration: None,
    };

    let res = execute(deps.as_mut(), env.clone(), admin_msg_info.clone(), msg);
    assert!(res.is_ok(), "error: {:?}", res);

    // Advance the chain into the round 2 plus 1 day
    env.block.time = FIRST_ROUND_START.plus_nanos(2 * ROUND_LENGTH + 1 + ONE_DAY_IN_NANO_SECONDS);
    env.block.height = INITIAL_BLOCK_HEIGHT + BLOCKS_PER_DAY * 61;

    // 13. Round 2: Have a completely new user lock tokens to fill up the public_cap, then try to lock more.
    // 14. Round 2: Have a known user lock maximum allowed in extra cap.

    let locking_infos = vec![
        // Completely new user locks up to the public_cap
        // After this action total locked tokens will be 45_000
        (
            user5,
            LOCK_EPOCH_LENGTH,
            Coin::new(5002u64, IBC_DENOM_1.to_string()),
            None,
        ),
        // Then the same user tries to lock more than allowed in public_cap, while extra_cap is still active
        (
            user5,
            LOCK_EPOCH_LENGTH,
            Coin::new(1u64, IBC_DENOM_1.to_string()),
            Some("The limit for locking tokens has been reached. No more tokens can be locked."),
        ),
        // User 4 had voting power 4_500 out of 71_996 total voting power in round 1
        // With the extra_cap of 5_000, it is allowed to lock 312 tokens in it
        // After this action total locked tokens will be 45_312
        (
            user4,
            LOCK_EPOCH_LENGTH,
            Coin::new(312u64, IBC_DENOM_1.to_string()),
            None,
        ),
    ];

    execute_locking_and_verify(&mut deps, &env, locking_infos);

    // 15. Round 2: Advance the chain to end the extra_cap duration and have a user from step #13 lock
    //     additional amount that matches the entire amount previously reserved for extra_cap.
    env.block.time = FIRST_ROUND_START.plus_nanos(2 * ROUND_LENGTH + TEN_DAYS_IN_NANOS + 1);
    env.block.height = INITIAL_BLOCK_HEIGHT + BLOCKS_PER_DAY * 70;

    let info = get_message_info(
        &deps.api,
        user5,
        &[Coin::new(4688u64, IBC_DENOM_1.to_string())],
    );
    let msg = ExecuteMsg::LockTokens {
        lock_duration: LOCK_EPOCH_LENGTH,
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok());

    verify_locked_tokens_info(
        &deps.storage,
        2,
        50000,
        312,
        vec![
            (user1_addr.clone(), 0),
            (user2_addr.clone(), 0),
            (user3_addr.clone(), 0),
            (user4_addr.clone(), 312),
            (user5_addr.clone(), 0),
        ],
    );
}

fn execute_locking_and_verify(
    deps: &mut OwnedDeps<MockStorage, MockApi, MockQuerier, NeutronQuery>,
    env: &Env,
    locking_infos: Vec<(&str, u64, Coin, Option<&str>)>,
) {
    for locking_info in locking_infos {
        let info = get_message_info(&deps.api, locking_info.0, &[locking_info.2]);
        let msg = ExecuteMsg::LockTokens {
            lock_duration: locking_info.1,
        };
        let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
        match locking_info.3 {
            None => {
                assert!(res.is_ok(), "error: {:?}", res);
            }
            Some(error_message) => {
                assert!(res.unwrap_err().to_string().contains(error_message));
            }
        }
    }
}

fn verify_locked_tokens_info(
    storage: &impl Storage,
    round_id: u64,
    expected_total_locked_tokens: u128,
    expected_extra_locked_tokens_round: u128,
    expected_extra_locked_tokens_round_users: Vec<(Addr, u128)>,
) {
    assert_eq!(
        LOCKED_TOKENS.load(storage).unwrap_or_default(),
        expected_total_locked_tokens
    );
    assert_eq!(
        EXTRA_LOCKED_TOKENS_ROUND_TOTAL
            .load(storage, round_id)
            .unwrap_or_default(),
        expected_extra_locked_tokens_round
    );

    for expected_user_extra_locked in expected_extra_locked_tokens_round_users {
        assert_eq!(
            EXTRA_LOCKED_TOKENS_CURRENT_USERS
                .load(storage, (round_id, expected_user_extra_locked.0))
                .unwrap_or_default(),
            expected_user_extra_locked.1,
        );
    }
}
