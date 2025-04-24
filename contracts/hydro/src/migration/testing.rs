use std::str::FromStr;

use cosmwasm_std::{testing::mock_env, Addr, Coin, Decimal, Order, Timestamp, Uint128};
use cw_storage_plus::Map;

use crate::{
    contract::query_token_info_providers,
    migration::unreleased::{
        is_full_migration_done, migrate_locks_batch, migrate_v3_1_1_to_unreleased,
        migrate_votes_batch,
    },
    state::{
        Constants, LockEntryV1, RoundLockPowerSchedule, Vote, CONSTANTS, LOCKS_MAP_V1,
        LOCKS_MAP_V2, VOTE_MAP_V1, VOTE_MAP_V2,
    },
    testing_mocks::{mock_dependencies, no_op_grpc_query_mock},
    token_manager::TokenInfoProvider,
};

use super::v3_1_1::ConstantsV3_1_1;

#[test]
fn migrate_test() {
    const OLD_CONSTANTS: Map<u64, ConstantsV3_1_1> = Map::new("constants");

    let (mut deps, mut env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());
    env.block.time = Timestamp::from_nanos(1742482800000000000);

    let first_constants = ConstantsV3_1_1 {
        round_length: 2628000000000000,
        lock_epoch_length: 2628000000000000,
        first_round_start: Timestamp::from_nanos(1730851140000000000),
        max_locked_tokens: 40000000000,
        known_users_cap: 0,
        max_validator_shares_participating: 500,
        hub_connection_id: "connection-0".to_string(),
        hub_transfer_channel_id: "channel-1".to_string(),
        icq_update_period: 109000,
        paused: false,
        max_deployment_duration: 3,
        round_lock_power_schedule: RoundLockPowerSchedule::new(vec![
            (1, Decimal::from_str("1").unwrap()),
            (3, Decimal::from_str("2").unwrap()),
        ]),
    };

    let mut second_constants = first_constants.clone();
    second_constants.max_locked_tokens = 50000000000;

    let old_constants_vec: Vec<(u64, ConstantsV3_1_1)> = vec![
        (1730851140000000000, first_constants.clone()),
        (1741190135000000000, second_constants.clone()),
    ];

    for old_constants in &old_constants_vec {
        OLD_CONSTANTS
            .save(&mut deps.storage, old_constants.0, &old_constants.1)
            .unwrap();
    }

    let res = migrate_v3_1_1_to_unreleased(&mut deps.as_mut());
    assert!(res.is_ok());

    let new_constants = CONSTANTS
        .range(&deps.storage, None, None, Order::Ascending)
        .filter_map(|c| match c {
            Err(_) => None,
            Ok(c) => Some(c),
        })
        .collect::<Vec<(u64, Constants)>>();
    assert_eq!(old_constants_vec.len(), new_constants.len());

    for (i, old_constants) in old_constants_vec.iter().enumerate() {
        assert_eq!(old_constants.0, new_constants[i].0);

        assert_eq!(
            (old_constants.1).round_length,
            (new_constants[i].1).round_length
        );
        assert_eq!(
            (old_constants.1).lock_epoch_length,
            (new_constants[i].1).lock_epoch_length
        );
        assert_eq!(
            (old_constants.1).first_round_start,
            (new_constants[i].1).first_round_start
        );
        assert_eq!(
            (old_constants.1).max_locked_tokens,
            (new_constants[i].1).max_locked_tokens
        );
        assert_eq!(
            (old_constants.1).known_users_cap,
            (new_constants[i].1).known_users_cap
        );
        assert_eq!((old_constants.1).paused, (new_constants[i].1).paused);
        assert_eq!(
            (old_constants.1).max_deployment_duration,
            (new_constants[i].1).max_deployment_duration
        );
        assert_eq!(
            (old_constants.1).round_lock_power_schedule,
            (new_constants[i].1).round_lock_power_schedule
        );
    }

    let token_info_providers = query_token_info_providers(deps.as_ref()).unwrap().providers;
    assert_eq!(token_info_providers.len(), 1);

    match token_info_providers[0].clone() {
        TokenInfoProvider::Derivative(_) => panic!("Derivative token info provider not expected."),
        TokenInfoProvider::LSM(lsm_provider) => {
            assert_eq!(
                lsm_provider.hub_connection_id,
                second_constants.hub_connection_id
            );
            assert_eq!(
                lsm_provider.hub_transfer_channel_id,
                second_constants.hub_transfer_channel_id
            );
            assert_eq!(
                lsm_provider.max_validator_shares_participating,
                second_constants.max_validator_shares_participating
            );
            assert_eq!(
                lsm_provider.icq_update_period,
                second_constants.icq_update_period
            );
        }
    };
}

