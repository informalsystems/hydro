use std::collections::HashMap;

use cosmos_sdk_proto::prost::Message;
use cosmwasm_std::{
    testing::mock_env, BankMsg, Binary, Coin, CosmosMsg, Decimal, DepsMut, Env, StdError,
    StdResult, Storage, SystemError, SystemResult, Timestamp, Uint128,
};
use neutron_sdk::{
    bindings::{query::NeutronQuery, types::StorageValue},
    interchain_queries::{types::QueryType, v047::types::STAKING_STORE_KEY},
    sudo::msg::SudoMsg,
};
use neutron_std::types::ibc::applications::transfer::v1::QueryDenomTraceResponse;

use crate::{
    contract::{execute, instantiate, query_round_tranche_proposals, query_top_n_proposals, sudo},
    lsm_integration::{
        get_total_power_for_round, get_validator_power_ratio_for_round,
        update_scores_due_to_power_ratio_change, validate_denom,
    },
    msg::{ExecuteMsg, ProposalToLockups},
    state::{ValidatorInfo, VALIDATORS_INFO, VALIDATORS_PER_ROUND, VALIDATORS_STORE_INITIALIZED},
    testing::{
        get_default_instantiate_msg, get_message_info, set_default_validator_for_rounds,
        IBC_DENOM_1, IBC_DENOM_2, IBC_DENOM_3, ONE_DAY_IN_NANO_SECONDS, ONE_MONTH_IN_NANO_SECONDS,
        VALIDATOR_1, VALIDATOR_1_LST_DENOM_1, VALIDATOR_2, VALIDATOR_2_LST_DENOM_1, VALIDATOR_3,
        VALIDATOR_3_LST_DENOM_1,
    },
    testing_mocks::{
        custom_interchain_query_mock, denom_trace_grpc_query_mock, mock_dependencies,
        no_op_grpc_query_mock, system_result_ok_from, GrpcQueryFunc, ICQMockData,
    },
    testing_validators_icqs::get_mock_validator,
    validators_icqs::TOKENS_TO_SHARES_MULTIPLIER,
};

fn get_default_constants() -> crate::state::Constants {
    crate::state::Constants {
        round_length: ONE_DAY_IN_NANO_SECONDS,
        lock_epoch_length: 1,
        first_round_start: Timestamp::from_seconds(0),
        max_locked_tokens: 1,
        paused: false,
        max_validator_shares_participating: 2,
        hub_connection_id: "connection-0".to_string(),
        hub_transfer_channel_id: "channel-0".to_string(),
        icq_update_period: 100,
        is_in_pilot_mode: false,
        max_deployment_duration: 12,
    }
}

pub fn set_validator_infos_for_round(
    storage: &mut dyn Storage,
    round_id: u64,
    validators: Vec<String>,
) -> StdResult<()> {
    for validator in validators.iter() {
        set_validator_power_ratio(storage, round_id, validator, Decimal::one());
    }
    Ok(())
}

pub fn set_validators_constant_power_ratios_for_rounds(
    deps: DepsMut<NeutronQuery>,
    start_round: u64,
    end_round: u64,
    validators: Vec<String>,
    power_ratios: Vec<Decimal>,
) {
    for round_id in start_round..end_round {
        // set the power ratio for each validator to 1 for that round
        for (i, validator) in validators.iter().enumerate() {
            set_validator_power_ratio(deps.storage, round_id, validator, power_ratios[i]);
        }
    }
}

pub fn set_validator_power_ratio(
    storage: &mut dyn Storage,
    round_id: u64,
    validator: &str,
    power_ratio: Decimal,
) {
    let old_power_ratio =
        get_validator_power_ratio_for_round(storage, round_id, validator.to_string()).unwrap();
    if old_power_ratio != power_ratio {
        let res = update_scores_due_to_power_ratio_change(
            storage,
            validator,
            round_id,
            old_power_ratio,
            power_ratio,
        );
        assert!(res.is_ok());
    }
    let res = VALIDATORS_INFO.save(
        storage,
        (round_id, validator.to_string()),
        &ValidatorInfo {
            power_ratio,
            address: validator.to_string(),
            ..ValidatorInfo::default()
        },
    );
    assert!(res.is_ok());

    let res = VALIDATORS_PER_ROUND.save(
        storage,
        (round_id, 100, validator.to_string()),
        &validator.to_string(),
    );
    assert!(res.is_ok());

    // mark the round as having its store initialized
    let res = VALIDATORS_STORE_INITIALIZED.save(storage, round_id, &true);
    assert!(res.is_ok());
}

