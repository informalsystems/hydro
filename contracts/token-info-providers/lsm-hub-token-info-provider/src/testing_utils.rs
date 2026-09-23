use cosmwasm_std::{testing::MockStorage, Decimal, Order, Storage, Uint128};
use interface::lsm::ValidatorInfo;

use crate::{
    state::{VALIDATORS_INFO, VALIDATORS_PER_ROUND, VALIDATORS_STORE_INITIALIZED},
    utils::{initialize_validator_store, is_validator_store_initialized, load_validators_infos},
};

fn sample_validators() -> Vec<ValidatorInfo> {
    vec![
        ValidatorInfo::new(
            "validator-1".to_string(),
            Uint128::new(300),
            Decimal::percent(100),
        ),
        ValidatorInfo::new(
            "validator-2".to_string(),
            Uint128::new(200),
            Decimal::percent(90),
        ),
        ValidatorInfo::new(
            "validator-3".to_string(),
            Uint128::new(100),
            Decimal::percent(80),
        ),
    ]
}

fn seed_round(storage: &mut dyn Storage, round_id: u64, validators: &[ValidatorInfo]) {
    for validator in validators {
        VALIDATORS_INFO
            .save(storage, (round_id, validator.address.clone()), validator)
            .unwrap();
        VALIDATORS_PER_ROUND
            .save(
                storage,
                (
                    round_id,
                    validator.delegated_tokens.u128(),
                    validator.address.clone(),
                ),
                &validator.address,
            )
            .unwrap();
    }

    VALIDATORS_STORE_INITIALIZED
        .save(storage, round_id, &true)
        .unwrap();
}

// Asserts that round_id has been initialized and both VALIDATORS_INFO and VALIDATORS_PER_ROUND
// contain exactly `expected` (which must already be sorted by address, ascending).
fn assert_round_populated(storage: &dyn Storage, round_id: u64, expected: &[ValidatorInfo]) {
    assert!(
        is_validator_store_initialized(storage, round_id),
        "round {round_id} should be initialized"
    );

    assert_eq!(
        load_validators_infos(storage, round_id),
        expected,
        "VALIDATORS_INFO mismatch for round {round_id}"
    );

    let per_round: Vec<String> = VALIDATORS_PER_ROUND
        .sub_prefix(round_id)
        .range(storage, None, None, Order::Ascending)
        .map(|entry| entry.unwrap().1)
        .collect();
    // VALIDATORS_PER_ROUND is keyed by (round_id, delegated_tokens, address), so ascending
    // iteration order sorts by delegated_tokens first, not by address.
    let mut expected_sorted = expected.to_vec();
    expected_sorted.sort_by_key(|v| v.delegated_tokens);
    let expected_addresses: Vec<String> =
        expected_sorted.iter().map(|v| v.address.clone()).collect();
    assert_eq!(
        per_round, expected_addresses,
        "VALIDATORS_PER_ROUND mismatch for round {round_id}"
    );
}

// Asserts that round_id has not been touched at all- not marked initialized and no validator
// entries in either store.
fn assert_round_not_initialized(storage: &dyn Storage, round_id: u64) {
    assert!(
        !is_validator_store_initialized(storage, round_id),
        "round {round_id} should not be initialized"
    );
    assert!(
        load_validators_infos(storage, round_id).is_empty(),
        "VALIDATORS_INFO for round {round_id} should be empty"
    );
    assert_eq!(
        VALIDATORS_PER_ROUND
            .sub_prefix(round_id)
            .range(storage, None, None, Order::Ascending)
            .count(),
        0,
        "VALIDATORS_PER_ROUND for round {round_id} should be empty"
    );
}

#[test]
fn test_initialize_validator_store_current_round_already_initialized() {
    let mut storage = MockStorage::new();
    let validators = sample_validators();

    seed_round(&mut storage, 5, &validators);

    initialize_validator_store(&mut storage, 5).unwrap();

    // Nothing should have changed: round 5 still has exactly the seeded data, and no other
    // round got touched.
    assert_round_populated(&storage, 5, &validators);

    let initialized_rounds: Vec<u64> = VALIDATORS_STORE_INITIALIZED
        .range(&storage, None, None, Order::Ascending)
        .map(|entry| entry.unwrap().0)
        .collect();
    assert_eq!(initialized_rounds, vec![5]);
}

#[test]
fn test_initialize_validator_store_one_round_after_last_initialized() {
    let mut storage = MockStorage::new();
    let validators = sample_validators();

    seed_round(&mut storage, 5, &validators);

    initialize_validator_store(&mut storage, 6).unwrap();

    // Round 6 (the current round) gets populated by copying round 5's validators...
    assert_round_populated(&storage, 6, &validators);
    // ...but nothing beyond the current round is touched.
    assert_round_not_initialized(&storage, 7);

    let initialized_rounds: Vec<u64> = VALIDATORS_STORE_INITIALIZED
        .range(&storage, None, None, Order::Ascending)
        .map(|entry| entry.unwrap().0)
        .collect();
    assert_eq!(initialized_rounds, vec![5, 6]);
}

#[test]
fn test_initialize_validator_store_last_initialized_ten_rounds_in_the_past() {
    let mut storage = MockStorage::new();
    let validators = sample_validators();

    seed_round(&mut storage, 5, &validators);

    let current_round = 15;
    initialize_validator_store(&mut storage, current_round).unwrap();

    // Every round from the one after the last initialized round up to and including the
    // current round should now be populated with the same validators.
    for round_id in 6..=current_round {
        assert_round_populated(&storage, round_id, &validators);
    }
    assert_round_not_initialized(&storage, current_round + 1);

    let initialized_rounds: Vec<u64> = VALIDATORS_STORE_INITIALIZED
        .range(&storage, None, None, Order::Ascending)
        .map(|entry| entry.unwrap().0)
        .collect();
    assert_eq!(initialized_rounds, (5..=current_round).collect::<Vec<_>>());
}
