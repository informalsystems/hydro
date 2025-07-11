use std::collections::HashMap;

use cosmos_sdk_proto::cosmos::staking::v1beta1::Validator as CosmosValidator;
use cosmos_sdk_proto::prost::Message;
use cosmwasm_std::{
    attr, coins, testing::mock_env, Addr, BankMsg, Binary, Coin, Decimal, SubMsg, Uint128,
};
use neutron_sdk::{
    bindings::types::StorageValue,
    interchain_queries::{types::QueryType, v047::types::STAKING_STORE_KEY},
    sudo::msg::SudoMsg,
};

use crate::{
    contract::{
        execute, instantiate, query_icq_managers, query_validators_info,
        query_validators_per_round, sudo, NATIVE_TOKEN_DENOM,
    },
    error::ContractError,
    msg::{ExecuteMsg, TokenInfoProviderInstantiateMsg},
    state::{
        ValidatorInfo, QUERY_ID_TO_VALIDATOR, VALIDATORS_INFO, VALIDATORS_PER_ROUND,
        VALIDATOR_TO_QUERY_ID,
    },
    testing::{
        get_address_as_str, get_default_instantiate_msg, get_message_info, VALIDATOR_1,
        VALIDATOR_2, VALIDATOR_3,
    },
    testing_mocks::{
        custom_interchain_query_mock, min_query_deposit_grpc_query_mock, mock_dependencies,
        no_op_grpc_query_mock, ICQMockData,
    },
    validators_icqs::TOKENS_TO_SHARES_MULTIPLIER,
};

struct ICQResultsParseTestCase {
    description: String,
    query_id: u64,
    mock_data: HashMap<u64, ICQMockData>,
    expected_validator_added: Option<ValidatorInfo>,
}

#[test]
fn create_interchain_queries_test() {
    let min_deposit = Coin::new(1000000u64, NATIVE_TOKEN_DENOM);
    let (mut deps, env) = (
        mock_dependencies(min_query_deposit_grpc_query_mock(min_deposit.clone())),
        mock_env(),
    );
    let info = get_message_info(&deps.api, "addr0000", &[]);

    let mut msg = get_default_instantiate_msg(&deps.api);
    msg.icq_managers = vec![]; // make sure we have no icq managers
    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());

    let msg = ExecuteMsg::CreateICQsForValidators {
        validators: vec![
            VALIDATOR_1.to_string(),
            VALIDATOR_2.to_string(),
            // duplicate
            VALIDATOR_1.to_string(),
            // invalid cosmosvaloper address (last 3 chars edited)
            "cosmosvaloper157v7tczs40axfgejp2m43kwuzqe0wsy0rv8fff".to_string(),
            // valoper addresses with different prefixes are also invalid
            "injvaloper1agu7gu9ay39jkaccsfnt0ykjce6daycjuzyg2a".to_string(),
            // account addresses are also invalid
            "cosmos18gt0fzdd0ay8zceprumcalux3vv348hpqflrtr".to_string(),
            "invalid_address".to_string(),
        ],
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone()).unwrap_err();
    assert!(res.to_string().to_lowercase().contains("no funds sent"));

    let user_token = Coin::new(1000u128, NATIVE_TOKEN_DENOM);
    // in the msg above there are 2 valid addresses, hence 2 * min_deposit
    let min_deposit_required = Coin::new(2 * min_deposit.amount.u128(), min_deposit.denom.clone());

    let info = get_message_info(&deps.api, "addr0000", &[user_token.clone()]);
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone()).unwrap_err();

    assert!(
        res
        .to_string()
        .to_lowercase().contains(format!("insufficient tokens sent to pay for {} interchain queries deposits. sent: {}, required: {}", 2, user_token, min_deposit_required).as_str()));

    let info = get_message_info(&deps.api, "addr0000", &[min_deposit_required.clone()]);
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());
    let messages = res.unwrap().messages;
    assert_eq!(messages.len(), 2);
}

