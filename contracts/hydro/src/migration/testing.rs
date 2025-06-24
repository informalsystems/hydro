use std::str::FromStr;

use cosmwasm_std::{testing::mock_env, Addr, Decimal, Order, Storage};
use cw_storage_plus::Map;

use crate::{
    migration::{v3_2_0::ConstantsV3_2_0, v3_4_1::migrate_v3_2_0_to_v3_4_1},
    state::{RoundLockPowerSchedule, Vote, USER_LOCKS, USER_LOCKS_FOR_CLAIM, VOTE_MAP_V1},
    testing_mocks::{mock_dependencies, no_op_grpc_query_mock},
};

fn insert_old_constants(storage: &mut dyn Storage) {
    const OLD_CONSTANTS: Map<u64, ConstantsV3_2_0> = Map::new("constants");

    let timestamp = 1730851140000000000;
    let constants = ConstantsV3_2_0 {
        round_length: 100,
        lock_epoch_length: 10,
        first_round_start: mock_env().block.time,
        max_locked_tokens: 1_000_000u128,
        known_users_cap: 10_000u128,
        paused: false,
        max_deployment_duration: 3600,
        round_lock_power_schedule: RoundLockPowerSchedule::new(vec![
            (1, Decimal::from_str("1.0").unwrap()),
            (2, Decimal::from_str("1.25").unwrap()),
            (3, Decimal::from_str("1.5").unwrap()),
        ]),
    };

    OLD_CONSTANTS.save(storage, timestamp, &constants).unwrap();
}

#[test]
fn test_migrate_v3_2_0_to_v3_4_1_comprehensive_scenarios() {
    let (mut deps, env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());
    insert_old_constants(&mut deps.storage);

    let alice = Addr::unchecked("alice");
    let bob = Addr::unchecked("bob");
    let charlie = Addr::unchecked("charlie");

    // Current lock ownership (USER_LOCKS)
    // Alice currently owns: [1, 2]
    // Bob currently owns: [3] (used to own lock 5, voted with it in round 1, then unlocked)
    // Charlie owns nothing currently (used to own lock 7, never voted with it, later unlocked)
    USER_LOCKS
        .save(
            &mut deps.storage,
            alice.clone(),
            &vec![1, 2],
            env.block.height,
        )
        .unwrap();
    USER_LOCKS
        .save(&mut deps.storage, bob.clone(), &vec![3], env.block.height)
        .unwrap();

    let vote = Vote {
        prop_id: 101,
        time_weighted_shares: ("1".to_string(), Decimal::from_ratio(1000u128, 1u128)),
    };

    // Historical voting in VOTE_MAP_V1 (round 1)
    // Alice voted with locks 1, 2 (still owns them)
    VOTE_MAP_V1
        .save(&mut deps.storage, ((1, 1), alice.clone(), 1), &vote)
        .unwrap();
    VOTE_MAP_V1
        .save(&mut deps.storage, ((1, 1), alice.clone(), 2), &vote)
        .unwrap();

    // Bob voted with lock 5 (currently also owns lock 3, but unlocked 5)
    VOTE_MAP_V1
        .save(&mut deps.storage, ((1, 1), bob.clone(), 5), &vote)
        .unwrap();

    // Charlie never voted, and unlocked the lock 7

    // This migration happens right after VOTE_MAP_V2 migration (v3.3.0)
    // No lock has been transferred yet between participants
    // There is no entry in VOTE_MAP_V2

    // Perform migration
    let migration_result = migrate_v3_2_0_to_v3_4_1(&mut deps.as_mut());
    assert!(migration_result.is_ok());

    // Verify USER_LOCKS_FOR_CLAIM results
    let alice_claim_locks = USER_LOCKS_FOR_CLAIM
        .load(&deps.storage, alice.clone())
        .unwrap();

    let bob_claim_locks = USER_LOCKS_FOR_CLAIM
        .load(&deps.storage, bob.clone())
        .unwrap();

    // Charlie should not be able to claim for any lock:
    // - Locks he currently owns: []
    // - Locks he voted with historically: []
    // Total: []
    let charlie_claim_locks = USER_LOCKS_FOR_CLAIM.load(&deps.storage, charlie.clone());
    assert!(charlie_claim_locks.is_err());

    assert!(charlie_claim_locks
        .unwrap_err()
        .to_string()
        .contains("not found"));

    // Convert to sets for easier comparison
    let alice_set: std::collections::HashSet<u64> = alice_claim_locks.into_iter().collect();
    let bob_set: std::collections::HashSet<u64> = bob_claim_locks.into_iter().collect();

    // Alice should be able to claim for:
    // - Locks she currently owns: [1, 2]
    // - Locks she voted with: [1]
    // Total: [1, 2]
    let expected_alice: std::collections::HashSet<u64> = vec![1, 2].into_iter().collect();
    assert_eq!(alice_set, expected_alice);

    // Bob should be able to claim for:
    // - Locks he currently owns: [3]
    // - Locks he voted with: [5]
    // Total: [3, 5] (he cannot claim for lock 5 as he transferred it)
    let expected_bob: std::collections::HashSet<u64> = vec![3, 5].into_iter().collect();
    assert_eq!(bob_set, expected_bob);
}