#[test]
fn test_validate_denom() {
    type SetupFunc = dyn Fn(&mut dyn Storage, &mut Env);

    struct TestCase {
        description: String,
        denom: String,
        expected_result: Result<String, StdError>,
        setup: Box<SetupFunc>,
        grpc_query: Box<GrpcQueryFunc>,
    }

    let test_cases = vec![
        TestCase {
            description: "non-IBC denom".to_string(),
            denom: "invalid_denom".to_string(),
            expected_result: Err(StdError::generic_err(
                "IBC token expected",
            )),
            setup: Box::new(|_storage, _env| { }),
            grpc_query: no_op_grpc_query_mock(),
        },
        TestCase {
            description: "gRPC query returns error".to_string(),
            denom: IBC_DENOM_1.to_string(),
            expected_result: Err(StdError::generic_err("Failed to obtain IBC denom trace: Generic error: Querier system error: Unknown system error")),
            setup: Box::new(|_storage, _env| { }),
            grpc_query: Box::new(|_query| { SystemResult::Err(SystemError::Unknown {}) }),
        },
        TestCase {
            description: "gRPC fails to provide denom trace information".to_string(),
            denom: IBC_DENOM_1.to_string(),
            expected_result: Err(StdError::generic_err("Failed to obtain IBC denom trace")),
            setup: Box::new(|_storage, _env| { }),
            grpc_query: Box::new(|_query| { system_result_ok_from(QueryDenomTraceResponse { denom_trace: None }.encode_to_vec()) }),
        },
        TestCase {
            description: "IBC denom received over multiple hops".to_string(),
            denom: IBC_DENOM_1.to_string(),
            expected_result: Err(StdError::generic_err(
                "Only LSTs transferred directly from the Cosmos Hub can be locked.",
            )),
            setup: Box::new(|_storage, _env| {}),
            grpc_query: denom_trace_grpc_query_mock(
                "transfer/channel-0/transfer/channel-7".to_string(),
                HashMap::from([
                    (IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string()),
                ]),
            ),
        },
        TestCase {
            description: "IBC denom received over non-transfer port".to_string(),
            denom: IBC_DENOM_1.to_string(),
            expected_result: Err(StdError::generic_err(
                "Only LSTs transferred directly from the Cosmos Hub can be locked.",
            )),
            setup: Box::new(|_storage, _env| {}),
            grpc_query: denom_trace_grpc_query_mock(
                "icahost/channel-0".to_string(),
                HashMap::from([
                    (IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string()),
                ]),
            ),
        },
        TestCase {
            description: "IBC denom received over unexpected channel ID".to_string(),
            denom: IBC_DENOM_1.to_string(),
            expected_result: Err(StdError::generic_err(
                "Only LSTs transferred directly from the Cosmos Hub can be locked.",
            )),
            setup: Box::new(|_storage, _env| {}),
            grpc_query: denom_trace_grpc_query_mock(
                "transfer/channel-1".to_string(),
                HashMap::from([
                    (IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string()),
                ]),
            ),
        },
        TestCase {
            description: "base denom not LST- has extra parts".to_string(),
            denom: IBC_DENOM_1.to_string(),
            expected_result: Err(StdError::generic_err(
                "Only LSTs from the Cosmos Hub can be locked.",
            )),
            setup: Box::new(|_storage, _env| {}),
            grpc_query: denom_trace_grpc_query_mock(
                "transfer/channel-0".to_string(),
                HashMap::from([
                    (IBC_DENOM_1.to_string(), (VALIDATOR_1_LST_DENOM_1.to_owned() + "/456").to_string()),
                ]),
            ),
        },
        TestCase {
            description: "base denom not LST- wrong validator address length".to_string(),
            denom: IBC_DENOM_1.to_string(),
            expected_result: Err(StdError::generic_err(
                "Only LSTs from the Cosmos Hub can be locked.",
            )),
            setup: Box::new(|_storage, _env| {}),
            grpc_query: denom_trace_grpc_query_mock(
                "transfer/channel-0".to_string(),
                HashMap::from([
                    (IBC_DENOM_1.to_string(), "cosmosvaloper157v7tczs40axfgejp2m43kwuzqe0wsy0rv8/789".to_string()),
                ]),
            ),
        },
        TestCase {
            description: "base denom not LST- wrong validator address prefix".to_string(),
            denom: IBC_DENOM_1.to_string(),
            expected_result: Err(StdError::generic_err(
                "Only LSTs from the Cosmos Hub can be locked.",
            )),
            setup: Box::new(|_storage, _env| {}),
            grpc_query: denom_trace_grpc_query_mock(
                "transfer/channel-0".to_string(),
                HashMap::from([
                    (IBC_DENOM_1.to_string(), "neutrnvaloper157v7tczs40axfgejp2m43kwuzqe0wsy0rv8puv/789".to_string()),
                ]),
            ),
        },
        TestCase {
            description: "base denom not LST- tokenize share record ID not a number".to_string(),
            denom: IBC_DENOM_1.to_string(),
            expected_result: Err(StdError::generic_err(
                "Only LSTs from the Cosmos Hub can be locked.",
            )),
            setup: Box::new(|_storage, _env| {}),
            grpc_query: denom_trace_grpc_query_mock(
                "transfer/channel-0".to_string(),
                HashMap::from([
                    (IBC_DENOM_1.to_string(), (VALIDATOR_1_LST_DENOM_1.to_owned() + "a")),
                ]),
            ),
        },
        TestCase {
            description: "validator not in top validators set".to_string(),
            denom: IBC_DENOM_1.to_string(),
            expected_result: Err(StdError::generic_err(format!("Validator {} is not present; possibly they are not part of the top 2 validators by delegated tokens", VALIDATOR_1))),
            setup: Box::new(|storage, _env| {
                let validators = vec![VALIDATOR_2.to_string(), VALIDATOR_3.to_string()];
                let res = set_validator_infos_for_round(storage, 0, validators);
                assert!(res.is_ok());
            }),
            grpc_query: denom_trace_grpc_query_mock(
                "transfer/channel-0".to_string(),
                HashMap::from([
                    (IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string()),
                ]),
            ),
        },
        TestCase {
            description: "validator not in top validators set".to_string(),
            denom: IBC_DENOM_1.to_string(),
            expected_result: Err(StdError::generic_err(format!("Validator {} is not present; possibly they are not part of the top 2 validators by delegated tokens", VALIDATOR_1))),
            setup: Box::new(|storage, env| {
                let res = set_validator_infos_for_round(storage, 0, vec![VALIDATOR_1.to_string(), VALIDATOR_2.to_string()]);
                assert!(res.is_ok());
                let res = set_validator_infos_for_round(storage, 1, vec![VALIDATOR_2.to_string(), VALIDATOR_3.to_string()]);
                assert!(res.is_ok());

                env.block.time = Timestamp::from_nanos(ONE_DAY_IN_NANO_SECONDS+1);
            }),
            grpc_query: denom_trace_grpc_query_mock(
                "transfer/channel-0".to_string(),
                HashMap::from([
                    (IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string()),
                ]),
            ),
        },
        TestCase {
            description: "happy path".to_string(),
            denom: IBC_DENOM_1.to_string(),
            expected_result: Ok(VALIDATOR_1.to_string()),
            setup: Box::new(|storage, _env| {
                let constants = get_default_constants();
                crate::state::CONSTANTS.save(storage, &constants).unwrap();
                let round_id = 0;
                let res = set_validator_infos_for_round(
                        storage,
                        round_id,
                        vec![VALIDATOR_1.to_string(), VALIDATOR_2.to_string()],
                    );
                assert!(res.is_ok());
            }),
            grpc_query: denom_trace_grpc_query_mock(
                "transfer/channel-0".to_string(),
                HashMap::from([
                    (IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string()),
                ]),
            ),
        },
    ];

    for (i, test_case) in test_cases.into_iter().enumerate() {
        println!("running test case: {}", test_case.description);

        let mut deps = mock_dependencies(test_case.grpc_query);

        let mut env = mock_env();

        let constants = get_default_constants();
        crate::state::CONSTANTS
            .save(&mut deps.storage, &constants)
            .unwrap();

        env.block.time = Timestamp::from_seconds(0);

        (test_case.setup)(&mut deps.storage, &mut env);

        let result = validate_denom(
            deps.as_ref(),
            env.clone(),
            &constants,
            test_case.denom.clone(),
        );

        assert_eq!(
            result, test_case.expected_result,
            "Test case {} failed: expected {:?}, got {:?}",
            i, test_case.expected_result, result
        );
    }
}