#[test]
fn icq_results_parse_test() {
    let mock_tokens = Uint128::new(1000001000);
    let mock_shares = Uint128::new(1000001000) * TOKENS_TO_SHARES_MULTIPLIER;
    let mock_validator = get_mock_validator(VALIDATOR_1, mock_tokens, mock_shares);

    let test_cases = vec![
        ICQResultsParseTestCase {
            description: "failed to obtain registered query".to_string(),
            expected_validator_added: None,
            query_id: 1,
            mock_data: HashMap::from([(
                1,
                ICQMockData {
                    query_type: QueryType::KV,
                    should_query_return_error: true,
                    should_query_result_return_error: false,
                    kv_results: vec![],
                },
            )]),
        },
        ICQResultsParseTestCase {
            description: "failed to obtain registered query result".to_string(),
            query_id: 1,
            expected_validator_added: None,
            mock_data: HashMap::from([(
                1,
                ICQMockData {
                    query_type: QueryType::KV,
                    should_query_return_error: false,
                    should_query_result_return_error: true,
                    kv_results: vec![],
                },
            )]),
        },
        ICQResultsParseTestCase {
            description: "wrong interchain query type".to_string(),
            expected_validator_added: None,
            query_id: 1,
            mock_data: HashMap::from([(
                1,
                ICQMockData {
                    query_type: QueryType::TX,
                    should_query_return_error: false,
                    should_query_result_return_error: true,
                    kv_results: vec![],
                },
            )]),
        },
        ICQResultsParseTestCase {
            description: "no KV results received".to_string(),
            query_id: 1,
            expected_validator_added: None,
            mock_data: HashMap::from([(
                1,
                ICQMockData {
                    query_type: QueryType::KV,
                    should_query_return_error: false,
                    should_query_result_return_error: false,
                    kv_results: vec![],
                },
            )]),
        },
        ICQResultsParseTestCase {
            description:
                "KV results with empty storage value received (address is not a validator)"
                    .to_string(),
            query_id: 1,
            expected_validator_added: None,
            mock_data: HashMap::from([(
                1,
                ICQMockData {
                    query_type: QueryType::KV,
                    should_query_return_error: false,
                    should_query_result_return_error: false,
                    kv_results: vec![StorageValue {
                        storage_prefix: STAKING_STORE_KEY.to_string(),
                        key: Binary::default(),
                        value: Binary::default(),
                    }],
                },
            )]),
        },
        ICQResultsParseTestCase {
            description: "happy path".to_string(),
            query_id: 1,
            expected_validator_added: Some(ValidatorInfo {
                address: mock_validator.operator_address.clone(),
                delegated_tokens: mock_tokens,
                power_ratio: Decimal::one(),
            }),
            mock_data: HashMap::from([(
                1,
                ICQMockData {
                    query_type: QueryType::KV,
                    should_query_return_error: false,
                    should_query_result_return_error: false,
                    kv_results: vec![StorageValue {
                        storage_prefix: STAKING_STORE_KEY.to_string(),
                        key: Binary::default(),
                        value: Binary::from(mock_validator.encode_to_vec()),
                    }],
                },
            )]),
        },
    ];

    for test_case in test_cases {
        println!("running test case: {}", test_case.description);

        let (mut deps, env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());
        deps.querier = deps
            .querier
            .with_custom_handler(custom_interchain_query_mock(test_case.mock_data));
        let info = get_message_info(&deps.api, "addr0000", &[]);

        let msg = get_default_instantiate_msg(&deps.api);
        let res = instantiate(deps.as_mut(), env.clone(), info, msg.clone());
        assert!(res.is_ok());

        let msg = SudoMsg::KVQueryResult {
            query_id: test_case.query_id,
        };
        let res = sudo(deps.as_mut(), env, msg);
        assert!(res.is_ok());

        let res = query_validators_info(deps.as_ref(), 0);
        assert!(res.is_ok());

        let validators_info = res.unwrap();
        match test_case.expected_validator_added {
            None => {
                assert!(validators_info.is_empty());
            }
            Some(expected_validator_info) => {
                assert_eq!(validators_info.len(), 1);
                assert_eq!(expected_validator_info.address, validators_info[0].address);
                assert_eq!(
                    expected_validator_info.delegated_tokens,
                    validators_info[0].delegated_tokens
                );
                assert_eq!(
                    expected_validator_info.power_ratio,
                    validators_info[0].power_ratio
                );
            }
        }
    }
}

struct ICQResultsStoreUpdateTestCase {
    description: String,
    query_id: u64,
    top_n_validators: u64,
    initial_validators: Vec<ValidatorInfo>,
    mock_data: HashMap<u64, ICQMockData>,
    // order is important- highest delegated tokens first
    expected_validators: Vec<ValidatorInfo>,
}