#[test]
fn test_migrate_v3_2_0_to_v3_4_1_no_duplicates() {
    let (mut deps, env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());
    insert_old_constants(&mut deps.storage);

    let alice = Addr::unchecked("alice");

    // Alice currently owns lock 1 and also voted with it in multiple rounds
    USER_LOCKS
        .save(&mut deps.storage, alice.clone(), &vec![1], env.block.height)
        .unwrap();

    let vote = Vote {
        prop_id: 101,
        time_weighted_shares: ("1".to_string(), Decimal::from_ratio(1000u128, 1u128)),
    };

    // Alice voted with the same lock in multiple rounds
    // Round 1 on tranches 1 and 2
    VOTE_MAP_V1
        .save(&mut deps.storage, ((1, 1), alice.clone(), 1), &vote)
        .unwrap();
    VOTE_MAP_V1
        .save(&mut deps.storage, ((1, 2), alice.clone(), 1), &vote)
        .unwrap();
    // Round 2 on tranche on
    VOTE_MAP_V1
        .save(&mut deps.storage, ((2, 1), alice.clone(), 1), &vote)
        .unwrap();

    // Perform migration
    let migration_result = migrate_v3_2_0_to_v3_4_1(&mut deps.as_mut());
    assert!(migration_result.is_ok());

    // Alice should have lock 1 only once (no duplicates despite multiple votes)
    let alice_claim_locks = USER_LOCKS_FOR_CLAIM
        .load(&deps.storage, alice.clone())
        .unwrap();

    assert_eq!(alice_claim_locks.len(), 1);
    assert!(alice_claim_locks.contains(&1));
}

#[test]
fn test_migrate_v3_2_0_to_v3_4_1_only_current_locks() {
    let (mut deps, env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());
    insert_old_constants(&mut deps.storage);

    let alice = Addr::unchecked("alice");

    // Alice has current locks 1, 2 and 3, but no historical votes
    USER_LOCKS
        .save(
            &mut deps.storage,
            alice.clone(),
            &vec![1, 2, 3],
            env.block.height,
        )
        .unwrap();

    // Perform migration
    let migration_result = migrate_v3_2_0_to_v3_4_1(&mut deps.as_mut());
    assert!(migration_result.is_ok());

    // Alice should be able to claim for her current locks
    let alice_claim_locks = USER_LOCKS_FOR_CLAIM
        .load(&deps.storage, alice.clone())
        .unwrap();

    let expected: std::collections::HashSet<u64> = vec![1, 2, 3].into_iter().collect();
    let actual: std::collections::HashSet<u64> = alice_claim_locks.into_iter().collect();
    assert_eq!(actual, expected);
}

#[test]
fn test_migrate_v3_2_0_to_v3_4_1_only_historical_votes() {
    let (mut deps, _env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());
    insert_old_constants(&mut deps.storage);

    let alice = Addr::unchecked("alice");

    // Alice has no current locks but has historical votes
    let vote = Vote {
        prop_id: 101,
        time_weighted_shares: ("1".to_string(), Decimal::from_ratio(1000u128, 1u128)),
    };

    // Historical votes with locks 1 and 2
    VOTE_MAP_V1
        .save(&mut deps.storage, ((1, 1), alice.clone(), 1), &vote)
        .unwrap();
    VOTE_MAP_V1
        .save(&mut deps.storage, ((1, 1), alice.clone(), 2), &vote)
        .unwrap();

    // Perform migration
    let migration_result = migrate_v3_2_0_to_v3_4_1(&mut deps.as_mut());
    assert!(migration_result.is_ok());

    // Alice should be able to claim for her historical voting locks
    let alice_claim_locks = USER_LOCKS_FOR_CLAIM
        .load(&deps.storage, alice.clone())
        .unwrap();

    let expected: std::collections::HashSet<u64> = vec![1, 2].into_iter().collect();
    let actual: std::collections::HashSet<u64> = alice_claim_locks.into_iter().collect();
    assert_eq!(actual, expected);
}

#[test]
fn test_migrate_v3_2_0_to_v3_4_1_empty_state() {
    let (mut deps, _env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());
    insert_old_constants(&mut deps.storage);

    // No existing data - migration should succeed without error
    let migration_result = migrate_v3_2_0_to_v3_4_1(&mut deps.as_mut());
    assert!(migration_result.is_ok());

    // Verify no entries in USER_LOCKS_FOR_CLAIM
    let entries: Vec<_> = USER_LOCKS_FOR_CLAIM
        .range(&deps.storage, None, None, Order::Ascending)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert!(entries.is_empty());
}
