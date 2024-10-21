#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::contract::{compute_round_end, execute, instantiate, CONTRACT_NAME};
    use crate::migration::migrate::migrate;
    use crate::msg::{ExecuteMsg, MigrateMsg};
    use crate::state::{CONSTANTS, LOCKS_MAP};
    use crate::testing::{
        get_default_instantiate_msg, get_message_info, set_default_validator_for_rounds,
        IBC_DENOM_1, ONE_DAY_IN_NANO_SECONDS, ONE_MONTH_IN_NANO_SECONDS, VALIDATOR_1_LST_DENOM_1,
    };
    use crate::testing_mocks::{denom_trace_grpc_query_mock, mock_dependencies};
    use cosmwasm_std::testing::mock_env;
    use cosmwasm_std::{Coin, Env, Order, StdResult, Timestamp};
    use cw2::{get_contract_version, set_contract_version};

    #[test]
    fn test_migrate_v100_to_v110() {
        struct TestCase {
            description: String,
            migrate_msg: fn(Env) -> MigrateMsg,
            env_time: fn(Env) -> u64,
            expected_error: Option<&'static str>,
        }

        fn create_migrate_msg(env: &Env, offset: i64) -> MigrateMsg {
            if offset > 0 {
                return MigrateMsg {
                    new_first_round_start: env.block.time.plus_nanos(offset as u64),
                };
            } else if offset < 0 {
                return MigrateMsg {
                    new_first_round_start: env.block.time.minus_nanos(offset.abs() as u64),
                };
            } else {
                return MigrateMsg {
                    new_first_round_start: env.block.time,
                };
            }
        }

        fn compute_env_time(env: &Env, offset: u64) -> u64 {
            env.block.time.plus_nanos(offset).nanos()
        }

        let test_cases = vec![
            TestCase {
                description: "Happy path".to_string(),
                migrate_msg: |env| create_migrate_msg(&env, ONE_DAY_IN_NANO_SECONDS as i64),
                env_time: |env| compute_env_time(&env, 0),
                expected_error: None,
            },
            TestCase {
                description: "Migrate with new first round end in the past".to_string(),
                migrate_msg: |env| create_migrate_msg(&env, -(ONE_MONTH_IN_NANO_SECONDS as i64)),
                env_time: |env| compute_env_time(&env, 0),
                expected_error: Some(
                    "can only be done if the new first round end is in the future",
                ),
            },
            TestCase {
                description: "Migrate not during the first round".to_string(),
                migrate_msg: |env| create_migrate_msg(&env, ONE_DAY_IN_NANO_SECONDS as i64),
                env_time: |env| compute_env_time(&env, ONE_MONTH_IN_NANO_SECONDS * 2),
                expected_error: Some("can only be done within the first round"),
            },
        ];

        for (i, test_case) in test_cases.iter().enumerate() {
            // log the test case description
            println!("Test case {}: {}", i, test_case.description);

            let grpc_query = denom_trace_grpc_query_mock(
                "transfer/channel-0".to_string(),
                HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
            );
            let user_addr = "addr0000";
            let (mut deps, mut env) = (mock_dependencies(grpc_query), mock_env());
            let info = get_message_info(&deps.api, user_addr, &[]);

            // Instantiate the contract
            let instantiate_msg = get_default_instantiate_msg(&deps.api);
            let _res =
                instantiate(deps.as_mut(), env.clone(), info.clone(), instantiate_msg).unwrap();

            set_default_validator_for_rounds(deps.as_mut(), 0, 100);

            // Set contract version to 1.0.0
            set_contract_version(deps.as_mut().storage, CONTRACT_NAME, "1.0.0").unwrap();

            // Create some locks
            // time will be advanced by the given duration after each lock
            let lock_creation_delays = [
                ONE_DAY_IN_NANO_SECONDS * 4,
                ONE_DAY_IN_NANO_SECONDS * 2,
                ONE_DAY_IN_NANO_SECONDS * 2,
            ];

            for (i, &delay) in lock_creation_delays.iter().enumerate() {
                let lock_info = get_message_info(
                    &deps.api,
                    user_addr,
                    &[Coin::new(1000u64, IBC_DENOM_1.to_string())],
                );
                let lock_msg = ExecuteMsg::LockTokens {
                    lock_duration: ONE_MONTH_IN_NANO_SECONDS,
                };
                let res = execute(deps.as_mut(), env.clone(), lock_info.clone(), lock_msg);
                assert!(
                    res.is_ok(),
                    "Lock creation failed for lock {}: {}",
                    i,
                    res.unwrap_err()
                );

                // Advance time after each lock
                env.block.time = env.block.time.plus_nanos(delay);
            }

            env.block.time = Timestamp::from_nanos((test_case.env_time)(env.clone()));
            let migrate_msg = (test_case.migrate_msg)(env.clone());
            let res = migrate(deps.as_mut(), env.clone(), migrate_msg.clone());

            match &test_case.expected_error {
                Some(expected_error) => {
                    assert!(
                        res.is_err(),
                        "Test case {}: Migration should have failed",
                        i
                    );
                    let error_string = res.unwrap_err();
                    assert!(
                        error_string.to_string().contains(expected_error),
                        "Test case {}: Expected error: {:?}, got: {:?}",
                        i,
                        expected_error,
                        error_string
                    );
                }
                None => {
                    assert!(res.is_ok(), "Test case {}: Migration failed: {:?}", i, res);
                    let contract_version = get_contract_version(deps.as_ref().storage).unwrap();
                    assert_eq!(contract_version.version, "1.1.0");

                    let constants = CONSTANTS.load(deps.as_ref().storage).unwrap();
                    let first_round_end = compute_round_end(&constants, 0).unwrap();
                    assert_eq!(
                        constants.first_round_start,
                        migrate_msg.new_first_round_start
                    );

                    let locks = LOCKS_MAP
                        .range(deps.as_ref().storage, None, None, Order::Ascending)
                        .collect::<StdResult<Vec<_>>>()
                        .unwrap();

                    assert_eq!(locks.len(), 3, "Locks count mismatch");

                    for ((addr, lock_id), lock_entry) in locks {
                        assert_eq!(
                            lock_entry.lock_end, first_round_end,
                            "Lock end mismatch for address: {} and lock_id: {}",
                            addr, lock_id
                        );
                    }
                }
            }
        }
    }
}
