use std::str::FromStr;

use cosmwasm_std::{testing::mock_env, Decimal, Order, Timestamp};
use cw_storage_plus::Map;

use crate::{
    migration::v3_2_0::migrate_v3_2_0_to_v3_3_0,
    state::{Constants, RoundLockPowerSchedule, CONSTANTS},
    testing_mocks::{mock_dependencies, no_op_grpc_query_mock},
};

use super::v3_2_0::ConstantsV3_2_0;

#[test]
fn migrate_test() {
    const OLD_CONSTANTS: Map<u64, ConstantsV3_2_0> = Map::new("constants");

    let (mut deps, mut env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());
    env.block.time = Timestamp::from_nanos(1742482800000000000);

    // Create first constants configuration for timestamp 1
    let first_round_timestamp = 1730851140000000000;
    let first_constants_config = ConstantsV3_2_0 {
        round_length: 2628000000000000,
        lock_epoch_length: 2628000000000000,
        first_round_start: Timestamp::from_nanos(first_round_timestamp),
        max_locked_tokens: 40000000000,
        known_users_cap: 0,
        paused: false,
        max_deployment_duration: 3,
        round_lock_power_schedule: RoundLockPowerSchedule::new(vec![
            (1, Decimal::from_str("1").unwrap()),
            (3, Decimal::from_str("2").unwrap()),
        ]),
    };

    // Create second constants configuration for timestamp 2
    let second_timestamp = 1741190135000000000;
    let second_constants_config = ConstantsV3_2_0 {
        max_locked_tokens: 50000000000,
        ..first_constants_config.clone()
    };

    // Store original constants entries by timestamp
    let original_constants_entries: Vec<(u64, ConstantsV3_2_0)> = vec![
        (first_round_timestamp, first_constants_config),
        (second_timestamp, second_constants_config),
    ];

    // Save original constants to storage
    for (timestamp, constants) in &original_constants_entries {
        OLD_CONSTANTS
            .save(&mut deps.storage, *timestamp, constants)
            .unwrap();
    }

    // Perform migration
    let migration_result = migrate_v3_2_0_to_v3_3_0(&mut deps.as_mut());
    assert!(migration_result.is_ok());

    // Retrieve migrated constants
    let migrated_constants_entries = CONSTANTS
        .range(&deps.storage, None, None, Order::Ascending)
        .filter_map(|result| result.ok())
        .collect::<Vec<(u64, Constants)>>();

    // Verify same number of entries
    assert_eq!(
        original_constants_entries.len(),
        migrated_constants_entries.len()
    );

    // Compare each entry before and after migration
    for (index, (original_timestamp, original_constants)) in
        original_constants_entries.iter().enumerate()
    {
        let (migrated_timestamp, migrated_constants) = &migrated_constants_entries[index];

        // Verify timestamp remains the same
        assert_eq!(*original_timestamp, *migrated_timestamp);

        // Verify all original fields are preserved
        assert_eq!(
            original_constants.round_length,
            migrated_constants.round_length
        );
        assert_eq!(
            original_constants.lock_epoch_length,
            migrated_constants.lock_epoch_length
        );
        assert_eq!(
            original_constants.first_round_start,
            migrated_constants.first_round_start
        );
        assert_eq!(
            original_constants.max_locked_tokens,
            migrated_constants.max_locked_tokens
        );
        assert_eq!(
            original_constants.known_users_cap,
            migrated_constants.known_users_cap
        );
        assert_eq!(original_constants.paused, migrated_constants.paused);
        assert_eq!(
            original_constants.max_deployment_duration,
            migrated_constants.max_deployment_duration
        );
        assert_eq!(
            original_constants.round_lock_power_schedule,
            migrated_constants.round_lock_power_schedule
        );

        // Verify new collection info field is added correctly
        assert_eq!(
            "Hydro Lockups",
            migrated_constants.cw721_collection_info.name
        );
        assert_eq!(
            "hydro-lockups",
            migrated_constants.cw721_collection_info.symbol
        );
    }
}