struct LockMultipleDenomTestCases {
    description: &'static str,
    validators: Vec<&'static str>,
    funds: Vec<Coin>,
    lock_duration: u64,
    grpc_query: Box<GrpcQueryFunc>,
    expected_error_msg: String,
}

#[test]
fn lock_tokens_with_multiple_denoms() {
    let test_cases = vec![
        LockMultipleDenomTestCases {
            description:
                "Lock two different denoms, both from validators that are set as validators",
            validators: vec![VALIDATOR_1, VALIDATOR_2],
            funds: vec![
                Coin::new(1000u64, IBC_DENOM_1.to_string()),
                Coin::new(2000u64, IBC_DENOM_2.to_string()),
            ],
            lock_duration: ONE_MONTH_IN_NANO_SECONDS,
            grpc_query: denom_trace_grpc_query_mock(
                "transfer/channel-0".to_string(),
                HashMap::from([
                    (IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string()),
                    (IBC_DENOM_2.to_string(), VALIDATOR_2_LST_DENOM_1.to_string()),
                ]),
            ),
            expected_error_msg: "".to_string(),
        },
        LockMultipleDenomTestCases {
            description: "Lock a denom that is not from a validator that is currently in the set",
            validators: vec![VALIDATOR_1],
            funds: vec![Coin::new(1000u64, IBC_DENOM_3.to_string())],
            lock_duration: ONE_MONTH_IN_NANO_SECONDS,
            grpc_query: denom_trace_grpc_query_mock(
                "transfer/channel-0".to_string(),
                HashMap::from([(IBC_DENOM_3.to_string(), VALIDATOR_3_LST_DENOM_1.to_string())]),
            ),
            expected_error_msg: "is not present".to_string(),
        },
    ];

    for case in test_cases {
        let (mut deps, env) = (mock_dependencies(case.grpc_query), mock_env());
        let info = get_message_info(&deps.api, "addr0001", &[]);
        let msg = get_default_instantiate_msg(&deps.api);

        let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
        assert!(
            res.is_ok(),
            "Failed to instantiate for case: {}",
            case.description
        );

        set_validators_constant_power_ratios_for_rounds(
            deps.as_mut(),
            0,
            100,
            // convert to String
            case.validators.iter().map(|&v| v.to_string()).collect(),
            // each validator gets power ratio 1
            case.validators.iter().map(|_| Decimal::one()).collect(),
        );

        for fund in case.funds.iter() {
            let info = get_message_info(&deps.api, "addr0001", &[fund.clone()]);
            let msg = ExecuteMsg::LockTokens {
                lock_duration: case.lock_duration,
            };
            let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);

            match case.expected_error_msg {
                ref msg if msg.is_empty() => assert!(
                    res.is_ok(),
                    "Failed to lock tokens for case: {}",
                    case.description
                ),
                ref msg => assert!(
                    res.as_ref().err().unwrap().to_string().contains(msg),
                    "for test case {}, expected error message to contain '{}', got '{}'",
                    case.description,
                    msg,
                    res.as_ref().err().unwrap(),
                ),
            }
        }
    }
}