#[test]
fn migrate_locks_batch_test() {
    let mut deps = mock_dependencies(no_op_grpc_query_mock());

    // Create test data in V1 format
    let addr1 = Addr::unchecked("addr1");
    let addr2 = Addr::unchecked("addr2");

    // Create locks
    let lock1 = LockEntryV1 {
        lock_id: 1,
        funds: Coin {
            denom: "atom".to_string(),
            amount: Uint128::new(100),
        },
        lock_start: Timestamp::from_seconds(1000),
        lock_end: Timestamp::from_seconds(2000),
    };

    let lock2 = LockEntryV1 {
        lock_id: 2,
        funds: Coin {
            denom: "atom".to_string(),
            amount: Uint128::new(200),
        },
        lock_start: Timestamp::from_seconds(1200),
        lock_end: Timestamp::from_seconds(2200),
    };

    // Store locks in V1 format with specific block heights
    LOCKS_MAP_V1
        .save(&mut deps.storage, (addr1.clone(), 1), &lock1, 100)
        .unwrap();
    LOCKS_MAP_V1
        .save(&mut deps.storage, (addr2.clone(), 2), &lock2, 200)
        .unwrap();

    // Create historical entries for lock1
    let lock1_updated = LockEntryV1 {
        lock_id: 1,
        funds: Coin {
            denom: "atom".to_string(),
            amount: Uint128::new(100),
        },
        lock_start: Timestamp::from_seconds(1000),
        lock_end: Timestamp::from_seconds(3000), // Extended lock
    };
    LOCKS_MAP_V1
        .save(&mut deps.storage, (addr1.clone(), 1), &lock1_updated, 150)
        .unwrap();

    // Run migration at height 300
    let result = migrate_locks_batch(&mut deps.as_mut(), 300, 0, 2).unwrap();

    // Verify migration attributes
    assert_eq!(result.attributes[0].value, "migrate_locks_batch");
    assert_eq!(result.attributes[1].value, "0"); // 0 starts
    assert_eq!(result.attributes[2].value, "2"); // 2 limit
    assert_eq!(result.attributes[3].value, "2"); // 2 locks migrated

    // Check migrated locks (most recent entries)
    let migrated_lock1 = LOCKS_MAP_V2.load(&deps.storage, 1).unwrap();
    let migrated_lock2 = LOCKS_MAP_V2.load(&deps.storage, 2).unwrap();

    assert_eq!(migrated_lock1.lock_id, 1);
    assert_eq!(migrated_lock1.owner, addr1);
    assert_eq!(migrated_lock1.funds.amount, Uint128::new(100));
    assert_eq!(migrated_lock1.lock_end, Timestamp::from_seconds(3000)); // The updated value

    assert_eq!(migrated_lock2.lock_id, 2);
    assert_eq!(migrated_lock2.owner, addr2);
    assert_eq!(migrated_lock2.funds.amount, Uint128::new(200));
}

