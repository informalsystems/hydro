use std::{collections::HashMap, str::FromStr};

use cosmos_sdk_proto::prost::Message;
use cosmwasm_std::{
    from_json,
    testing::{mock_env, MockApi},
    Binary, CosmosMsg, Decimal, Env, MemoryStorage, MessageInfo, OwnedDeps, Uint128, WasmMsg,
};
use interface::{
    hydro::ExecuteMsg as HydroExecuteMsg,
    lsm::{ValidatorInfo, TOKENS_TO_SHARES_MULTIPLIER},
};
use neutron_sdk::{
    bindings::{query::NeutronQuery, types::StorageValue},
    interchain_queries::{types::QueryType, v047::types::STAKING_STORE_KEY},
    sudo::msg::SudoMsg,
};

use crate::{
    contract::{execute, instantiate, query_validators_info, query_validators_per_round, sudo},
    msg::ExecuteMsg,
    state::VALIDATORS_STORE_INITIALIZED,
    testing::{
        get_default_instantiate_msg, get_message_info, hydro_round_validators_info_mock,
        VALIDATOR_1, VALIDATOR_2, VALIDATOR_3,
    },
    testing_mocks::{
        custom_interchain_query_mock, mock_dependencies, no_op_grpc_query_mock, ICQMockData,
        MockQuerier, MockWasmQuerier,
    },
    testing_validators_icqs::get_mock_validator,
};

#[test]
fn copy_round_validators_data_test() {
    let hydro_address_str = "addr0000";
    let icq_manager_address_str = "addr0001";
    let non_icq_manager_address_str = "addr0002";

    let (mut deps, env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());

    let hydro_info = get_message_info(&deps.api, hydro_address_str, &[]);
    let icq_manager_info = get_message_info(&deps.api, icq_manager_address_str, &[]);
    let non_icq_manager_info = get_message_info(&deps.api, non_icq_manager_address_str, &[]);

    let mut msg = get_default_instantiate_msg(&deps.api);
    msg.icq_managers = vec![icq_manager_info.sender.to_string()];
    let res = instantiate(deps.as_mut(), env.clone(), hydro_info.clone(), msg.clone());
    assert!(res.is_ok());

    let round_id_1 = 0;
    let round_id_2 = 1;
    let round_id_3 = 2;

    let validator_info_1 = ValidatorInfo {
        address: VALIDATOR_1.to_string().clone(),
        delegated_tokens: Uint128::new(1500000),
        power_ratio: Decimal::one(),
    };

    let validator_info_2 = ValidatorInfo {
        address: VALIDATOR_2.to_string().clone(),
        delegated_tokens: Uint128::new(1600000),
        power_ratio: Decimal::one(),
    };

    let validator_info_3 = ValidatorInfo {
        address: VALIDATOR_3.to_string().clone(),
        delegated_tokens: Uint128::new(1700000),
        power_ratio: Decimal::one(),
    };

    let hydro_round_validators = HashMap::from_iter([
        (
            round_id_1,
            vec![validator_info_1.clone(), validator_info_2.clone()],
        ),
        (
            round_id_2,
            vec![validator_info_2.clone(), validator_info_3.clone()],
        ),
        (
            round_id_3,
            vec![validator_info_1.clone(), validator_info_3.clone()],
        ),
    ]);

    deps.querier.update_wasm(move |q| {
        MockWasmQuerier::new(hydro_round_validators_info_mock(
            round_id_3,
            hydro_round_validators.clone(),
        ))
        .handler(q)
    });

    // Have non-ICQ manager try to copy round data.
    let msg = ExecuteMsg::CopyRoundValidatorsData {
        round_id: round_id_1,
    };
    let res = execute(
        deps.as_mut(),
        env.clone(),
        non_icq_manager_info.clone(),
        msg,
    );
    assert!(res.unwrap_err().to_string().contains("Unauthorized"));

    // ICQ manager copies round 1 data
    execute_copy_and_verify_round_data(
        &mut deps,
        env.clone(),
        icq_manager_info.clone(),
        round_id_1,
        vec![validator_info_2.clone(), validator_info_1.clone()],
    );

    // ICQ manager copies round 2 data
    execute_copy_and_verify_round_data(
        &mut deps,
        env.clone(),
        icq_manager_info.clone(),
        round_id_2,
        vec![validator_info_3.clone(), validator_info_2.clone()],
    );

    // ICQ manager copies round 3 data
    execute_copy_and_verify_round_data(
        &mut deps,
        env.clone(),
        icq_manager_info.clone(),
        round_id_3,
        vec![validator_info_3.clone(), validator_info_1.clone()],
    );

    // After every round data has been copied from Hydro contract, execute sudo() call that delivers ICQ results.
    // 1. validator_1 has no changes in delegated tokens, nor in power ratio
    // 2. validator_2 enters the top N for round 3
    // 3. validator_3 has its power ratio updated from 1 to 0.99
    let validator_1_result = get_mock_validator(
        &validator_info_1.address,
        Uint128::new(1500000),
        Uint128::new(1500000) * TOKENS_TO_SHARES_MULTIPLIER,
    );
    let validator_2_result = get_mock_validator(
        &validator_info_2.address,
        Uint128::new(1600000),
        Uint128::new(1600000) * TOKENS_TO_SHARES_MULTIPLIER,
    );
    let validator_3_result = get_mock_validator(
        &validator_info_3.address,
        Uint128::new(1683000),
        Uint128::new(1700000) * TOKENS_TO_SHARES_MULTIPLIER,
    );

    let validator_1_icq_id = 153;
    let validator_2_icq_id = 154;
    let validator_3_icq_id = 155;

    let validator_icq_mock = HashMap::from([
        (
            validator_1_icq_id,
            ICQMockData {
                query_type: QueryType::KV,
                should_query_return_error: false,
                should_query_result_return_error: false,
                kv_results: vec![StorageValue {
                    storage_prefix: STAKING_STORE_KEY.to_string(),
                    key: Binary::default(),
                    value: Binary::from(validator_1_result.encode_to_vec()),
                }],
            },
        ),
        (
            validator_2_icq_id,
            ICQMockData {
                query_type: QueryType::KV,
                should_query_return_error: false,
                should_query_result_return_error: false,
                kv_results: vec![StorageValue {
                    storage_prefix: STAKING_STORE_KEY.to_string(),
                    key: Binary::default(),
                    value: Binary::from(validator_2_result.encode_to_vec()),
                }],
            },
        ),
        (
            validator_3_icq_id,
            ICQMockData {
                query_type: QueryType::KV,
                should_query_return_error: false,
                should_query_result_return_error: false,
                kv_results: vec![StorageValue {
                    storage_prefix: STAKING_STORE_KEY.to_string(),
                    key: Binary::default(),
                    value: Binary::from(validator_3_result.encode_to_vec()),
                }],
            },
        ),
    ]);

    deps.querier = deps
        .querier
        .with_custom_handler(custom_interchain_query_mock(validator_icq_mock));

    let msg = SudoMsg::KVQueryResult {
        query_id: validator_1_icq_id,
    };

    let res = sudo(deps.as_mut(), env.clone(), msg).unwrap();

    // Validator_1 had no changes, therefore there is no SubMsg to update power ratio in Hydro
    assert!(res.messages.is_empty());

    let msg = SudoMsg::KVQueryResult {
        query_id: validator_2_icq_id,
    };

    let res = sudo(deps.as_mut(), env.clone(), msg).unwrap();
    assert_eq!(res.messages.len(), 1);

    // Validator_2 was newly added to the round, so there should be
    // a SubMsg to update its ratio in Hydro from 0 to 1.
    match res.messages[0].clone().msg {
        CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr,
            msg,
            funds: _,
        }) => {
            assert_eq!(contract_addr, hydro_info.sender.to_string());

            let execute_msg: HydroExecuteMsg = from_json(msg).unwrap();
            match execute_msg {
                HydroExecuteMsg::UpdateTokenGroupsRatios { changes } => {
                    assert_eq!(changes.len(), 1);

                    let token_group_change = changes[0].clone();
                    assert_eq!(
                        token_group_change.token_group_id,
                        validator_2_result.operator_address
                    );
                    assert_eq!(token_group_change.old_ratio, Decimal::zero());
                    assert_eq!(token_group_change.new_ratio, Decimal::one());
                }
            }
        }
        _ => {
            panic!("unexpected msg type");
        }
    }

    let msg = SudoMsg::KVQueryResult {
        query_id: validator_3_icq_id,
    };

    let res = sudo(deps.as_mut(), env.clone(), msg).unwrap();
    assert_eq!(res.messages.len(), 1);

    // Validator_3 power ratio has changed from 1 to 0.99, so there should be
    // a SubMsg to also update its ratio in Hydro from 1 to 0.99.
    match res.messages[0].clone().msg {
        CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr,
            msg,
            funds: _,
        }) => {
            assert_eq!(contract_addr, hydro_info.sender.to_string());

            let execute_msg: HydroExecuteMsg = from_json(msg).unwrap();
            match execute_msg {
                HydroExecuteMsg::UpdateTokenGroupsRatios { changes } => {
                    assert_eq!(changes.len(), 1);

                    let token_group_change = changes[0].clone();
                    assert_eq!(
                        token_group_change.token_group_id,
                        validator_3_result.operator_address
                    );
                    assert_eq!(token_group_change.old_ratio, Decimal::one());
                    assert_eq!(
                        token_group_change.new_ratio,
                        Decimal::from_str("0.99").unwrap()
                    );
                }
            }
        }
        _ => {
            panic!("unexpected msg type");
        }
    }
}