#[test]
fn unlock_tokens_multiple_denoms() {
    let user_address = "addr0000";
    let user_token1 = Coin::new(1000u64, IBC_DENOM_1.to_string());
    let user_token2 = Coin::new(2000u64, IBC_DENOM_2.to_string());

    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([
            (IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string()),
            (IBC_DENOM_2.to_string(), VALIDATOR_2_LST_DENOM_1.to_string()),
        ]),
    );

    let (mut deps, mut env) = (mock_dependencies(grpc_query), mock_env());
    let mut info = get_message_info(
        &deps.api,
        user_address,
        &[user_token1.clone(), user_token2.clone()],
    );
    let msg = get_default_instantiate_msg(&deps.api);

    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok(), "instantiating contract: {:?}", res);

    set_validators_constant_power_ratios_for_rounds(
        deps.as_mut(),
        0,
        100,
        vec![VALIDATOR_1.to_string(), VALIDATOR_2.to_string()],
        vec![Decimal::one(), Decimal::one()],
    );

    info.funds = vec![user_token1.clone()];

    // lock tokens from validator1
    let msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok(), "locking tokens: {:?}", res);

    info.funds = vec![user_token2.clone()];

    // lock tokens from validator2
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok(), "locking tokens: {:?}", res);

    // advance the chain by one month + 1 nano second and check that user can unlock tokens
    env.block.time = env.block.time.plus_nanos(ONE_MONTH_IN_NANO_SECONDS + 1);

    set_default_validator_for_rounds(deps.as_mut(), 0, 100);

    let res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::UnlockTokens {},
    );
    assert!(res.is_ok(), "unlocking tokens: {:?}", res);

    let res = res.unwrap();
    assert_eq!(2, res.messages.len());

    // check that all messages are BankMsg::Send
    for msg in res.messages.iter() {
        match msg.msg.clone() {
            CosmosMsg::Bank(bank_msg) => match bank_msg {
                BankMsg::Send { to_address, amount } => {
                    assert_eq!(info.sender.to_string(), *to_address);
                    assert_eq!(1, amount.len());
                    if amount[0].denom == user_token1.denom {
                        assert_eq!(user_token1.amount.u128(), amount[0].amount.u128());
                    } else if amount[0].denom == user_token2.denom {
                        assert_eq!(user_token2.amount.u128(), amount[0].amount.u128());
                    } else {
                        panic!("unexpected denom");
                    }
                }
                _ => panic!("expected BankMsg::Send message"),
            },
            _ => panic!("expected CosmosMsg::Bank msg"),
        }
    }
}