#[test]
fn icq_results_state_update_test() {
    let mock_tokens1 = Uint128::new(270000000);
    let mock_shares1 = Uint128::new(300000000) * TOKENS_TO_SHARES_MULTIPLIER;
    let mock_power_ratio1 =
        Decimal::from_ratio(mock_tokens1 * TOKENS_TO_SHARES_MULTIPLIER, mock_shares1);

    let mock_validator1 = get_mock_validator(VALIDATOR_1, mock_tokens1, mock_shares1);

    let test_cases: Vec<ICQResultsStoreUpdateTestCase> = vec![
        ICQResultsStoreUpdateTestCase {
            description: "ICQ result received for a validator when there are no validators in the set- it gets added".to_string(),
            query_id: 1,
            top_n_validators: 3,
            initial_validators: vec![],
            expected_validators: vec![ValidatorInfo {
                address: mock_validator1.operator_address.clone(),
                delegated_tokens: mock_tokens1,
                power_ratio: mock_power_ratio1,
            }],
            mock_data: HashMap::from([(1, ICQMockData {
                query_type: QueryType::KV,
                should_query_return_error: false,
                should_query_result_return_error: false,
                kv_results: vec![StorageValue {
                    storage_prefix: STAKING_STORE_KEY.to_string(),
                    key: Binary::default(),
                    value: Binary::from(mock_validator1.encode_to_vec()),
                }],
            })]),
        },
        ICQResultsStoreUpdateTestCase {
            description: "ICQ result received for a validator that is already in the set- it gets updated".to_string(),
            query_id: 1,
            top_n_validators: 3,
            initial_validators: vec![ValidatorInfo {
                address: VALIDATOR_1.to_string(),
                delegated_tokens: Uint128::new(150000000),
                power_ratio: Decimal::one(),
            },
            ValidatorInfo {
                address: VALIDATOR_2.to_string(),
                delegated_tokens: Uint128::new(250000000),
                power_ratio: Decimal::one(),
            }],
            expected_validators: vec![ValidatorInfo {
                address: mock_validator1.operator_address.clone(),
                delegated_tokens: mock_tokens1,
                power_ratio: mock_power_ratio1,
            },
            ValidatorInfo {
                address: VALIDATOR_2.to_string(),
                delegated_tokens: Uint128::new(250000000),
                power_ratio: Decimal::one(),
            }],
            mock_data: HashMap::from([(1, ICQMockData {
                query_type: QueryType::KV,
                should_query_return_error: false,
                should_query_result_return_error: false,
                kv_results: vec![StorageValue {
                    storage_prefix: STAKING_STORE_KEY.to_string(),
                    key: Binary::default(),
                    value: Binary::from(mock_validator1.encode_to_vec()),
                }],
            })]),
        },
        ICQResultsStoreUpdateTestCase {
            description: "ICQ result received for a new validator that has less delegated tokens than the last one in the top N- nothing changes".to_string(),
            query_id: 1,
            top_n_validators: 2,
            initial_validators: vec![ValidatorInfo {
                address: VALIDATOR_2.to_string(),
                delegated_tokens: Uint128::new(500000000),
                power_ratio: Decimal::one(),
            },
            ValidatorInfo {
                address: VALIDATOR_3.to_string(),
                delegated_tokens: Uint128::new(400000000),
                power_ratio: Decimal::one(),
            }],
            expected_validators: vec![ValidatorInfo {
                address: VALIDATOR_2.to_string(),
                delegated_tokens: Uint128::new(500000000),
                power_ratio: Decimal::one(),
            },
            ValidatorInfo {
                address: VALIDATOR_3.to_string(),
                delegated_tokens: Uint128::new(400000000),
                power_ratio: Decimal::one(),
            }],
            mock_data: HashMap::from([(1, ICQMockData {
                query_type: QueryType::KV,
                should_query_return_error: false,
                should_query_result_return_error: false,
                kv_results: vec![StorageValue {
                    storage_prefix: STAKING_STORE_KEY.to_string(),
                    key: Binary::default(),
                    value: Binary::from(mock_validator1.encode_to_vec()),
                }],
            })]),
        },
        ICQResultsStoreUpdateTestCase {
            description: "ICQ result received for a new validator that has more delegated tokens than the last one in the top N- it gets into the top N set".to_string(),
            query_id: 1,
            top_n_validators: 2,
            initial_validators: vec![ValidatorInfo {
                address: VALIDATOR_2.to_string(),
                delegated_tokens: Uint128::new(250000000),
                power_ratio: Decimal::one(),
            },
            ValidatorInfo {
                address: VALIDATOR_3.to_string(),
                delegated_tokens: Uint128::new(210000000),
                power_ratio: Decimal::one(),
            }],
            expected_validators: vec![ValidatorInfo {
                address: VALIDATOR_1.to_string(),
                delegated_tokens: mock_tokens1,
                power_ratio: mock_power_ratio1,
            },
            ValidatorInfo {
                address: VALIDATOR_2.to_string(),
                delegated_tokens: Uint128::new(250000000),
                power_ratio: Decimal::one(),
            }],
            mock_data: HashMap::from([(1, ICQMockData {
                query_type: QueryType::KV,
                should_query_return_error: false,
                should_query_result_return_error: false,
                kv_results: vec![StorageValue {
                    storage_prefix: STAKING_STORE_KEY.to_string(),
                    key: Binary::default(),
                    value: Binary::from(mock_validator1.encode_to_vec()),
                }],
            })]),
        },
    ];

    for test_case in test_cases {
        println!("running test case: {}", test_case.description);

        let (mut deps, env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());
        deps.querier = deps
            .querier
            .with_custom_handler(custom_interchain_query_mock(test_case.mock_data));
        let info = get_message_info(&deps.api, "addr0000", &[]);

        let mut msg = get_default_instantiate_msg(&deps.api);
        msg.token_info_providers[0] = TokenInfoProviderInstantiateMsg::LSM {
            max_validator_shares_participating: test_case.top_n_validators,
            hub_connection_id: "connection-0".to_string(),
            hub_transfer_channel_id: "channel-0".to_string(),
            icq_update_period: 100,
        };

        let res = instantiate(deps.as_mut(), env.clone(), info, msg.clone());
        assert!(res.is_ok());

        let current_round = 0u64;

        // setup initial validators
        let mut mock_query_id = 1;
        for validator in test_case.initial_validators {
            let res = VALIDATORS_INFO.save(
                deps.as_mut().storage,
                (current_round, validator.address.clone()),
                &validator,
            );
            assert!(res.is_ok());
            let res = VALIDATORS_PER_ROUND.save(
                deps.as_mut().storage,
                (
                    current_round,
                    validator.delegated_tokens.u128(),
                    validator.address.clone(),
                ),
                &validator.address,
            );
            assert!(res.is_ok());

            let res = VALIDATOR_TO_QUERY_ID.save(
                deps.as_mut().storage,
                validator.address.clone(),
                &mock_query_id,
            );
            assert!(res.is_ok());

            let res = QUERY_ID_TO_VALIDATOR.save(
                deps.as_mut().storage,
                mock_query_id,
                &validator.address,
            );
            assert!(res.is_ok());

            mock_query_id += 1;
        }

        let msg = SudoMsg::KVQueryResult {
            query_id: test_case.query_id,
        };
        let res = sudo(deps.as_mut(), env, msg);
        assert!(res.is_ok());

        // returns validators for the current round ordered by the number of delegated tokens- descending
        let validators_per_round =
            query_validators_per_round(deps.as_ref(), current_round).unwrap();
        assert_eq!(
            test_case.expected_validators.len(),
            validators_per_round.len()
        );

        // order of expected_validators is important- highest delegated tokens first
        #[allow(clippy::needless_range_loop)]
        for i in 0..test_case.expected_validators.len() {
            let expected_validator = test_case.expected_validators[i].clone();
            let actual_validator = validators_per_round[i].clone();

            assert_eq!(
                expected_validator.delegated_tokens.u128(),
                actual_validator.0
            );
            assert_eq!(
                expected_validator.address.clone(),
                actual_validator.1.clone()
            );

            // load the validator info and check that the expected info matches
            let validator_info = VALIDATORS_INFO
                .load(
                    deps.as_ref().storage,
                    (current_round, actual_validator.1.clone()),
                )
                .unwrap();

            assert_eq!(expected_validator.address, validator_info.address);
            assert_eq!(
                expected_validator.delegated_tokens,
                validator_info.delegated_tokens
            );
            assert_eq!(expected_validator.power_ratio, validator_info.power_ratio);
        }
    }
}