fn execute_copy_and_verify_round_data(
    deps: &mut OwnedDeps<MemoryStorage, MockApi, MockQuerier, NeutronQuery>,
    env: Env,
    sender_info: MessageInfo,
    round_id_1: u64,
    expected_validators_ordered: Vec<ValidatorInfo>, // ordered by number of delegated tokens
) {
    let msg = ExecuteMsg::CopyRoundValidatorsData {
        round_id: round_id_1,
    };
    execute(deps.as_mut(), env.clone(), sender_info.clone(), msg).unwrap();

    assert!(VALIDATORS_STORE_INITIALIZED
        .load(&deps.storage, round_id_1)
        .unwrap());

    let validators_info = query_validators_info(deps.as_ref(), round_id_1)
        .unwrap()
        .validators;
    let validators_per_round = query_validators_per_round(deps.as_ref(), round_id_1).unwrap();

    assert_eq!(expected_validators_ordered.len(), validators_info.len());
    assert_eq!(validators_info.len(), validators_per_round.len());

    for (i, expected_validator) in expected_validators_ordered.into_iter().enumerate() {
        // By finding the address in the HashMap, address match is verified
        let validator_info = validators_info.get(&expected_validator.address).unwrap();

        assert_eq!(
            expected_validator.delegated_tokens,
            validator_info.delegated_tokens
        );
        assert_eq!(expected_validator.power_ratio, validator_info.power_ratio);

        let validator_per_round = &validators_per_round[i];

        assert_eq!(
            expected_validator.delegated_tokens.u128(),
            validator_per_round.0
        );
        assert_eq!(expected_validator.address.clone(), validator_per_round.1);
    }
}