#[test]
fn unlock_tokens_multiple_users() {
    let user1_address = "addr0001";
    let user2_address = "addr0002";
    let user1_token = Coin::new(1000u64, IBC_DENOM_1.to_string());
    let user2_token = Coin::new(2000u64, IBC_DENOM_1.to_string());

    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
    );

    let (mut deps, mut env) = (mock_dependencies(grpc_query), mock_env());
    let info1 = get_message_info(&deps.api, user1_address, &[user1_token.clone()]);
    let info2 = get_message_info(&deps.api, user2_address, &[user2_token.clone()]);
    let msg = get_default_instantiate_msg(&deps.api);

    let res = instantiate(deps.as_mut(), env.clone(), info1.clone(), msg.clone());
    assert!(res.is_ok(), "instantiating contract: {:?}", res);

    set_validators_constant_power_ratios_for_rounds(
        deps.as_mut(),
        0,
        100,
        vec![VALIDATOR_1.to_string()],
        vec![Decimal::one()],
    );

    // user1 locks tokens
    let msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
    };
    let res = execute(deps.as_mut(), env.clone(), info1.clone(), msg.clone());
    assert!(res.is_ok(), "locking tokens: {:?}", res);

    // user2 locks tokens
    let res = execute(deps.as_mut(), env.clone(), info2.clone(), msg);
    assert!(res.is_ok(), "locking tokens: {:?}", res);

    // advance the chain by one month + 1 nano second and check that users can unlock tokens
    env.block.time = env.block.time.plus_nanos(ONE_MONTH_IN_NANO_SECONDS + 1);

    // user1 unlocks tokens
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info1.clone(),
        ExecuteMsg::UnlockTokens {},
    );
    assert!(res.is_ok(), "unlocking tokens: {:?}", res);

    let res = res.unwrap();
    assert_eq!(1, res.messages.len());

    // check that the message is BankMsg::Send
    match res.messages[0].msg.clone() {
        CosmosMsg::Bank(bank_msg) => match bank_msg {
            BankMsg::Send { to_address, amount } => {
                assert_eq!(info1.sender.to_string(), *to_address);
                assert_eq!(1, amount.len());
                assert_eq!(user1_token.denom, amount[0].denom);
                assert_eq!(user1_token.amount.u128(), amount[0].amount.u128());
            }
            _ => panic!("expected BankMsg::Send message"),
        },
        _ => panic!("expected CosmosMsg::Bank msg"),
    }

    // user2 unlocks tokens
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info2.clone(),
        ExecuteMsg::UnlockTokens {},
    );
    assert!(res.is_ok());

    let res = res.unwrap();
    assert_eq!(1, res.messages.len());

    // check that the message is BankMsg::Send
    match res.messages[0].msg.clone() {
        CosmosMsg::Bank(bank_msg) => match bank_msg {
            BankMsg::Send { to_address, amount } => {
                assert_eq!(info2.sender.to_string(), *to_address);
                assert_eq!(1, amount.len());
                assert_eq!(user2_token.denom, amount[0].denom);
                assert_eq!(user2_token.amount.u128(), amount[0].amount.u128());
            }
            _ => panic!("expected BankMsg::Send message"),
        },
        _ => panic!("expected CosmosMsg::Bank msg"),
    }
}

