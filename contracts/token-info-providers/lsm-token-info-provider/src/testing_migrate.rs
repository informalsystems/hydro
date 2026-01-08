use cosmwasm_std::{testing::mock_env, Decimal, Uint128};
use cw2::set_contract_version;
use interface::lsm::ValidatorInfo;

use crate::{
    contract::{instantiate, CONTRACT_NAME},
    migrate::{migrate, MigrateMsg},
    state::{QUERY_ID_TO_VALIDATOR, VALIDATORS_INFO, VALIDATORS_PER_ROUND, VALIDATOR_TO_QUERY_ID},
    testing::{
        get_default_instantiate_msg, get_message_info, hydro_current_round_mock, VALIDATOR_1,
        VALIDATOR_2, VALIDATOR_3,
    },
    testing_mocks::{mock_dependencies, no_op_grpc_query_mock, MockWasmQuerier},
};

#[test]
fn test_migrate_removes_validators_without_queries() {
    let current_round_id = 0;
    let (mut deps, env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());
    deps.querier.update_wasm(move |q| {
        MockWasmQuerier::new(hydro_current_round_mock(current_round_id)).handler(q)
    });
    let info = get_message_info(&deps.api, "addr0000", &[]);

    // Instantiate the contract
    let msg = get_default_instantiate_msg(&deps.api);
    let res = instantiate(deps.as_mut(), env.clone(), info, msg);
    assert!(res.is_ok());

    // Set contract version to some older version to allow migration
    set_contract_version(deps.as_mut().storage, CONTRACT_NAME, "3.6.5").unwrap();

    // Setup: Define validators with their properties
    // Format: (address, delegated_tokens, has_query, query_id)
    let validators = vec![
        (VALIDATOR_1, 300000000u128, true, 1u64),
        (VALIDATOR_2, 200000000u128, false, 0u64), // No query
        (VALIDATOR_3, 100000000u128, true, 3u64),
    ];

    // Save all validators and set up queries
    for (validator_addr, delegated_tokens, has_query, query_id) in &validators {
        let validator_info = ValidatorInfo {
            address: validator_addr.to_string(),
            delegated_tokens: Uint128::new(*delegated_tokens),
            power_ratio: Decimal::one(),
        };

        VALIDATORS_INFO
            .save(
                deps.as_mut().storage,
                (current_round_id, validator_addr.to_string()),
                &validator_info,
            )
            .unwrap();

        VALIDATORS_PER_ROUND
            .save(
                deps.as_mut().storage,
                (
                    current_round_id,
                    *delegated_tokens,
                    validator_addr.to_string(),
                ),
                &validator_addr.to_string(),
            )
            .unwrap();

        // Set up query if this validator has one
        if *has_query {
            VALIDATOR_TO_QUERY_ID
                .save(deps.as_mut().storage, validator_addr.to_string(), query_id)
                .unwrap();
            QUERY_ID_TO_VALIDATOR
                .save(
                    deps.as_mut().storage,
                    *query_id,
                    &validator_addr.to_string(),
                )
                .unwrap();
        }
    }

    // Verify initial state: all validators exist
    for (validator_addr, delegated_tokens, _, _) in &validators {
        assert!(VALIDATORS_INFO
            .may_load(
                deps.as_ref().storage,
                (current_round_id, validator_addr.to_string())
            )
            .unwrap()
            .is_some());

        assert!(VALIDATORS_PER_ROUND
            .may_load(
                deps.as_ref().storage,
                (
                    current_round_id,
                    *delegated_tokens,
                    validator_addr.to_string()
                )
            )
            .unwrap()
            .is_some());
    }

    // Run migration
    let res = migrate(deps.as_mut(), env, MigrateMsg {});
    assert!(res.is_ok());

    // Verify final state: check each validator based on whether it had a query
    for (validator_addr, delegated_tokens, has_query, _) in &validators {
        let should_exist = *has_query;

        let validators_info_exists = VALIDATORS_INFO
            .may_load(
                deps.as_ref().storage,
                (current_round_id, validator_addr.to_string()),
            )
            .unwrap()
            .is_some();

        let validators_per_round_exists = VALIDATORS_PER_ROUND
            .may_load(
                deps.as_ref().storage,
                (
                    current_round_id,
                    *delegated_tokens,
                    validator_addr.to_string(),
                ),
            )
            .unwrap()
            .is_some();

        assert_eq!(
            validators_info_exists, should_exist,
            "Validator {} VALIDATORS_INFO existence mismatch",
            validator_addr
        );
        assert_eq!(
            validators_per_round_exists, should_exist,
            "Validator {} VALIDATORS_PER_ROUND existence mismatch",
            validator_addr
        );
    }
}
