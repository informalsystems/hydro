use std::{collections::HashMap, str::FromStr};

use cosmos_sdk_proto::prost::Message;
use cosmwasm_std::{
    testing::{mock_env, MockApi, MockStorage},
    BankMsg, Coin, CosmosMsg, Decimal, Env, OwnedDeps, StdError, SystemError, SystemResult,
    Timestamp, Uint128,
};
use interface::hydro::TokenGroupRatioChange;
use neutron_sdk::bindings::query::NeutronQuery;
use neutron_std::types::ibc::applications::transfer::v1::QueryDenomTraceResponse;

use crate::{
    contract::{
        compute_current_round_id, execute, instantiate, query_round_tranche_proposals,
        query_top_n_proposals,
    },
    msg::{ExecuteMsg, ProposalToLockups},
    score_keeper::get_total_power_for_round,
    testing::{
        get_default_cw721_collection_info, get_default_instantiate_msg, get_default_power_schedule,
        get_message_info, setup_lsm_token_info_provider_mock, IBC_DENOM_1, IBC_DENOM_2,
        IBC_DENOM_3, LSM_TOKEN_PROVIDER_ADDR, ONE_DAY_IN_NANO_SECONDS, ONE_MONTH_IN_NANO_SECONDS,
        VALIDATOR_1, VALIDATOR_1_LST_DENOM_1, VALIDATOR_2, VALIDATOR_2_LST_DENOM_1, VALIDATOR_3,
        VALIDATOR_3_LST_DENOM_1,
    },
    testing_mocks::{
        denom_trace_grpc_query_mock, mock_dependencies, no_op_grpc_query_mock,
        system_result_ok_from, GrpcQueryFunc, MockQuerier,
    },
    token_manager::TokenManager,
};

pub fn get_default_constants() -> crate::state::Constants {
    crate::state::Constants {
        round_length: ONE_DAY_IN_NANO_SECONDS,
        lock_epoch_length: 1,
        first_round_start: Timestamp::from_seconds(0),
        max_locked_tokens: 1,
        known_users_cap: 0,
        paused: false,
        max_deployment_duration: 12,
        round_lock_power_schedule: get_default_power_schedule(),
        cw721_collection_info: get_default_cw721_collection_info(),
        lock_depth_limit: 50,
        lock_expiry_duration_seconds: 60 * 60 * 24 * 30 * 6, // 6 months
        slash_percentage_threshold: Decimal::from_str("0.5").unwrap(),
        slash_tokens_receiver_addr: String::new(),
    }
}

