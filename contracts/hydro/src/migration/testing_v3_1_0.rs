use std::str::FromStr;

use cosmwasm_std::{
    testing::{mock_env, MockApi, MockStorage},
    Addr, Coin, Decimal, Env, OwnedDeps, Timestamp, Uint128,
};
use cw2::set_contract_version;
use cw_storage_plus::{Item, Map};
use neutron_sdk::bindings::query::NeutronQuery;

use crate::{
    contract::{instantiate, CONTRACT_NAME},
    migration::{
        migrate::{migrate, CONTRACT_VERSION_V3_0_0},
        v3_0_0::ConstantsV3_0_0,
        v3_1_0::MigrateMsgV3_1_0,
    },
    state::{
        LockEntry, HEIGHT_TO_ROUND, LOCKS_MAP, ROUND_TO_HEIGHT_RANGE,
        SCALED_ROUND_POWER_SHARES_MAP, SNAPSHOTS_ACTIVATION_HEIGHT, TOTAL_VOTING_POWER_PER_ROUND,
        USER_LOCKS,
    },
    testing::{
        get_default_instantiate_msg, get_default_power_schedule, get_message_info, IBC_DENOM_1,
        IBC_DENOM_2, IBC_DENOM_3, ONE_MONTH_IN_NANO_SECONDS, VALIDATOR_1, VALIDATOR_2, VALIDATOR_3,
    },
    testing_lsm_integration::set_validators_constant_power_ratios_for_rounds,
    testing_mocks::{mock_dependencies, no_op_grpc_query_mock, MockQuerier},
    utils::load_constants_active_at_timestamp,
};

const INITIAL_TIMESTAMP: Timestamp = Timestamp::from_nanos(1738130400000000000);

#[test]
fn test_constants_and_height_mappings_migration() {
    let (mut deps, mut env, old_constants) = setup_current_contract();

    // Advance the chain into the fourth round and run the migration
    env.block.time = env
        .block
        .time
        .plus_nanos(3 * old_constants.round_length + 1);
    env.block.height += 300_000;

    let current_round_id = 3;
    let migration_height = env.block.height;

    let res = migrate(deps.as_mut(), env.clone(), MigrateMsgV3_1_0 {});
    assert!(
        res.is_ok(),
        "failed to migrate contract to the newest version"
    );

    for timestamp_to_check in [INITIAL_TIMESTAMP, env.block.time] {
        let constants = load_constants_active_at_timestamp(&deps.as_ref(), timestamp_to_check)
            .unwrap()
            .1;

        assert_eq!(constants.round_length, old_constants.round_length);
        assert_eq!(constants.lock_epoch_length, old_constants.lock_epoch_length);
        assert_eq!(constants.first_round_start, old_constants.first_round_start);
        assert_eq!(constants.max_locked_tokens, old_constants.max_locked_tokens);
        assert_eq!(
            constants.max_validator_shares_participating,
            old_constants.max_validator_shares_participating
        );
        assert_eq!(constants.hub_connection_id, old_constants.hub_connection_id);
        assert_eq!(
            constants.hub_transfer_channel_id,
            old_constants.hub_transfer_channel_id
        );
        assert_eq!(constants.icq_update_period, old_constants.icq_update_period);
        assert_eq!(constants.paused, old_constants.paused);
        assert_eq!(
            constants.max_deployment_duration,
            old_constants.max_deployment_duration
        );
        assert_eq!(
            constants.round_lock_power_schedule,
            old_constants.round_lock_power_schedule
        );
    }

    let height_range = ROUND_TO_HEIGHT_RANGE
        .load(&deps.storage, current_round_id)
        .unwrap();
    assert_eq!(height_range.lowest_known_height, migration_height);
    assert_eq!(height_range.highest_known_height, migration_height);

    assert_eq!(
        HEIGHT_TO_ROUND
            .load(&deps.storage, migration_height)
            .unwrap(),
        current_round_id
    );

    assert_eq!(
        SNAPSHOTS_ACTIVATION_HEIGHT.load(&deps.storage).unwrap(),
        migration_height
    );
}

