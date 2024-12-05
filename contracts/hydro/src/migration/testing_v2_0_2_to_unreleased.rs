use std::str::FromStr;

use cosmwasm_std::{testing::mock_env, Decimal, Timestamp};
use cw2::{get_contract_version, set_contract_version};
use cw_storage_plus::Item;

use crate::{
    contract::{instantiate, CONTRACT_NAME},
    migration::{
        migrate::{migrate, CONTRACT_VERSION_UNRELEASED, CONTRACT_VERSION_V2_0_2},
        unreleased::{ConstantsUNRELEASED, ConstantsV2_0_2, MigrateMsgUNRELEASED},
    },
    testing::{get_default_instantiate_msg, get_message_info},
    testing_mocks::{mock_dependencies, no_op_grpc_query_mock},
};

#[test]
fn test_constants_migration() {
    let (mut deps, env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());

    let user_addr = "addr0000";
    let info = get_message_info(&deps.api, user_addr, &[]);

    // Instantiate the contract
    let instantiate_msg = get_default_instantiate_msg(&deps.api);
    instantiate(deps.as_mut(), env.clone(), info.clone(), instantiate_msg).unwrap();

    // Override contract version so that we can run the migration
    let res = set_contract_version(&mut deps.storage, CONTRACT_NAME, CONTRACT_VERSION_V2_0_2);
    assert!(
        res.is_ok(),
        "failed to set contract version before running the migration"
    );

    const OLD_CONSTANTS: Item<ConstantsV2_0_2> = Item::new("constants");
    const NEW_CONSTANTS: Item<ConstantsUNRELEASED> = Item::new("constants");

    // Override the constants so that they have old data structure stored before running the migration
    let old_constants = ConstantsV2_0_2 {
        round_length: 2628000000000000,
        lock_epoch_length: 2628000000000000,
        first_round_start: Timestamp::from_nanos(1730851140000000000),
        max_locked_tokens: 20000000000,
        max_validator_shares_participating: 500,
        hub_connection_id: "connection-0".to_string(),
        hub_transfer_channel_id: "channel-1".to_string(),
        icq_update_period: 109000,
        paused: false,
        is_in_pilot_mode: true,
        max_deployment_duration: 12,
    };

    let res = OLD_CONSTANTS.save(&mut deps.storage, &old_constants);
    assert!(
        res.is_ok(),
        "failed to save old constants before running the migration"
    );

    // Run the migration
    let migrate_msg = MigrateMsgUNRELEASED {};
    let res = migrate(deps.as_mut(), env.clone(), migrate_msg.clone());
    assert!(res.is_ok(), "migration failed: {}", res.unwrap_err());

    // Verify that the Constants got migrated properly
    let expected_new_constants = ConstantsUNRELEASED {
        round_length: old_constants.round_length,
        lock_epoch_length: old_constants.lock_epoch_length,
        first_round_start: old_constants.first_round_start,
        max_locked_tokens: old_constants.max_locked_tokens,
        max_validator_shares_participating: old_constants.max_validator_shares_participating,
        hub_connection_id: old_constants.hub_connection_id,
        hub_transfer_channel_id: old_constants.hub_transfer_channel_id,
        icq_update_period: old_constants.icq_update_period,
        paused: old_constants.paused,
        max_deployment_duration: old_constants.max_deployment_duration,
        round_lock_power_schedule: vec![
            (1, Decimal::from_str("1").unwrap()),
            (2, Decimal::from_str("1.25").unwrap()),
            (3, Decimal::from_str("1.5").unwrap()),
            (6, Decimal::from_str("2").unwrap()),
            (12, Decimal::from_str("4").unwrap()),
        ],
    };
    let res = NEW_CONSTANTS.load(&deps.storage);
    assert!(
        res.is_ok(),
        "failed to load new constants after running the migration"
    );
    let new_constants = res.unwrap();
    assert_eq!(
        new_constants, expected_new_constants,
        "migrated constants not equal to expected ones"
    );

    // Verify the contract version after running the migration
    let res = get_contract_version(&deps.storage);
    assert_eq!(
        res.unwrap().version,
        CONTRACT_VERSION_UNRELEASED.to_string()
    );
}
