use cosmwasm_std::testing::{mock_dependencies, mock_env};
use cosmwasm_std::{Decimal, Timestamp};
use cw2::set_contract_version;
use cw_storage_plus::Map;

use crate::contract::CONTRACT_NAME;
use crate::migration::migrate::{migrate, MigrateMsg};
use crate::migration::v3_6_4::ConstantsV3_6_4;
use crate::state::{Constants, CONSTANTS};
use crate::testing::{get_default_cw721_collection_info, get_default_power_schedule};

#[test]
fn migrate_constants_test() {
    let (mut deps, env) = (mock_dependencies(), mock_env());

    const OLD_CONSTANTS: Map<u64, ConstantsV3_6_4> = Map::new("constants");

    let timestamp1 = 1763560800000000000;
    let old_constants1 = ConstantsV3_6_4 {
        round_length: 86400000000000,
        lock_epoch_length: 86400000000000,
        first_round_start: Timestamp::from_nanos(timestamp1),
        max_locked_tokens: 50000,
        known_users_cap: 0,
        paused: false,
        max_deployment_duration: 1,
        round_lock_power_schedule: get_default_power_schedule(),
        cw721_collection_info: get_default_cw721_collection_info(),
        lock_expiry_duration_seconds: 15552000,
        lock_depth_limit: 100,
        slash_percentage_threshold: Decimal::percent(50),
        slash_tokens_receiver_addr: deps.api.addr_make("address0001").to_string(),
    };

    let timestamp2 = 1764511200000000000;
    let old_constants2 = ConstantsV3_6_4 {
        round_length: 86400000000000,
        lock_epoch_length: 86400000000000,
        first_round_start: Timestamp::from_nanos(timestamp1),
        max_locked_tokens: 55000,
        known_users_cap: 0,
        paused: false,
        max_deployment_duration: 3,
        round_lock_power_schedule: get_default_power_schedule(),
        cw721_collection_info: get_default_cw721_collection_info(),
        lock_expiry_duration_seconds: 15552000,
        lock_depth_limit: 150,
        slash_percentage_threshold: Decimal::percent(30),
        slash_tokens_receiver_addr: deps.api.addr_make("address0001").to_string(),
    };

    OLD_CONSTANTS
        .save(deps.as_mut().storage, timestamp1, &old_constants1)
        .unwrap();
    OLD_CONSTANTS
        .save(deps.as_mut().storage, timestamp2, &old_constants2)
        .unwrap();

    let old_constants = [old_constants1.clone(), old_constants2.clone()];

    // Set initial contract version to v3.6.3 to be able to migrate to the latest version
    set_contract_version(deps.as_mut().storage, CONTRACT_NAME, "v3.6.3").unwrap();

    let conversion_fee = Decimal::percent(5);

    migrate(
        deps.as_mut(),
        env,
        MigrateMsg {
            lockup_conversion_fee_percent: conversion_fee,
        },
    )
    .expect("migration failed");

    let new_constants1: Constants = CONSTANTS
        .load(deps.as_ref().storage, timestamp1)
        .expect("migrated constants 1 missing");

    let new_constants2: Constants = CONSTANTS
        .load(deps.as_ref().storage, timestamp2)
        .expect("migrated constants 2 missing");

    let new_constants = [new_constants1.clone(), new_constants2.clone()];

    for (i, old_constants) in old_constants.iter().enumerate() {
        let new_constants = &new_constants[i];

        assert_eq!(new_constants.round_length, old_constants.round_length);
        assert_eq!(
            new_constants.lock_epoch_length,
            old_constants.lock_epoch_length
        );
        assert_eq!(
            new_constants.first_round_start,
            old_constants.first_round_start
        );
        assert_eq!(
            new_constants.max_locked_tokens,
            old_constants.max_locked_tokens
        );
        assert_eq!(new_constants.known_users_cap, old_constants.known_users_cap);
        assert_eq!(new_constants.paused, old_constants.paused);
        assert_eq!(
            new_constants.max_deployment_duration,
            old_constants.max_deployment_duration
        );
        assert_eq!(
            new_constants.round_lock_power_schedule,
            old_constants.round_lock_power_schedule
        );
        assert_eq!(
            new_constants.cw721_collection_info,
            old_constants.cw721_collection_info
        );
        assert_eq!(
            new_constants.lock_expiry_duration_seconds,
            old_constants.lock_expiry_duration_seconds
        );
        assert_eq!(
            new_constants.lock_depth_limit,
            old_constants.lock_depth_limit
        );
        assert_eq!(
            new_constants.slash_percentage_threshold,
            old_constants.slash_percentage_threshold
        );
        assert_eq!(
            new_constants.slash_tokens_receiver_addr,
            old_constants.slash_tokens_receiver_addr
        );

        // Verify new field was set to 5%
        assert_eq!(new_constants.lockup_conversion_fee_percent, conversion_fee);
    }
}