#[test]
fn test_validate_denom() {
    type SetupFunc =
        dyn Fn(&mut OwnedDeps<MockStorage, MockApi, MockQuerier, NeutronQuery>, &mut Env);

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
            setup: Box::new(|_deps, _env| { }),
            grpc_query: no_op_grpc_query_mock(),
        },
        TestCase {
            description: "gRPC query returns error".to_string(),
            denom: IBC_DENOM_1.to_string(),
            expected_result: Err(StdError::generic_err("Failed to obtain IBC denom trace: Generic error: Querier system error: Unknown system error")),
            setup: Box::new(|_deps, _env| { }),
            grpc_query: Box::new(|_query| { SystemResult::Err(SystemError::Unknown {}) }),
        },
        TestCase {
            description: "gRPC fails to provide denom trace information".to_string(),
            denom: IBC_DENOM_1.to_string(),
            expected_result: Err(StdError::generic_err("Failed to obtain IBC denom trace")),
            setup: Box::new(|_deps, _env| { }),
            grpc_query: Box::new(|_query| { system_result_ok_from(QueryDenomTraceResponse { denom_trace: None }.encode_to_vec()) }),
        },
        TestCase {
            description: "IBC denom received over multiple hops".to_string(),
            denom: IBC_DENOM_1.to_string(),
            expected_result: Err(StdError::generic_err(
                "Only LSTs transferred directly from the Cosmos Hub can be locked.",
            )),
            setup: Box::new(|_deps, _env| {}),
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
            setup: Box::new(|_deps, _env| {}),
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
            setup: Box::new(|_deps, _env| {}),
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
            setup: Box::new(|_deps, _env| {}),
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
            setup: Box::new(|_deps, _env| {}),
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
            setup: Box::new(|_deps, _env| {}),
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
            setup: Box::new(|_deps, _env| {}),
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
            expected_result: Err(StdError::generic_err(format!("Validator {VALIDATOR_1} is not present; possibly they are not part of the top N validators by delegated tokens"))),
            setup: Box::new(|deps, _env| {
                let lsm_token_info_provider_addr = deps.api.addr_make(LSM_TOKEN_PROVIDER_ADDR);
                setup_lsm_token_info_provider_mock(deps, lsm_token_info_provider_addr.clone(),
                    vec![(0, vec![(VALIDATOR_2.to_string(), Decimal::one()), (VALIDATOR_3.to_string(), Decimal::one())])], true);
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
            expected_result: Err(StdError::generic_err(format!("Validator {VALIDATOR_1} is not present; possibly they are not part of the top N validators by delegated tokens"))),
            setup: Box::new(|deps, env| {
                let lsm_token_info_provider_addr = deps.api.addr_make(LSM_TOKEN_PROVIDER_ADDR);
                setup_lsm_token_info_provider_mock(deps, lsm_token_info_provider_addr.clone(),
                    vec![
                        (0, vec![(VALIDATOR_1.to_string(), Decimal::one()), (VALIDATOR_2.to_string(), Decimal::one())]),
                        (1, vec![(VALIDATOR_2.to_string(), Decimal::one()), (VALIDATOR_3.to_string(), Decimal::one())])
                    ], true);

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
            setup: Box::new(|deps, _env| {
                let lsm_token_info_provider_addr = deps.api.addr_make(LSM_TOKEN_PROVIDER_ADDR);
                setup_lsm_token_info_provider_mock(deps, lsm_token_info_provider_addr,
                    vec![(0, vec![(VALIDATOR_1.to_string(), Decimal::one()), (VALIDATOR_2.to_string(), Decimal::one())])], true);
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
            .save(&mut deps.storage, env.block.time.nanos(), &constants)
            .unwrap();

        let lsm_token_info_provider_addr = deps.api.addr_make(LSM_TOKEN_PROVIDER_ADDR);
        setup_lsm_token_info_provider_mock(
            &mut deps,
            lsm_token_info_provider_addr.clone(),
            vec![(0, vec![(VALIDATOR_1.to_string(), Decimal::one())])],
            true,
        );

        env.block.time = Timestamp::from_seconds(0);

        (test_case.setup)(&mut deps, &mut env);

        let mut token_manager = TokenManager::new(&deps.as_ref());
        let result = token_manager.validate_denom(
            &deps.as_ref(),
            compute_current_round_id(&env, &constants).unwrap(),
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

        let lsm_token_info_provider_addr = deps.api.addr_make(LSM_TOKEN_PROVIDER_ADDR);
        setup_lsm_token_info_provider_mock(
            &mut deps,
            lsm_token_info_provider_addr.clone(),
            vec![(
                0,
                case.validators
                    .iter()
                    .map(|val| (val.to_string(), Decimal::one()))
                    .collect(),
            )],
            true,
        );

        for fund in case.funds.iter() {
            let info = get_message_info(&deps.api, "addr0001", std::slice::from_ref(fund));
            let msg = ExecuteMsg::LockTokens {
                lock_duration: case.lock_duration,
                proof: None,
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
    assert!(res.is_ok(), "instantiating contract: {res:?}");

    let lsm_token_info_provider_addr = deps.api.addr_make(LSM_TOKEN_PROVIDER_ADDR);
    setup_lsm_token_info_provider_mock(
        &mut deps,
        lsm_token_info_provider_addr.clone(),
        vec![(
            0,
            vec![
                (VALIDATOR_1.to_string(), Decimal::one()),
                (VALIDATOR_2.to_string(), Decimal::one()),
            ],
        )],
        true,
    );

    info.funds = vec![user_token1.clone()];

    // lock tokens from validator1
    let msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok(), "locking tokens: {res:?}");

    info.funds = vec![user_token2.clone()];

    // lock tokens from validator2
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok(), "locking tokens: {res:?}");

    // advance the chain by one month + 1 nano second and check that user can unlock tokens
    env.block.time = env.block.time.plus_nanos(ONE_MONTH_IN_NANO_SECONDS + 1);

    let res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::UnlockTokens { lock_ids: None },
    );
    assert!(res.is_ok(), "unlocking tokens: {res:?}");

    let res = res.unwrap();
    assert_eq!(1, res.messages.len());

    // check that the message is BankMsg::Send
    match res.messages[0].clone().msg {
        CosmosMsg::Bank(bank_msg) => match bank_msg {
            BankMsg::Send { to_address, amount } => {
                assert_eq!(info.sender.to_string(), *to_address);
                assert_eq!(2, amount.len());

                for amount in amount.iter() {
                    if amount.denom == user_token1.denom {
                        assert_eq!(user_token1.amount.u128(), amount.amount.u128());
                    } else if amount.denom == user_token2.denom {
                        assert_eq!(user_token2.amount.u128(), amount.amount.u128());
                    } else {
                        panic!("unexpected denom");
                    }
                }
            }
            _ => panic!("expected BankMsg::Send message"),
        },
        _ => panic!("expected CosmosMsg::Bank msg"),
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
    let info1 = get_message_info(&deps.api, user1_address, std::slice::from_ref(&user1_token));
    let info2 = get_message_info(&deps.api, user2_address, std::slice::from_ref(&user2_token));
    let msg = get_default_instantiate_msg(&deps.api);

    let res = instantiate(deps.as_mut(), env.clone(), info1.clone(), msg.clone());
    assert!(res.is_ok(), "instantiating contract: {res:?}");

    let lsm_token_info_provider_addr = deps.api.addr_make(LSM_TOKEN_PROVIDER_ADDR);
    setup_lsm_token_info_provider_mock(
        &mut deps,
        lsm_token_info_provider_addr.clone(),
        vec![(0, vec![(VALIDATOR_1.to_string(), Decimal::one())])],
        true,
    );

    // user1 locks tokens
    let msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };
    let res = execute(deps.as_mut(), env.clone(), info1.clone(), msg.clone());
    assert!(res.is_ok(), "locking tokens: {res:?}");

    // user2 locks tokens
    let res = execute(deps.as_mut(), env.clone(), info2.clone(), msg);
    assert!(res.is_ok(), "locking tokens: {res:?}");

    // advance the chain by one month + 1 nano second and check that users can unlock tokens
    env.block.time = env.block.time.plus_nanos(ONE_MONTH_IN_NANO_SECONDS + 1);

    // user1 unlocks tokens
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info1.clone(),
        ExecuteMsg::UnlockTokens { lock_ids: None },
    );
    assert!(res.is_ok(), "unlocking tokens: {res:?}");

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
        ExecuteMsg::UnlockTokens { lock_ids: None },
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
    assert!(res.is_ok(), "instantiating contract: {res:?}");

    let lsm_token_info_provider_addr = deps.api.addr_make(LSM_TOKEN_PROVIDER_ADDR);
    setup_lsm_token_info_provider_mock(
        &mut deps,
        lsm_token_info_provider_addr.clone(),
        vec![(
            0,
            vec![
                (VALIDATOR_1.to_string(), Decimal::one()),
                (VALIDATOR_2.to_string(), Decimal::percent(95)),
                (VALIDATOR_3.to_string(), Decimal::percent(60)),
            ],
        )],
        true,
    );

    // Lock tokens from validator1
    info.funds = vec![user_token1.clone()];
    let msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok(), "locking tokens: {res:?}");

    // Lock tokens from validator2
    info.funds = vec![user_token2.clone()];
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok(), "locking tokens: {res:?}");

    // Lock tokens from validator3
    info.funds = vec![user_token3.clone()];
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok(), "locking tokens: {res:?}");

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
        let total_power = get_total_power_for_round(&deps.as_ref(), 0);
        assert!(total_power.is_ok());
        assert_eq!(Uint128::new(4700), total_power.unwrap().to_uint_floor());
    }

    // update the power ratio for validator 1 to become 0.5
    let new_power_ratio = Decimal::percent(50);
    let lsm_token_info_provider_addr = deps.api.addr_make(LSM_TOKEN_PROVIDER_ADDR);
    setup_lsm_token_info_provider_mock(
        &mut deps,
        lsm_token_info_provider_addr.clone(),
        vec![(0, vec![(VALIDATOR_1.to_string(), new_power_ratio)])],
        true,
    );

    let msg = ExecuteMsg::UpdateTokenGroupsRatios {
        changes: vec![TokenGroupRatioChange {
            token_group_id: VALIDATOR_1.to_string(),
            old_ratio: Decimal::one(),
            new_ratio: new_power_ratio,
        }],
    };

    let lsm_token_provider_msg_info = get_message_info(&deps.api, LSM_TOKEN_PROVIDER_ADDR, &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        lsm_token_provider_msg_info.clone(),
        msg,
    );
    assert!(res.is_ok());

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
        let total_power = get_total_power_for_round(&deps.as_ref(), 0);
        assert!(total_power.is_ok());
        assert_eq!(Uint128::new(4200), total_power.unwrap().to_uint_floor());
    }
}