#[test]
fn migrate_1_of_2_locks_batch_test() {
    let mut deps = mock_dependencies(no_op_grpc_query_mock());

    // Create test data in V1 format
    let addr1 = Addr::unchecked("addr1");
    let addr2 = Addr::unchecked("addr2");

    // Create locks
    let lock1 = LockEntryV1 {
        lock_id: 1,
        funds: Coin {
            denom: "atom".to_string(),
            amount: Uint128::new(100),
        },
        lock_start: Timestamp::from_seconds(1000),
        lock_end: Timestamp::from_seconds(2000),
    };

    let lock2 = LockEntryV1 {
        lock_id: 2,
        funds: Coin {
            denom: "atom".to_string(),
            amount: Uint128::new(200),
        },
        lock_start: Timestamp::from_seconds(1200),
        lock_end: Timestamp::from_seconds(2200),
    };

    // Store locks in V1 format with specific block heights
    LOCKS_MAP_V1
        .save(&mut deps.storage, (addr1.clone(), 1), &lock1, 100)
        .unwrap();
    LOCKS_MAP_V1
        .save(&mut deps.storage, (addr2.clone(), 2), &lock2, 200)
        .unwrap();

    // Create historical entries for lock1
    let lock1_updated = LockEntryV1 {
        lock_id: 1,
        funds: Coin {
            denom: "atom".to_string(),
            amount: Uint128::new(100),
        },
        lock_start: Timestamp::from_seconds(1000),
        lock_end: Timestamp::from_seconds(3000), // Extended lock
    };
    LOCKS_MAP_V1
        .save(&mut deps.storage, (addr1.clone(), 1), &lock1_updated, 150)
        .unwrap();

    // Run migration at height 300
    let result = migrate_locks_batch(&mut deps.as_mut(), 300, 0, 1).unwrap();

    // Verify migration attributes
    assert_eq!(result.attributes[0].value, "migrate_locks_batch");
    assert_eq!(result.attributes[1].value, "0"); // 0 starts
    assert_eq!(result.attributes[2].value, "1"); // 2 limit
    assert_eq!(result.attributes[3].value, "1"); // 2 locks migrated

    // Check migrated locks (most recent entries)
    let migrated_lock1 = LOCKS_MAP_V2.load(&deps.storage, 1).unwrap();
    let migrated_lock2 = LOCKS_MAP_V2.load(&deps.storage, 2);

    assert!(migrated_lock2.is_err()); // Lock 2 should not be migrated

    assert_eq!(migrated_lock1.lock_id, 1);
    assert_eq!(migrated_lock1.owner, addr1);
    assert_eq!(migrated_lock1.funds.amount, Uint128::new(100));
    assert_eq!(migrated_lock1.lock_end, Timestamp::from_seconds(3000)); // The updated value
}

#[test]
fn migrate_only_second_lock_batch_test() {
    let mut deps = mock_dependencies(no_op_grpc_query_mock());

    // Create test data in V1 format
    let addr1 = Addr::unchecked("addr1");
    let addr2 = Addr::unchecked("addr2");

    // Create locks
    let lock1 = LockEntryV1 {
        lock_id: 1,
        funds: Coin {
            denom: "atom".to_string(),
            amount: Uint128::new(100),
        },
        lock_start: Timestamp::from_seconds(1000),
        lock_end: Timestamp::from_seconds(2000),
    };

    let lock2 = LockEntryV1 {
        lock_id: 2,
        funds: Coin {
            denom: "atom".to_string(),
            amount: Uint128::new(200),
        },
        lock_start: Timestamp::from_seconds(1200),
        lock_end: Timestamp::from_seconds(2200),
    };

    // Store locks in V1 format with specific block heights
    LOCKS_MAP_V1
        .save(&mut deps.storage, (addr1.clone(), 1), &lock1, 100)
        .unwrap();
    LOCKS_MAP_V1
        .save(&mut deps.storage, (addr2.clone(), 2), &lock2, 200)
        .unwrap();

    // Create historical entries for lock1
    let lock1_updated = LockEntryV1 {
        lock_id: 1,
        funds: Coin {
            denom: "atom".to_string(),
            amount: Uint128::new(100),
        },
        lock_start: Timestamp::from_seconds(1000),
        lock_end: Timestamp::from_seconds(3000), // Extended lock
    };
    LOCKS_MAP_V1
        .save(&mut deps.storage, (addr1.clone(), 1), &lock1_updated, 150)
        .unwrap();

    // Run migration at height 300
    let result = migrate_locks_batch(&mut deps.as_mut(), 300, 1, 1).unwrap();

    // Verify migration attributes
    assert_eq!(result.attributes[0].value, "migrate_locks_batch");
    assert_eq!(result.attributes[1].value, "1"); // 0 starts
    assert_eq!(result.attributes[2].value, "1"); // 2 limit
    assert_eq!(result.attributes[3].value, "1"); // 2 locks migrated

    // Check migrated locks (most recent entries)
    let migrated_lock1 = LOCKS_MAP_V2.load(&deps.storage, 1);
    let migrated_lock2 = LOCKS_MAP_V2.load(&deps.storage, 2).unwrap();

    assert!(migrated_lock1.is_err()); // Lock 2 should not be migrated

    assert_eq!(migrated_lock2.lock_id, 2);
    assert_eq!(migrated_lock2.owner, addr2);
    assert_eq!(migrated_lock2.funds.amount, Uint128::new(200));
}

