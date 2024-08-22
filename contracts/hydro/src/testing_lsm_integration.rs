use cosmwasm_std::{
    testing::{mock_dependencies, mock_env},
    BankMsg, Coin, CosmosMsg, Decimal, Env, StdError, Storage, Timestamp,
};

use crate::{
    contract::{execute, instantiate, query_top_n_proposals},
    lsm_integration::{set_current_validators, validate_denom, VALIDATORS_PER_ROUND},
    msg::ExecuteMsg,
    testing::{
        get_default_instantiate_msg, get_message_info, set_default_validator_for_rounds,
        set_validators_constant_power_ratios_for_rounds, ONE_DAY_IN_NANO_SECONDS,
        ONE_MONTH_IN_NANO_SECONDS,
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
    }
}

#[test]
fn test_validate_denom() {
    type SetupFunc = dyn Fn(&mut dyn Storage, &mut Env);

    struct TestCase {
        denom: String,
        expected_result: Result<String, StdError>,
        setup: Box<SetupFunc>,
    }

    let test_cases = vec![
            TestCase {
                denom: "invalid_denom".to_string(),
                expected_result: Err(StdError::generic_err(
                    "Invalid denom invalid_denom: not an LSM tokenized share",
                )),
                setup: Box::new(|storage, _env| {
                    let round_id = 0;
                    VALIDATORS_PER_ROUND
                        .save(
                            storage,
                            round_id,
                            &vec!["validator2".to_string(), "validator3".to_string()],
                        )
                        .unwrap();
                }),
            },
            TestCase {
                denom: "validator1/record_id".to_string(),
                expected_result: Err(StdError::generic_err("Validator validator1 is not present; possibly they are not part of the top 2 validators by delegated tokens")),
                setup: Box::new(|storage, _env| {
                    let round_id = 0;
                    VALIDATORS_PER_ROUND.save(storage, round_id, &vec!["validator2".to_string(), "validator3".to_string()]).unwrap();
                }),
            },
            TestCase {
                denom: "validator1/record_id".to_string(),
                expected_result: Err(StdError::generic_err("Validator validator1 is not present; possibly they are not part of the top 2 validators by delegated tokens")),
                setup: Box::new(|storage, env| {
                    let round_id = 1;
                    VALIDATORS_PER_ROUND.save(storage, round_id - 1, &vec!["validator1".to_string(), "validator2".to_string()]).unwrap();
                    VALIDATORS_PER_ROUND.save(storage, round_id, &vec!["validator2".to_string(), "validator3".to_string()]).unwrap();

                    env.block.time = Timestamp::from_nanos(ONE_DAY_IN_NANO_SECONDS+1);
                }),
            },
            TestCase {
                denom: "validator1/record_id".to_string(),
                expected_result: Ok("validator1".to_string()),
                setup: Box::new(|storage, _env| {
                    let constants = get_default_constants();
                    crate::state::CONSTANTS.save(storage, &constants).unwrap();
                    let round_id = 0;
                    VALIDATORS_PER_ROUND
                        .save(
                            storage,
                            round_id,
                            &vec!["validator1".to_string(), "validator2".to_string()],
                        )
                        .unwrap();
                }),
            },
        ];

    for (i, test_case) in test_cases.into_iter().enumerate() {
        let mut deps = mock_dependencies();
        let mut env = mock_env();

        let constants = get_default_constants();
        crate::state::CONSTANTS
            .save(&mut deps.storage, &constants)
            .unwrap();

        env.block.time = Timestamp::from_seconds(0);

        (test_case.setup)(&mut deps.storage, &mut env);

        let result = validate_denom(deps.as_ref(), env.clone(), test_case.denom.clone());

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
    expected_error_msg: String,
}

#[test]
fn lock_tokens_with_multiple_denoms() {
    let test_cases = vec![
        LockMultipleDenomTestCases {
            description:
                "Lock two different denoms, both from validators that are set as validators",
            validators: vec!["validator1", "validator2"],
            funds: vec![
                Coin::new(1000u64, "validator1/record_id".to_string()),
                Coin::new(2000u64, "validator2/record_id".to_string()),
            ],
            lock_duration: ONE_MONTH_IN_NANO_SECONDS,
            expected_error_msg: "".to_string(),
        },
        LockMultipleDenomTestCases {
            description: "Lock a denom that is not from a validator that is currently in the set",
            validators: vec!["validator1"],
            funds: vec![Coin::new(1000u64, "validator3/record_id".to_string())],
            lock_duration: ONE_MONTH_IN_NANO_SECONDS,
            expected_error_msg: "is not present".to_string(),
        },
    ];

    for case in test_cases {
        let (mut deps, env) = (mock_dependencies(), mock_env());
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
    let user_token1 = Coin::new(1000u64, "validator1/record_id".to_string());
    let user_token2 = Coin::new(2000u64, "validator2/record_id".to_string());

    let (mut deps, mut env) = (mock_dependencies(), mock_env());
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
    let user1_token = Coin::new(1000u64, "validator1/record_id".to_string());
    let user2_token = Coin::new(2000u64, "validator1/record_id".to_string());

    let (mut deps, mut env) = (mock_dependencies(), mock_env());
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
    let user_token1 = Coin::new(1000u64, "validator1/record_id".to_string());
    let user_token2 = Coin::new(2000u64, "validator2/record_id".to_string());
    let user_token3 = Coin::new(3000u64, "validator3/record_id".to_string());

    let (mut deps, mut env) = (mock_dependencies(), mock_env());
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
