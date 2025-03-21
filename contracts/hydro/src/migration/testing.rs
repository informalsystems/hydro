use std::str::FromStr;

use cosmwasm_std::{testing::mock_env, Decimal, Order, Timestamp};
use cw_storage_plus::Map;

use crate::{
    contract::query_token_info_providers,
    migration::unreleased::migrate_v3_1_1_to_unreleased,
    state::{Constants, RoundLockPowerSchedule, CONSTANTS},
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