#[test]
fn lock_tokens_multiple_validators_and_vote() {
    let user_address = "addr0000";
    let user_token1 = Coin::new(1000u64, IBC_DENOM_1.to_string());
    let user_token2 = Coin::new(2000u64, IBC_DENOM_2.to_string());
    let user_token3 = Coin::new(3000u64, IBC_DENOM_3.to_string());

    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([
            (IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string()),
            (IBC_DENOM_2.to_string(), VALIDATOR_2_LST_DENOM_1.to_string()),
            (IBC_DENOM_3.to_string(), VALIDATOR_3_LST_DENOM_1.to_string()),
        ]),
    );

    let (mut deps, env) = (mock_dependencies(grpc_query), mock_env());
    let mut info = get_message_info(
        &deps.api,
        user_address,
        &[
            user_token1.clone(),
            user_token2.clone(),
            user_token3.clone(),
        ],
    );
    let msg = get_default_instantiate_msg(&deps.api);

    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok(), "instantiating contract: {:?}", res);

    set_validators_constant_power_ratios_for_rounds(
        deps.as_mut(),
        0,
        100,
        vec![
            VALIDATOR_1.to_string(),
            VALIDATOR_2.to_string(),
            VALIDATOR_3.to_string(),
        ],
        vec![Decimal::one(), Decimal::percent(95), Decimal::percent(60)],
    );

    // Lock tokens from validator1
    info.funds = vec![user_token1.clone()];
    let msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok(), "locking tokens: {:?}", res);

    // Lock tokens from validator2
    info.funds = vec![user_token2.clone()];
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok(), "locking tokens: {:?}", res);

    // Lock tokens from validator3
    info.funds = vec![user_token3.clone()];
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok(), "locking tokens: {:?}", res);

    // create two proposals
    let msg1 = ExecuteMsg::CreateProposal {
        round_id: None,
        tranche_id: 1,
        title: "proposal title 1".to_string(),
        description: "proposal description 1".to_string(),
        minimum_atom_liquidity_request: Uint128::zero(),
        deployment_duration: 1,
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg1.clone());
    assert!(res.is_ok());

    let msg2 = ExecuteMsg::CreateProposal {
        round_id: None,
        tranche_id: 1,
        title: "proposal title 2".to_string(),
        description: "proposal description 2".to_string(),
        minimum_atom_liquidity_request: Uint128::zero(),
        deployment_duration: 1,
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg2.clone());
    assert!(res.is_ok());

    // User votes on the first proposal
    let msg = ExecuteMsg::Vote {
        tranche_id: 1,
        proposals_votes: vec![ProposalToLockups {
            proposal_id: 0,
            lock_ids: vec![0, 1, 2],
        }],
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok());

    // check proposals
    {
        // Check the proposal scores
        let proposals = query_round_tranche_proposals(deps.as_ref(), 0, 1, 0, 100);

        // unwrap the proposals
        let proposals = proposals.unwrap();

        // check that the first proposal is proposal 0, and that it has
        // power 1000 * 1 + 2000 * 0.95 + 3000 * 0.6 = 4700
        assert_eq!(2, proposals.proposals.len());
        let first_prop = &proposals.proposals[0];
        let second_prop = &proposals.proposals[1];

        assert_eq!(0, first_prop.proposal_id);
        assert_eq!(4700, first_prop.power.u128());

        assert_eq!(1, second_prop.proposal_id);
        assert_eq!(0, second_prop.power.u128());
    }

    // check the total round power
    {
        let total_power = get_total_power_for_round(deps.as_ref(), 0);
        assert!(total_power.is_ok());
        assert_eq!(Uint128::new(4700), total_power.unwrap().to_uint_floor());
    }

    // update the power ratio for validator 1 to become 0.5
    set_validator_power_ratio(deps.as_mut().storage, 0, VALIDATOR_1, Decimal::percent(50));

    // Check the proposal scores
    {
        let proposals = query_top_n_proposals(deps.as_ref(), 0, 1, 2);

        // unwrap the proposals
        let proposals = proposals.unwrap();

        // check that the first proposal is proposal 0, and that it has
        // power 1000 * 0.5 + 2000 * 0.95 + 3000 * 0.6 = 4200
        assert_eq!(2, proposals.proposals.len());
        let first_prop = &proposals.proposals[0];

        assert_eq!(0, first_prop.proposal_id);
        assert_eq!(4200, first_prop.power.u128());

        let second_prop = &proposals.proposals[1];
        assert_eq!(1, second_prop.proposal_id);
        assert_eq!(0, second_prop.power.u128());
    }

    // check the new total power
    {
        let total_power = get_total_power_for_round(deps.as_ref(), 0);
        assert!(total_power.is_ok());
        assert_eq!(Uint128::new(4200), total_power.unwrap().to_uint_floor());
    }
}

