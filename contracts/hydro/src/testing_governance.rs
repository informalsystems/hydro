use std::collections::HashMap;

use cosmwasm_std::{
    testing::{mock_env, MockApi, MockStorage},
    Coin, Env, OwnedDeps, Timestamp, Uint128,
};
use neutron_sdk::bindings::query::NeutronQuery;

use crate::{
    contract::{execute, instantiate},
    governance::{query_total_power_at_height, query_voting_power_at_height},
    msg::ExecuteMsg,
    testing::{
        get_default_instantiate_msg, get_message_info, IBC_DENOM_1, IBC_DENOM_2,
        ONE_DAY_IN_NANO_SECONDS, VALIDATOR_1, VALIDATOR_1_LST_DENOM_1, VALIDATOR_2,
        VALIDATOR_2_LST_DENOM_1,
    },
    testing_lsm_integration::set_validator_infos_for_round,
    testing_mocks::{denom_trace_grpc_query_mock, mock_dependencies, MockQuerier},
};

const ROUND_LENGTH: u64 = 30 * ONE_DAY_IN_NANO_SECONDS;
const LOCK_EPOCH_LENGTH: u64 = ROUND_LENGTH;
const FIRST_ROUND_START: Timestamp = Timestamp::from_nanos(1737540000000000000); // Wednesday, January 22, 2025 10:00:00 AM
const INITIAL_BLOCK_HEIGHT: u64 = 19_185_000;
const BLOCKS_PER_DAY: u64 = 35_000;

#[test]
fn test_voting_power_queries() {
    let user1 = "addr0001";
    let user2 = "addr0002";

    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([
            (IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string()),
            (IBC_DENOM_2.to_string(), VALIDATOR_2_LST_DENOM_1.to_string()),
        ]),
    );
    let (mut deps, mut env) = (mock_dependencies(grpc_query), mock_env());

    env.block.time = FIRST_ROUND_START;
    env.block.height = INITIAL_BLOCK_HEIGHT;

    let user1_addr = deps.api.addr_make(user1);
    let user2_addr = deps.api.addr_make(user2);

    let mut msg = get_default_instantiate_msg(&deps.api);

    msg.lock_epoch_length = LOCK_EPOCH_LENGTH;
    msg.round_length = ROUND_LENGTH;
    msg.first_round_start = env.block.time;

    let init_msg_info = get_message_info(&deps.api, user1, &[]);
    let res = instantiate(
        deps.as_mut(),
        env.clone(),
        init_msg_info.clone(),
        msg.clone(),
    );
    assert!(res.is_ok());

    // Set all validators power ratio in round 0 to 1
    let res = set_validator_infos_for_round(
        &mut deps.storage,
        0,
        vec![VALIDATOR_1.to_string(), VALIDATOR_2.to_string()],
    );
    assert!(res.is_ok());

    let mut expected_voting_powers: HashMap<u64, VotingPowersAtHeight> = HashMap::new();

    // 1 day after round 0 start, user1 locks 100 tokens for 12 rounds
    let days_num = 1;
    advance_chain_and_lock_tokens(
        &mut deps,
        &mut env,
        days_num,
        user1,
        Coin::new(100u128, IBC_DENOM_1),
        12 * LOCK_EPOCH_LENGTH,
    );

    expected_voting_powers.insert(
        env.block.height + 1,
        VotingPowersAtHeight {
            total_voting_power: Uint128::new(400),
            user_voting_powers: vec![
                (user1_addr.to_string(), Uint128::new(400)),
                (user2_addr.to_string(), Uint128::new(0)),
            ],
        },
    );

    // 10 days after round 0 start, user1 locks 200 tokens for 12 rounds
    let days_num = 9;
    advance_chain_and_lock_tokens(
        &mut deps,
        &mut env,
        days_num,
        user1,
        Coin::new(200u128, IBC_DENOM_1),
        12 * LOCK_EPOCH_LENGTH,
    );

    expected_voting_powers.insert(
        env.block.height + 1,
        VotingPowersAtHeight {
            total_voting_power: Uint128::new(1200),
            user_voting_powers: vec![
                (user1_addr.to_string(), Uint128::new(1200)),
                (user2_addr.to_string(), Uint128::new(0)),
            ],
        },
    );

    // Advance the chain by one day; we are still in round 0
    env.block.time = env.block.time.plus_days(1);
    env.block.height += BLOCKS_PER_DAY;

    // Verify voting powers are as expected for the given heights, as well as when no height is provided
    let expected_current_total_power = Uint128::new(1200);
    let expected_current_user_powers = vec![
        (user1_addr.to_string(), Uint128::new(1200)),
        (user2_addr.to_string(), Uint128::new(0)),
    ];

    verify_voting_powers(
        &mut deps,
        &env,
        &expected_voting_powers,
        expected_current_total_power,
        expected_current_user_powers,
    );

    // Advance the chain into round 1
    env.block.time = FIRST_ROUND_START.plus_nanos(ROUND_LENGTH + 1);
    env.block.height = INITIAL_BLOCK_HEIGHT + 30 * BLOCKS_PER_DAY;

    // Add a height that matches the beginning of round 1 to the list of heights to check
    expected_voting_powers.insert(
        env.block.height,
        VotingPowersAtHeight {
            total_voting_power: Uint128::new(1200),
            user_voting_powers: vec![
                (user1_addr.to_string(), Uint128::new(1200)),
                (user2_addr.to_string(), Uint128::new(0)),
            ],
        },
    );

    // 1 day after round 1 start, user2 locks 300 tokens for 6 rounds
    let days_num = 1;
    advance_chain_and_lock_tokens(
        &mut deps,
        &mut env,
        days_num,
        user2,
        Coin::new(300u128, IBC_DENOM_2),
        6 * LOCK_EPOCH_LENGTH,
    );

    expected_voting_powers.insert(
        env.block.height + 1,
        VotingPowersAtHeight {
            total_voting_power: Uint128::new(1800),
            user_voting_powers: vec![
                (user1_addr.to_string(), Uint128::new(1200)),
                (user2_addr.to_string(), Uint128::new(600)),
            ],
        },
    );

    // 15 days after round 1 start, user2 locks 500 tokens for 6 rounds
    let days_num = 14;
    advance_chain_and_lock_tokens(
        &mut deps,
        &mut env,
        days_num,
        user2,
        Coin::new(500u128, IBC_DENOM_2),
        6 * LOCK_EPOCH_LENGTH,
    );

    expected_voting_powers.insert(
        env.block.height + 1,
        VotingPowersAtHeight {
            total_voting_power: Uint128::new(2800),
            user_voting_powers: vec![
                (user1_addr.to_string(), Uint128::new(1200)),
                (user2_addr.to_string(), Uint128::new(1600)),
            ],
        },
    );

    // 16 days after round 1 start, user1 locks 700 tokens for 1 round
    let days_num = 1;
    advance_chain_and_lock_tokens(
        &mut deps,
        &mut env,
        days_num,
        user1,
        Coin::new(700u128, IBC_DENOM_1),
        LOCK_EPOCH_LENGTH,
    );

    expected_voting_powers.insert(
        env.block.height + 1,
        VotingPowersAtHeight {
            total_voting_power: Uint128::new(3500),
            user_voting_powers: vec![
                (user1_addr.to_string(), Uint128::new(1900)),
                (user2_addr.to_string(), Uint128::new(1600)),
            ],
        },
    );

    // Verify voting powers are as expected for the given heights, as well as when no height is provided
    let expected_current_total_power = Uint128::new(3500);
    let expected_current_user_powers = vec![
        (user1_addr.to_string(), Uint128::new(1900)),
        (user2_addr.to_string(), Uint128::new(1600)),
    ];

    verify_voting_powers(
        &mut deps,
        &env,
        &expected_voting_powers,
        expected_current_total_power,
        expected_current_user_powers,
    );

    // Verify that error is returned if we try to query for voting power before historical data was available
    let expected_err = format!(
        "Historical data not available before height: {}. Height requested: {}",
        INITIAL_BLOCK_HEIGHT,
        INITIAL_BLOCK_HEIGHT - 1
    );

    let total_power_err =
        query_total_power_at_height(&deps.as_ref(), &env, Some(INITIAL_BLOCK_HEIGHT - 1))
            .unwrap_err();
    assert!(total_power_err.to_string().contains(expected_err.as_str()));

    let user_power_err = query_voting_power_at_height(
        &deps.as_ref(),
        &env,
        user1_addr.to_string(),
        Some(INITIAL_BLOCK_HEIGHT - 1),
    )
    .unwrap_err();
    assert!(user_power_err.to_string().contains(expected_err.as_str()));
}

