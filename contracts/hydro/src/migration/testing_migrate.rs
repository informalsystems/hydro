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
    use cosmwasm_std::{Coin, Order, StdResult};
    use cw2::set_contract_version;

    #[test]
    fn test_migrate() {
        let grpc_query = denom_trace_grpc_query_mock(
            "transfer/channel-0".to_string(),
            HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
        );
        let user_addr = "addr0000";
        let (mut deps, mut env) = (mock_dependencies(grpc_query), mock_env());
        let info = get_message_info(&deps.api, user_addr, &[]);

        // Instantiate the contract
        let instantiate_msg = get_default_instantiate_msg(&deps.api);
        let _res = instantiate(deps.as_mut(), env.clone(), info.clone(), instantiate_msg).unwrap();

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

        // Prepare the migrate message
        let migrate_msg = MigrateMsg {
            new_first_round_start: env.block.time.plus_nanos(ONE_DAY_IN_NANO_SECONDS),
        };

        // Call the migrate function
        let res = migrate(deps.as_mut(), env.clone(), migrate_msg.clone());
        assert!(res.is_ok(), "Migration failed: {:?}", res);

        // Check that the first round end is as expected
        let constants = CONSTANTS.load(deps.as_ref().storage).unwrap();
        let first_round_end = compute_round_end(&constants, 0).unwrap();
        assert_eq!(
            constants.first_round_start,
            migrate_msg.new_first_round_start
        );

        // Check that all existing locks end at the end of the first round
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