#[test]
fn test_total_voting_power_migration() {
    let (mut deps, mut env, old_constants) = setup_current_contract();

    // Rounds IDs 0 and 1
    set_validators_constant_power_ratios_for_rounds(
        deps.as_mut(),
        0,
        2,
        vec![
            VALIDATOR_1.to_string(),
            VALIDATOR_2.to_string(),
            VALIDATOR_3.to_string(),
        ],
        vec![Decimal::one(), Decimal::one(), Decimal::one()],
    );

    // Round IDs 2 and 3
    set_validators_constant_power_ratios_for_rounds(
        deps.as_mut(),
        2,
        4,
        vec![
            VALIDATOR_1.to_string(),
            VALIDATOR_2.to_string(),
            VALIDATOR_3.to_string(),
        ],
        vec![
            Decimal::one(),
            Decimal::one(),
            Decimal::from_str("0.5").unwrap(),
        ],
    );

    // Round ID 4
    set_validators_constant_power_ratios_for_rounds(
        deps.as_mut(),
        4,
        5,
        vec![VALIDATOR_1.to_string(), VALIDATOR_3.to_string()],
        vec![
            Decimal::from_str("0.7").unwrap(),
            Decimal::from_str("0.5").unwrap(),
        ],
    );

    let validator_round_shares: Vec<(&str, Vec<u128>)> = vec![
        (
            VALIDATOR_1,
            vec![400, 400, 400, 400, 400, 400, 200, 200, 200, 150, 125, 100],
        ),
        (
            VALIDATOR_2,
            vec![400, 400, 400, 300, 250, 200, 0, 0, 0, 0, 0, 0],
        ),
        (
            VALIDATOR_3,
            vec![
                1600, 1600, 1600, 1600, 1600, 1600, 800, 800, 800, 600, 500, 400,
            ],
        ),
    ];

    let expected_round_total_powers = vec![
        (0, 2400),
        (1, 2400),
        (2, 1600),
        (3, 1500),
        (4, 1080),
        (5, 1080),
        (6, 540),
        (7, 540),
        (8, 540),
        (9, 405),
        (10, 338),
        (11, 270),
    ];

    // Insert validator shares into the store
    for validator_shares in validator_round_shares {
        for round_id in 0..12 {
            let round_shares = validator_shares.1[round_id as usize];
            if round_shares != 0 {
                let res = SCALED_ROUND_POWER_SHARES_MAP.save(
                    &mut deps.storage,
                    (round_id, validator_shares.0.to_string()),
                    &Decimal::from_ratio(round_shares, Uint128::one()),
                );
                assert!(
                    res.is_ok(),
                    "failed to save validator round power shares before running the migration"
                );
            }
        }
    }

    // Advance the chain into the fifth round and run the migration
    env.block.time = env
        .block
        .time
        .plus_nanos(4 * old_constants.round_length + 1);
    env.block.height += 400_000;

    let res = migrate(deps.as_mut(), env.clone(), MigrateMsgV3_1_0 {});
    assert!(
        res.is_ok(),
        "failed to migrate contract to the newest version"
    );

    for expected_round_total_power in expected_round_total_powers {
        let round_total_power = TOTAL_VOTING_POWER_PER_ROUND
            .load(&deps.storage, expected_round_total_power.0)
            .unwrap()
            .u128();
        assert_eq!(round_total_power, expected_round_total_power.1);
    }
}