fn advance_chain_and_lock_tokens(
    deps: &mut OwnedDeps<MockStorage, MockApi, MockQuerier, NeutronQuery>,
    env: &mut Env,
    days_num: u64,
    user: &str,
    to_lock: Coin,
    lock_duration: u64,
) {
    env.block.time = env.block.time.plus_days(days_num);
    env.block.height += days_num * BLOCKS_PER_DAY;

    let msg_info = get_message_info(&deps.api, user, &[to_lock]);
    let msg = ExecuteMsg::LockTokens {
        lock_duration,
        proof: None,
    };
    let res = execute(deps.as_mut(), env.clone(), msg_info.clone(), msg);
    assert!(res.is_ok(), "error: {:?}", res);
}

fn verify_voting_powers(
    deps: &mut OwnedDeps<MockStorage, MockApi, MockQuerier, NeutronQuery>,
    env: &Env,
    expected_powers_at_heights: &HashMap<u64, VotingPowersAtHeight>,
    expected_current_total_power: Uint128,
    expected_current_user_powers: Vec<(String, Uint128)>,
) {
    for expected_powers in expected_powers_at_heights {
        let total_power =
            query_total_power_at_height(&deps.as_ref(), env, Some(*expected_powers.0)).unwrap();
        assert_eq!(total_power.height, *expected_powers.0);
        assert_eq!(total_power.power, expected_powers.1.total_voting_power);

        for expected_user_power in &expected_powers.1.user_voting_powers {
            let user_power = query_voting_power_at_height(
                &deps.as_ref(),
                env,
                expected_user_power.0.clone(),
                Some(*expected_powers.0),
            )
            .unwrap();

            assert_eq!(user_power.height, *expected_powers.0);
            assert_eq!(user_power.power, expected_user_power.1);
        }
    }

    // Verify that if no height is specified, query returns current total voting power
    let total_power = query_total_power_at_height(&deps.as_ref(), env, None).unwrap();
    assert_eq!(total_power.height, env.block.height);
    assert_eq!(total_power.power, expected_current_total_power);

    // Verify that if no height is specified, query returns current user voting power
    for expected_current_user_power in expected_current_user_powers {
        let user_power =
            query_voting_power_at_height(&deps.as_ref(), env, expected_current_user_power.0, None)
                .unwrap();
        assert_eq!(user_power.height, env.block.height);
        assert_eq!(user_power.power, expected_current_user_power.1);
    }
}

struct VotingPowersAtHeight {
    pub total_voting_power: Uint128,
    pub user_voting_powers: Vec<(String, Uint128)>,
}