struct ValidatorSetInitializationTestCase {
    description: String,
    message: ExecuteMsg,
}

// Checks that the validator stores for rounds are initialized correctly
// when the contract is instantiated and when certain messages are executed.
#[test]
fn validator_set_initialization_test() {
    let test_cases = vec![
        ValidatorSetInitializationTestCase {
            description: "Lock tokens".to_string(),
            message: ExecuteMsg::LockTokens {
                lock_duration: ONE_MONTH_IN_NANO_SECONDS,
            },
        },
        ValidatorSetInitializationTestCase {
            description: "Create proposal".to_string(),
            message: ExecuteMsg::CreateProposal {
                round_id: None,
                tranche_id: 1,
                title: "proposal title".to_string(),
                description: "proposal description".to_string(),
                minimum_atom_liquidity_request: Uint128::zero(),
                deployment_duration: 1,
            },
        },
        ValidatorSetInitializationTestCase {
            description: "Refresh lock".to_string(),
            message: ExecuteMsg::RefreshLockDuration {
                lock_duration: ONE_MONTH_IN_NANO_SECONDS,
                lock_ids: vec![0],
            },
        },
    ];

    for test_case in test_cases {
        println!("Running test case: {}", test_case.description);

        let grpc_query = denom_trace_grpc_query_mock(
            "transfer/channel-0".to_string(),
            HashMap::from([
                (IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string()),
                (IBC_DENOM_2.to_string(), VALIDATOR_2_LST_DENOM_1.to_string()),
                (IBC_DENOM_3.to_string(), VALIDATOR_3_LST_DENOM_1.to_string()),
            ]),
        );

        let (mut deps, mut env) = (mock_dependencies(grpc_query), mock_env());
        let info = get_message_info(
            &deps.api,
            "addr0000",
            &[Coin::new(1000u64, IBC_DENOM_1.to_string())],
        );
        let instantiate_msg = get_default_instantiate_msg(&deps.api);

        // Initialize the contract
        let res = instantiate(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            instantiate_msg.clone(),
        );
        assert!(res.is_ok());

        // Set the validator set storage for round 0
        let validators = vec![
            ValidatorInfo {
                address: VALIDATOR_1.to_string(),
                delegated_tokens: Uint128::new(1000),
                power_ratio: Decimal::percent(50),
            },
            ValidatorInfo {
                address: VALIDATOR_2.to_string(),
                delegated_tokens: Uint128::new(2000),
                power_ratio: Decimal::percent(95),
            },
        ];

        for validator in validators.clone() {
            VALIDATORS_INFO
                .save(
                    deps.as_mut().storage,
                    (0, validator.address.clone()),
                    &validator,
                )
                .unwrap();
            VALIDATORS_PER_ROUND
                .save(
                    deps.as_mut().storage,
                    (
                        0,
                        validator.delegated_tokens.u128(),
                        validator.address.clone(),
                    ),
                    &validator.address,
                )
                .unwrap();
        }

        // create a proposal that can be voted on
        let msg = ExecuteMsg::CreateProposal {
            round_id: None,
            tranche_id: 1,
            title: "proposal title".to_string(),
            description: "proposal description".to_string(),
            minimum_atom_liquidity_request: Uint128::zero(),
            deployment_duration: 1,
        };

        let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone());

        // Check that the proposal was created successfully
        assert!(res.is_ok());

        // lock tokens in round 1 so that we can refresh a lock with a message
        let msg = ExecuteMsg::LockTokens {
            lock_duration: ONE_MONTH_IN_NANO_SECONDS,
        };
        let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone());

        // Check that the lock was successful
        assert!(res.is_ok());

        // Advance the time to round 3
        env.block.time = env
            .block
            .time
            .plus_nanos(instantiate_msg.round_length * 2 + 1);

        // Execute the message
        let res = execute(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            test_case.message.clone(),
        );
        assert!(res.is_ok(), "Failed to execute message: {:?}", res);

        // Check that the validator set storage is correctly initialized for round 1
        for round in 0..=2 {
            let is_initialized = VALIDATORS_STORE_INITIALIZED
                .load(deps.as_ref().storage, round)
                .unwrap();
            assert!(is_initialized);

            for validator in validators.clone() {
                let stored_validator_info = VALIDATORS_INFO
                    .load(deps.as_ref().storage, (round, validator.address.clone()))
                    .unwrap();
                assert_eq!(validator, stored_validator_info);

                let stored_validator_address = VALIDATORS_PER_ROUND
                    .load(
                        deps.as_ref().storage,
                        (
                            round,
                            validator.delegated_tokens.u128(),
                            validator.address.clone(),
                        ),
                    )
                    .unwrap();
                assert_eq!(validator.address, stored_validator_address);
            }
        }
    }
}