#[test]
fn test_user_lockups_migration() {
    let (mut deps, mut env, old_constants) = setup_current_contract();

    let user1 = deps.api.addr_make("user0001");
    let user2 = deps.api.addr_make("user0002");

    // Insert user lockups into the old storage and use this data in later verification
    const OLD_LOCKS_MAP: Map<(Addr, u64), LockEntry> = Map::new("locks_map");

    let users_lockups = vec![
        (
            user1.clone(),
            vec![
                LockEntry {
                    lock_id: 0,
                    funds: Coin::new(100u128, IBC_DENOM_1),
                    lock_start: env.block.time,
                    lock_end: env.block.time.plus_nanos(old_constants.round_length),
                },
                LockEntry {
                    lock_id: 3,
                    funds: Coin::new(400u128, IBC_DENOM_2),
                    lock_start: env.block.time,
                    lock_end: env.block.time.plus_nanos(old_constants.round_length * 2),
                },
            ],
        ),
        (
            user2.clone(),
            vec![
                LockEntry {
                    lock_id: 1,
                    funds: Coin::new(200u128, IBC_DENOM_2),
                    lock_start: env.block.time,
                    lock_end: env.block.time.plus_nanos(old_constants.round_length * 3),
                },
                LockEntry {
                    lock_id: 2,
                    funds: Coin::new(300u128, IBC_DENOM_3),
                    lock_start: env.block.time,
                    lock_end: env.block.time.plus_nanos(old_constants.round_length * 6),
                },
            ],
        ),
    ];

    for user_lockups in &users_lockups {
        for lockup in &user_lockups.1 {
            let res = OLD_LOCKS_MAP.save(
                &mut deps.storage,
                (user_lockups.0.clone(), lockup.lock_id),
                lockup,
            );
            assert!(
                res.is_ok(),
                "failed to save user lockup before running the migration"
            );
        }
    }

    // Advance the chain into the fifth round and run the migration
    env.block.time = env
        .block
        .time
        .plus_nanos(4 * old_constants.round_length + 1);
    env.block.height += 400_000;

    let res = migrate(deps.as_mut(), env.clone(), MigrateMsgV3_1_0 {});
    assert!(
        res.is_ok(),
        "failed to migrate contract to the newest version"
    );

    for user_lockups in &users_lockups {
        for expected_lockup in &user_lockups.1 {
            let lockup = LOCKS_MAP
                .load(
                    &deps.storage,
                    (user_lockups.0.clone(), expected_lockup.lock_id),
                )
                .unwrap();
            assert_eq!(lockup, *expected_lockup);

            let lockup = LOCKS_MAP
                .may_load_at_height(
                    &deps.storage,
                    (user_lockups.0.clone(), expected_lockup.lock_id),
                    env.block.height + 1,
                )
                .unwrap()
                .unwrap();
            assert_eq!(lockup, *expected_lockup);
        }

        let expected_user_lockup_ids: Vec<u64> = user_lockups
            .1
            .iter()
            .map(|lock_entry| lock_entry.lock_id)
            .collect();

        let user_lockup_ids = USER_LOCKS
            .load(&deps.storage, user_lockups.0.clone())
            .unwrap();
        assert_eq!(user_lockup_ids, expected_user_lockup_ids);

        let user_lockup_ids = USER_LOCKS
            .may_load_at_height(&deps.storage, user_lockups.0.clone(), env.block.height + 1)
            .unwrap()
            .unwrap();
        assert_eq!(user_lockup_ids, expected_user_lockup_ids);
    }
}

fn setup_current_contract() -> (
    OwnedDeps<MockStorage, MockApi, MockQuerier, NeutronQuery>,
    Env,
    ConstantsV3_0_0,
) {
    let (mut deps, mut env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());

    env.block.time = INITIAL_TIMESTAMP;
    env.block.height = 19_420_000;

    let user_addr = "addr0000";
    let info = get_message_info(&deps.api, user_addr, &[]);

    // Instantiate the contract
    let mut instantiate_msg = get_default_instantiate_msg(&deps.api);
    instantiate_msg.first_round_start = INITIAL_TIMESTAMP;
    instantiate_msg.round_length = ONE_MONTH_IN_NANO_SECONDS;

    instantiate(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        instantiate_msg.clone(),
    )
    .unwrap();

    // Override contract version so that we can run the migration
    let res = set_contract_version(&mut deps.storage, CONTRACT_NAME, CONTRACT_VERSION_V3_0_0);
    assert!(
        res.is_ok(),
        "failed to set contract version before running the migration"
    );

    // Override the constants so that they have old data structure stored before running the migration
    const OLD_CONSTANTS: Item<ConstantsV3_0_0> = Item::new("constants");
    let old_constants = ConstantsV3_0_0 {
        round_length: ONE_MONTH_IN_NANO_SECONDS,
        lock_epoch_length: ONE_MONTH_IN_NANO_SECONDS,
        first_round_start: INITIAL_TIMESTAMP,
        max_locked_tokens: 20000000000,
        max_validator_shares_participating: 500,
        hub_connection_id: "connection-0".to_string(),
        hub_transfer_channel_id: "channel-1".to_string(),
        icq_update_period: 109000,
        paused: false,
        max_deployment_duration: 12,
        round_lock_power_schedule: get_default_power_schedule(),
    };

    let res = OLD_CONSTANTS.save(&mut deps.storage, &old_constants);
    assert!(
        res.is_ok(),
        "failed to save old constants before running the migration"
    );

    (deps, env, old_constants)
}