#[test]
fn migrate_votes_batch_test() {
    let mut deps = mock_dependencies(no_op_grpc_query_mock());

    // Create test data in V1 format
    let addr1 = Addr::unchecked("addr1");
    let addr2 = Addr::unchecked("addr2");

    // Create votes
    let vote1 = Vote {
        prop_id: 10,
        time_weighted_shares: ("atom".to_string(), Decimal::from_str("1.5").unwrap()),
    };

    let vote2 = Vote {
        prop_id: 20,
        time_weighted_shares: ("atom".to_string(), Decimal::from_str("2.5").unwrap()),
    };

    // Store votes in V1 format
    VOTE_MAP_V1
        .save(&mut deps.storage, ((1, 1), addr1.clone(), 1), &vote1)
        .unwrap();
    VOTE_MAP_V1
        .save(&mut deps.storage, ((1, 1), addr2.clone(), 2), &vote2)
        .unwrap();

    // Run migration at height 300
    let result = migrate_votes_batch(&mut deps.as_mut(), 0, 2).unwrap();

    // Verify migration attributes
    assert_eq!(result.attributes[0].value, "migrate_votes_batch");
    assert_eq!(result.attributes[1].value, "0"); // 0 starts
    assert_eq!(result.attributes[2].value, "2"); // 2 limit
    assert_eq!(result.attributes[3].value, "2"); // 2 votes migrated

    // Check migrated votes
    let migrated_vote1 = VOTE_MAP_V2.load(&deps.storage, ((1, 1), 1)).unwrap();
    let migrated_vote2 = VOTE_MAP_V2.load(&deps.storage, ((1, 1), 2)).unwrap();

    assert_eq!(migrated_vote1.prop_id, 10);
    assert_eq!(migrated_vote2.prop_id, 20);
}

#[test]
fn migrate_1_of_2_votes_batch_test() {
    let mut deps = mock_dependencies(no_op_grpc_query_mock());

    // Create test data in V1 format
    let addr1 = Addr::unchecked("addr1");
    let addr2 = Addr::unchecked("addr2");

    // Create votes
    let vote1 = Vote {
        prop_id: 10,
        time_weighted_shares: ("atom".to_string(), Decimal::from_str("1.5").unwrap()),
    };

    let vote2 = Vote {
        prop_id: 20,
        time_weighted_shares: ("atom".to_string(), Decimal::from_str("2.5").unwrap()),
    };

    // Store votes in V1 format
    VOTE_MAP_V1
        .save(&mut deps.storage, ((1, 1), addr1.clone(), 1), &vote1)
        .unwrap();
    VOTE_MAP_V1
        .save(&mut deps.storage, ((1, 1), addr2.clone(), 2), &vote2)
        .unwrap();

    // Run migration at height 300
    let result = migrate_votes_batch(&mut deps.as_mut(), 0, 1).unwrap();

    // Verify migration attributes
    assert_eq!(result.attributes[0].value, "migrate_votes_batch");
    assert_eq!(result.attributes[1].value, "0"); // 0 starts
    assert_eq!(result.attributes[2].value, "1"); // 2 limit
    assert_eq!(result.attributes[3].value, "1"); // 2 votes migrated

    // Check migrated votes
    let migrated_vote1 = VOTE_MAP_V2.load(&deps.storage, ((1, 1), 1)).unwrap();
    let migrated_vote2 = VOTE_MAP_V2.load(&deps.storage, ((1, 1), 2));
    assert!(migrated_vote2.is_err()); // Vote 2 should not be migrated

    assert_eq!(migrated_vote1.prop_id, 10);
}

