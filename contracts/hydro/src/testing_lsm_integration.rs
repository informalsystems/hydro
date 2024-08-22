use std::collections::HashMap;

use cosmwasm_std::{
    testing::mock_env, BankMsg, Coin, CosmosMsg, Decimal, Env, StdError, Storage, SystemError,
    SystemResult, Timestamp,
};
use ibc_proto::ibc::apps::transfer::v1::QueryDenomTraceResponse;
use prost::Message;

use crate::{
    contract::{execute, instantiate, query_top_n_proposals},
    lsm_integration::{set_current_validators, validate_denom, VALIDATORS_PER_ROUND},
    msg::ExecuteMsg,
    testing::{
        get_default_instantiate_msg, get_message_info, set_default_validator_for_rounds,
        set_validators_constant_power_ratios_for_rounds, IBC_DENOM_1, IBC_DENOM_2, IBC_DENOM_3,
        ONE_DAY_IN_NANO_SECONDS, ONE_MONTH_IN_NANO_SECONDS, VALIDATOR_1, VALIDATOR_1_LST_DENOM_1,
        VALIDATOR_2, VALIDATOR_2_LST_DENOM_1, VALIDATOR_3, VALIDATOR_3_LST_DENOM_1,
    },
    testing_mocks::{
        denom_trace_grpc_query_mock, grpc_query_result_from, mock_dependencies,
        no_op_grpc_query_mock, GrpcQueryFunc,
    },
};

fn get_default_constants() -> crate::state::Constants {
    crate::state::Constants {
        round_length: ONE_DAY_IN_NANO_SECONDS,
        lock_epoch_length: 1,
        first_round_start: Timestamp::from_seconds(0),
        max_locked_tokens: 1,
        paused: false,
        max_validator_shares_participating: 2,
        hub_transfer_channel_id: "channel-0".to_string(),
    }
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
            expected_result: Err(StdError::generic_err("Querier system error: Unknown system error")),
            setup: Box::new(|_storage, _env| { }),
            grpc_query: Box::new(|_query| { SystemResult::Err(SystemError::Unknown {}) }),
        },
        TestCase {
            description: "gRPC fails to provide denom trace information".to_string(),
            denom: IBC_DENOM_1.to_string(),
            expected_result: Err(StdError::generic_err("Failed to obtain IBC denom trace")),
            setup: Box::new(|_storage, _env| { }),
            grpc_query: Box::new(|_query| { grpc_query_result_from(QueryDenomTraceResponse { denom_trace: None }.encode_to_vec()) }),
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
                let round_id = 0;
                VALIDATORS_PER_ROUND.save(storage, round_id, &vec![VALIDATOR_2.to_string(), VALIDATOR_3.to_string()]).unwrap();
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
                VALIDATORS_PER_ROUND.save(storage, 0, &vec![VALIDATOR_1.to_string(), VALIDATOR_2.to_string()]).unwrap();
                VALIDATORS_PER_ROUND.save(storage, 1, &vec![VALIDATOR_2.to_string(), VALIDATOR_3.to_string()]).unwrap();

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
                VALIDATORS_PER_ROUND
                    .save(
                        storage,
                        round_id,
                        &vec![VALIDATOR_1.to_string(), VALIDATOR_2.to_string()],
                    )
                    .unwrap();
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
        vec!["validator1".to_string(), "validator2".to_string()],
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
        vec!["validator1".to_string()],
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

    let (mut deps, mut env) = (mock_dependencies(grpc_query), mock_env());
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
            "validator1".to_string(),
            "validator2".to_string(),
            "validator3".to_string(),
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
        tranche_id: 1,
        title: "proposal title 1".to_string(),
        description: "proposal description 1".to_string(),
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg1.clone());
    assert!(res.is_ok());

    let msg2 = ExecuteMsg::CreateProposal {
        tranche_id: 1,
        title: "proposal title 2".to_string(),
        description: "proposal description 2".to_string(),
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg2.clone());
    assert!(res.is_ok());

    // User votes on the first proposal
    let msg = ExecuteMsg::Vote {
        tranche_id: 1,
        proposal_id: 0,
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok());

    // Check the proposal scores
    let proposals = query_top_n_proposals(deps.as_ref(), 0, 1, 2);

    // unwrap the proposals
    let proposals = proposals.unwrap();

    // check that the first proposal is proposal 0, and that it has
    // power 100 * 1 + 200 * 0.95 + 300 * 0.6 = 590
    assert_eq!(2, proposals.proposals.len());
    let first_prop = &proposals.proposals[0];
    let second_prop = &proposals.proposals[1];

    assert_eq!(0, first_prop.proposal_id);
    assert_eq!(590, first_prop.power.u128());

    assert_eq!(1, second_prop.proposal_id);
    assert_eq!(0, second_prop.power.u128());
}