// An extra test case to make sure that the validator store is initialized correctly
// when the result of an interchain query comes in.
// Since this is not an execute msg, it is a bit simpler to do this in a separate test case.
#[test]
fn icq_validator_set_initialization_test() {
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([
            (IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string()),
            (IBC_DENOM_2.to_string(), VALIDATOR_2_LST_DENOM_1.to_string()),
            (IBC_DENOM_3.to_string(), VALIDATOR_3_LST_DENOM_1.to_string()),
        ]),
    );

    let (mut deps, mut env) = (mock_dependencies(grpc_query), mock_env());
    let info = get_message_info(
        &deps.api,
        "addr0000",
        &[Coin::new(1000u64, IBC_DENOM_1.to_string())],
    );
    let instantiate_msg = get_default_instantiate_msg(&deps.api);

    // Initialize the contract
    let res = instantiate(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        instantiate_msg.clone(),
    );
    assert!(res.is_ok());

    // Set the validator set storage for round 0
    let validators = vec![
        ValidatorInfo {
            address: VALIDATOR_1.to_string(),
            delegated_tokens: Uint128::new(1000),
            power_ratio: Decimal::percent(50),
        },
        ValidatorInfo {
            address: VALIDATOR_2.to_string(),
            delegated_tokens: Uint128::new(2000),
            power_ratio: Decimal::percent(95),
        },
    ];

    for validator in validators.clone() {
        VALIDATORS_INFO
            .save(
                deps.as_mut().storage,
                (0, validator.address.clone()),
                &validator,
            )
            .unwrap();
        VALIDATORS_PER_ROUND
            .save(
                deps.as_mut().storage,
                (
                    0,
                    validator.delegated_tokens.u128(),
                    validator.address.clone(),
                ),
                &validator.address,
            )
            .unwrap();
    }

    // Mock data for the interchain query result
    let mock_tokens = Uint128::new(1000);
    let mock_shares = Uint128::new(2000) * TOKENS_TO_SHARES_MULTIPLIER;
    let mock_validator = get_mock_validator(VALIDATOR_1, mock_tokens, mock_shares);
    let mock_data = HashMap::from([(
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
    )]);

    deps.querier = deps
        .querier
        .with_custom_handler(custom_interchain_query_mock(mock_data));

    // Advance the time to round 3
    env.block.time = env
        .block
        .time
        .plus_nanos(instantiate_msg.round_length * 2 + 1);

    // Send the SudoMsg as a result of the interchain query
    let msg = SudoMsg::KVQueryResult { query_id: 1 };
    let res = sudo(deps.as_mut(), env.clone(), msg);
    assert!(res.is_ok(), "Failed to execute message: {:?}", res);

    // Check that the validator set storage is correctly initialized for rounds 0, 1, and 2
    for round in 0..=2 {
        let is_initialized = VALIDATORS_STORE_INITIALIZED
            .load(deps.as_ref().storage, round)
            .unwrap();
        assert!(is_initialized);

        for validator in validators.clone() {
            let stored_validator_info = VALIDATORS_INFO
                .load(deps.as_ref().storage, (round, validator.address.clone()))
                .unwrap();
            assert_eq!(validator, stored_validator_info);

            let stored_validator_address = VALIDATORS_PER_ROUND
                .load(
                    deps.as_ref().storage,
                    (
                        round,
                        validator.delegated_tokens.u128(),
                        validator.address.clone(),
                    ),
                )
                .unwrap();
            assert_eq!(validator.address, stored_validator_address);
        }
    }
}