#[test]
fn migrate_only_second_votes_batch_test() {
    let mut deps = mock_dependencies(no_op_grpc_query_mock());

    // Create test data in V1 format
    let addr1 = Addr::unchecked("addr1");
    let addr2 = Addr::unchecked("addr2");

    // Create votes
    let vote1 = Vote {
        prop_id: 10,
        time_weighted_shares: ("atom".to_string(), Decimal::from_str("1.5").unwrap()),
    };

    let vote2 = Vote {
        prop_id: 20,
        time_weighted_shares: ("atom".to_string(), Decimal::from_str("2.5").unwrap()),
    };

    // Store votes in V1 format
    VOTE_MAP_V1
        .save(&mut deps.storage, ((1, 1), addr1.clone(), 1), &vote1)
        .unwrap();
    VOTE_MAP_V1
        .save(&mut deps.storage, ((1, 1), addr2.clone(), 2), &vote2)
        .unwrap();

    // Run migration at height 300
    let result = migrate_votes_batch(&mut deps.as_mut(), 1, 1).unwrap();

    // Verify migration attributes
    assert_eq!(result.attributes[0].value, "migrate_votes_batch");
    assert_eq!(result.attributes[1].value, "1"); // 0 starts
    assert_eq!(result.attributes[2].value, "1"); // 2 limit
    assert_eq!(result.attributes[3].value, "1"); // 2 votes migrated

    // Check migrated votes
    let migrated_vote1 = VOTE_MAP_V2.load(&deps.storage, ((1, 1), 1));
    let migrated_vote2 = VOTE_MAP_V2.load(&deps.storage, ((1, 1), 2)).unwrap();
    assert!(migrated_vote1.is_err()); // Vote 1 should not be migrated

    assert_eq!(migrated_vote2.prop_id, 20);
}

#[test]
fn is_full_migration_done_test() {
    let mut deps = mock_dependencies(no_op_grpc_query_mock());

    // Create test data in V1 format
    let addr1 = Addr::unchecked("addr1");
    let addr2 = Addr::unchecked("addr2");

    // Create locks
    let lock1 = LockEntryV1 {
        lock_id: 1,
        funds: Coin {
            denom: "atom".to_string(),
            amount: Uint128::new(100),
        },
        lock_start: Timestamp::from_seconds(1000),
        lock_end: Timestamp::from_seconds(2000),
    };

    let lock2 = LockEntryV1 {
        lock_id: 2,
        funds: Coin {
            denom: "atom".to_string(),
            amount: Uint128::new(200),
        },
        lock_start: Timestamp::from_seconds(1200),
        lock_end: Timestamp::from_seconds(2200),
    };

    // Store locks in V1 format with specific block heights
    LOCKS_MAP_V1
        .save(&mut deps.storage, (addr1.clone(), 1), &lock1, 100)
        .unwrap();
    LOCKS_MAP_V1
        .save(&mut deps.storage, (addr2.clone(), 2), &lock2, 200)
        .unwrap();

    // Create votes
    let vote1 = Vote {
        prop_id: 10,
        time_weighted_shares: ("atom".to_string(), Decimal::from_str("1.5").unwrap()),
    };

    let vote2 = Vote {
        prop_id: 20,
        time_weighted_shares: ("atom".to_string(), Decimal::from_str("2.5").unwrap()),
    };

    // Store votes in V1 format
    VOTE_MAP_V1
        .save(&mut deps.storage, ((1, 1), addr1.clone(), 1), &vote1)
        .unwrap();
    VOTE_MAP_V1
        .save(&mut deps.storage, ((1, 1), addr2.clone(), 2), &vote2)
        .unwrap();

    assert!(
        !is_full_migration_done(deps.as_ref()).unwrap(),
        "migration is not complete"
    );

    migrate_locks_batch(&mut deps.as_mut(), 300, 0, 2).unwrap();
    migrate_votes_batch(&mut deps.as_mut(), 0, 2).unwrap();

    assert!(
        is_full_migration_done(deps.as_ref()).unwrap(),
        "migration is complete"
    );
}