pub fn mock_get_icq_result_for_validator(
    validator: &str,
    mock_tokens: u128,
    mock_shares_tokens: u128,
) -> HashMap<u64, ICQMockData> {
    let mock_tokens = Uint128::new(mock_tokens);
    let mock_shares = Uint128::new(mock_shares_tokens) * TOKENS_TO_SHARES_MULTIPLIER;
    let mock_validator = get_mock_validator(validator, mock_tokens, mock_shares);

    HashMap::from([(
        1,
        ICQMockData {
            query_type: QueryType::KV,
            should_query_return_error: false,
            should_query_result_return_error: false,
            kv_results: vec![StorageValue {
                storage_prefix: STAKING_STORE_KEY.to_string(),
                key: Binary::default(),
                value: Binary::from(mock_validator.encode_to_vec()),
            }],
        },
    )])
}

pub fn get_mock_validator(address: &str, tokens: Uint128, shares: Uint128) -> CosmosValidator {
    CosmosValidator {
        operator_address: address.to_string(),
        tokens: tokens.to_string(),
        delegator_shares: shares.to_string(),
        ..CosmosValidator::default()
    }
}

#[test]
fn test_icq_managers_feature() {
    let mut deps = mock_dependencies(no_op_grpc_query_mock());
    let env = mock_env();
    let admin = "admin";
    let non_manager = "non_manager";
    let non_manager_addr = get_address_as_str(&deps.api, non_manager);
    let initial_icq_manager = "manager";
    let initial_icq_manager_addr = get_address_as_str(&deps.api, initial_icq_manager);
    let info = get_message_info(&deps.api, admin, &coins(1000, NATIVE_TOKEN_DENOM));

    // Instantiate the contract
    let mut instantiate_msg = get_default_instantiate_msg(&deps.api);
    instantiate_msg.whitelist_admins = vec![get_address_as_str(&deps.api, admin)];
    instantiate_msg.icq_managers = vec![initial_icq_manager_addr.clone()];
    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), instantiate_msg);
    assert!(res.is_ok(), "Error: {res:?}");

    // query the initial icq managers to make sure that the manager was added correctly
    let managers = query_icq_managers(deps.as_ref()).unwrap().managers;
    assert!(
        managers.contains(&deps.api.addr_make(initial_icq_manager)),
        "Managers: {managers:?}"
    );

    // Scenario 1: An address that is not an ICQ manager cannot withdraw funds
    let non_manager_info = get_message_info(&deps.api, non_manager, &[]);
    let withdraw_msg = ExecuteMsg::WithdrawICQFunds {
        amount: Uint128::new(100),
    };
    let res = execute(
        deps.as_mut(),
        env.clone(),
        non_manager_info.clone(),
        withdraw_msg.clone(),
    );
    match res {
        Err(ContractError::Unauthorized) => {}
        _ => panic!("Expected Unauthorized error"),
    }

    // Scenario 2: Add that address to the ICQ managers and check that it was added correctly
    let add_manager_msg = ExecuteMsg::AddICQManager {
        address: non_manager_addr.clone(),
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), add_manager_msg);
    assert!(res.is_ok(), "Error: {res:?}");

    let managers = query_icq_managers(deps.as_ref()).unwrap().managers;
    assert!(
        managers.contains(&deps.api.addr_make(non_manager)),
        "Managers: {managers:?}"
    );

    // Scenario 3: Check that the manager address can withdraw funds
    let res = execute(
        deps.as_mut(),
        env.clone(),
        non_manager_info.clone(),
        withdraw_msg.clone(),
    )
    .unwrap();
    assert_eq!(
        res.attributes,
        vec![
            attr("action", "withdraw_icq_escrows"),
            attr("sender", non_manager_addr.clone()),
        ]
    );
    assert_eq!(
        res.messages,
        vec![SubMsg::new(BankMsg::Send {
            to_address: non_manager_addr.clone(),
            amount: vec![Coin {
                denom: NATIVE_TOKEN_DENOM.to_string(),
                amount: Uint128::new(100),
            }],
        })]
    );

    // Scenario 4: Check that the manager address can create ICQs without needing to send funds
    let create_icq_msg = ExecuteMsg::CreateICQsForValidators {
        validators: vec![VALIDATOR_1.to_string()],
    };
    let res = execute(
        deps.as_mut(),
        env.clone(),
        non_manager_info.clone(),
        create_icq_msg.clone(),
    )
    .unwrap();
    assert_eq!(
        res.attributes,
        vec![
            attr("action", "create_icqs_for_validators"),
            attr("sender", non_manager_addr.clone()),
            attr("validator_addresses", VALIDATOR_1.to_string()),
        ]
    );

    // Scenario 5: Remove the manager from the managers list and check that the removal was processed correctly
    let remove_manager_msg = ExecuteMsg::RemoveICQManager {
        address: non_manager_addr.clone(),
    };
    let _res = execute(deps.as_mut(), env.clone(), info.clone(), remove_manager_msg).unwrap();

    let managers: Vec<Addr> = query_icq_managers(deps.as_ref()).unwrap().managers;
    assert!(!managers.contains(&Addr::unchecked(non_manager)));

    // Check that the removed manager cannot withdraw funds anymore
    let res = execute(
        deps.as_mut(),
        env.clone(),
        non_manager_info.clone(),
        withdraw_msg,
    );
    match res {
        Err(ContractError::Unauthorized) => {}
        _ => panic!("Expected Unauthorized error"),
    }
}
