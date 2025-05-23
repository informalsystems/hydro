use cosmwasm_std::{testing::mock_env, Timestamp};

use crate::{
    state::{Constants, CONSTANTS},
    testing::{
        get_default_cw721_collection_info, get_default_power_schedule, ONE_DAY_IN_NANO_SECONDS,
    },
    testing_mocks::{mock_dependencies, no_op_grpc_query_mock},
    utils::load_current_constants,
};

#[test]
fn load_current_constants_test() {
    let (mut deps, mut env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());

    struct TestCase {
        activate_at_timestamp: u64,
        constants_to_insert: Constants,
    }

    let constants_template = Constants {
        round_length: ONE_DAY_IN_NANO_SECONDS,
        lock_epoch_length: ONE_DAY_IN_NANO_SECONDS,
        first_round_start: Timestamp::from_seconds(0),
        max_locked_tokens: 0,
        known_users_cap: 0,
        paused: false,
        max_deployment_duration: 12,
        round_lock_power_schedule: get_default_power_schedule(),
        cw721_collection_info: get_default_cw721_collection_info(),
    };

    // Change max_locked_tokens each time we insert new Constants so that we can differentiate them
    let clone_with_locked_tokens =
        |constants_template: &Constants, max_locked_tokens| -> Constants {
            let mut constants = constants_template.clone();
            constants.max_locked_tokens = max_locked_tokens;

            constants
        };

    let test_cases = vec![
        TestCase {
            activate_at_timestamp: 1730840400000000000, // Tuesday, November 05, 2024 09:00:00 PM GMT
            constants_to_insert: clone_with_locked_tokens(&constants_template, 1u128),
        },
        TestCase {
            activate_at_timestamp: 1731924672000000000, // Monday,  November 18, 2024 10:11:12 AM GMT
            constants_to_insert: clone_with_locked_tokens(&constants_template, 2u128),
        },
        TestCase {
            activate_at_timestamp: 1732421792000000000, // Sunday,  November 24, 2024 04:16:32 AM GMT
            constants_to_insert: clone_with_locked_tokens(&constants_template, 3u128),
        },
        TestCase {
            activate_at_timestamp: 1734264033000000000, // Sunday,  December 15, 2024 12:00:33 PM GMT
            constants_to_insert: clone_with_locked_tokens(&constants_template, 4u128),
        },
        TestCase {
            activate_at_timestamp: 1734955199000000000, // Monday,  December 23, 2024 11:59:59 AM GMT
            constants_to_insert: clone_with_locked_tokens(&constants_template, 5u128),
        },
        TestCase {
            activate_at_timestamp: 1735689599000000000, // Tuesday, December 31, 2024 11:59:59 PM GMT
            constants_to_insert: clone_with_locked_tokens(&constants_template, 6u128),
        },
        TestCase {
            activate_at_timestamp: 1736208000000000000, // Tuesday, January  07, 2025 12:00:00 AM GMT
            constants_to_insert: clone_with_locked_tokens(&constants_template, 7u128),
        },
    ];

    // first insert constants for all timestamps
    for test_case in test_cases.iter() {
        let res = CONSTANTS.save(
            &mut deps.storage,
            test_case.activate_at_timestamp,
            &test_case.constants_to_insert,
        );
        assert!(res.is_ok());
    }

    // Verify that we receive expected constants by setting the block time to activate_at_timestamp
    // in first atempt, and then setting it to activate_at_timestamp + 1 hour in second atempt.
    // In both cases we should get the same constants.
    for test_case in test_cases.iter() {
        let timestamps_to_check = vec![
            Timestamp::from_nanos(test_case.activate_at_timestamp),
            Timestamp::from_nanos(test_case.activate_at_timestamp).plus_seconds(3600),
        ];

        for timestamp_to_check in timestamps_to_check {
            env.block.time = timestamp_to_check;

            let res = load_current_constants(&deps.as_ref(), &env);
            assert!(res.is_ok());
            let constants = res.unwrap();

            assert_eq!(constants, test_case.constants_to_insert);
        }
    }
}
