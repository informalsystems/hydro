use std::collections::{HashMap, HashSet};

use cosmwasm_std::{testing::mock_env, BankMsg, Coin, CosmosMsg, Decimal, Uint128};

use crate::{
    contract::{execute, instantiate, query_all_user_lockups, MAX_LOCK_ENTRIES},
    msg::{ExecuteMsg, UpdateConfigData},
    state::{LOCKS_PENDING_SLASHES, USER_LOCKS},
    testing::{
        get_address_as_str, get_default_instantiate_msg, get_message_info,
        set_default_validator_for_rounds, setup_st_atom_token_info_provider_mock, IBC_DENOM_1,
        ONE_MONTH_IN_NANO_SECONDS, ST_ATOM_ON_NEUTRON, ST_ATOM_ON_STRIDE,
        THREE_MONTHS_IN_NANO_SECONDS, VALIDATOR_1, VALIDATOR_1_LST_DENOM_1,
    },
    testing_lsm_integration::set_validator_infos_for_round,
    testing_mocks::{denom_trace_grpc_query_mock, mock_dependencies},
};

#[test]
fn lock_tokens_basic_test() {
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
    );

    let user_address = "addr0000";
    let (mut deps, env) = (mock_dependencies(grpc_query), mock_env());
    let info = get_message_info(&deps.api, user_address, &[]);
    let msg = get_default_instantiate_msg(&deps.api);

    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());

    set_default_validator_for_rounds(deps.as_mut(), 0, 100);

    let info1 = get_message_info(
        &deps.api,
        user_address,
        &[Coin::new(1000u64, IBC_DENOM_1.to_string())],
    );
    let msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };
    let res = execute(deps.as_mut(), env.clone(), info1.clone(), msg);
    assert!(res.is_ok(), "error: {res:?}");

    let info2 = get_message_info(
        &deps.api,
        user_address,
        &[Coin::new(3000u64, IBC_DENOM_1.to_string())],
    );
    let msg = ExecuteMsg::LockTokens {
        lock_duration: THREE_MONTHS_IN_NANO_SECONDS,
        proof: None,
    };
    let res = execute(deps.as_mut(), env.clone(), info2.clone(), msg);
    assert!(res.is_ok());

    let res = query_all_user_lockups(&deps.as_ref(), &env, info.sender.to_string(), 0, 2000);
    assert!(res.is_ok());
    let res = res.unwrap();
    assert_eq!(2, res.lockups.len());

    let lockup = &res.lockups[0];
    // check that the id is 0
    assert_eq!(0, lockup.lock_entry.lock_id);
    assert_eq!(
        info1.funds[0].amount.u128(),
        lockup.lock_entry.funds.amount.u128()
    );
    assert_eq!(info1.funds[0].denom, lockup.lock_entry.funds.denom);
    assert_eq!(env.block.time, lockup.lock_entry.lock_start);
    assert_eq!(
        env.block.time.plus_nanos(ONE_MONTH_IN_NANO_SECONDS),
        lockup.lock_entry.lock_end
    );
    // check that the power is correct: 1000 tokens locked for one epoch
    // so power is 1000 * 1
    assert_eq!(1000, lockup.current_voting_power.u128());

    let lockup = &res.lockups[1];
    // check that the id is 1
    assert_eq!(1, lockup.lock_entry.lock_id);
    assert_eq!(
        info2.funds[0].amount.u128(),
        lockup.lock_entry.funds.amount.u128()
    );
    assert_eq!(info2.funds[0].denom, lockup.lock_entry.funds.denom);
    assert_eq!(env.block.time, lockup.lock_entry.lock_start);
    assert_eq!(
        env.block.time.plus_nanos(THREE_MONTHS_IN_NANO_SECONDS),
        lockup.lock_entry.lock_end
    );
    // check that the power is correct: 3000 tokens locked for three epochs
    // so power is 3000 * 1.5 = 4500
    assert_eq!(4500, lockup.current_voting_power.u128());

    // check that the USER_LOCKS are updated as expected
    let expected_lock_ids = HashSet::from([
        res.lockups[0].lock_entry.lock_id,
        res.lockups[1].lock_entry.lock_id,
    ]);
    let mut user_lock_ids = USER_LOCKS
        .load(&deps.storage, info2.sender.clone())
        .unwrap();
    user_lock_ids.retain(|lock_id| !expected_lock_ids.contains(lock_id));
    assert!(user_lock_ids.is_empty());
}

#[test]
fn lock_tokens_various_denoms_test() {
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([
            (IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string()),
            (
                ST_ATOM_ON_NEUTRON.to_string(),
                ST_ATOM_ON_STRIDE.to_string(),
            ),
        ]),
    );

    let user_address = "addr0000";
    let (mut deps, env) = (mock_dependencies(grpc_query), mock_env());
    let info = get_message_info(&deps.api, user_address, &[]);
    let token_info_provider_addr = deps.api.addr_make("token_info_provider_1");
    let msg = get_default_instantiate_msg(&deps.api);

    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());

    set_validator_infos_for_round(&mut deps.storage, 0, vec![VALIDATOR_1.to_string()]).unwrap();
    setup_st_atom_token_info_provider_mock(&mut deps, token_info_provider_addr, Decimal::one());

    // Try to lock some unsupported token and verify this is not possible
    let info1 = get_message_info(
        &deps.api,
        user_address,
        &[Coin::new(1000u64, "untrn".to_string())],
    );
    let msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };
    let res = execute(deps.as_mut(), env.clone(), info1.clone(), msg);
    assert!(res.is_err());
    assert!(res
        .unwrap_err()
        .to_string()
        .to_lowercase()
        .contains("token with denom untrn can not be locked in hydro."));

    let info2 = get_message_info(
        &deps.api,
        user_address,
        &[Coin::new(1000u64, ST_ATOM_ON_NEUTRON.to_string())],
    );
    let msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };
    let res = execute(deps.as_mut(), env.clone(), info2.clone(), msg);
    assert!(res.is_ok(), "error: {res:?}");

    let info3 = get_message_info(
        &deps.api,
        user_address,
        &[Coin::new(3000u64, IBC_DENOM_1.to_string())],
    );
    let msg = ExecuteMsg::LockTokens {
        lock_duration: THREE_MONTHS_IN_NANO_SECONDS,
        proof: None,
    };
    let res = execute(deps.as_mut(), env.clone(), info3.clone(), msg);
    assert!(res.is_ok());

    // Verify both user lockups and their voting power
    let user_lockups =
        query_all_user_lockups(&deps.as_ref(), &env, info.sender.to_string(), 0, 100)
            .unwrap()
            .lockups;

    assert_eq!(user_lockups.len(), 2);

    let lockup = user_lockups[0].clone();
    assert_eq!(lockup.lock_entry.funds.denom.clone(), ST_ATOM_ON_NEUTRON);
    assert_eq!(lockup.current_voting_power, Uint128::new(1000));

    let lockup = user_lockups[1].clone();
    assert_eq!(lockup.lock_entry.funds.denom.clone(), IBC_DENOM_1);
    assert_eq!(lockup.current_voting_power, Uint128::new(4500));
}

#[test]
fn unlock_tokens_basic_test() {
    let user_address = "addr0000";
    let user_token = Coin::new(1000u64, IBC_DENOM_1.to_string());

    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
    );
    let (mut deps, mut env) = (mock_dependencies(grpc_query), mock_env());
    let info = get_message_info(&deps.api, user_address, &[user_token.clone()]);
    let msg = get_default_instantiate_msg(&deps.api);

    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());

    set_default_validator_for_rounds(deps.as_mut(), 0, 100);

    // lock 1000 tokens for one month
    let msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());

    // lock another 1000 tokens for one month
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok());

    // check that user can not unlock tokens immediately
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::UnlockTokens { lock_ids: None },
    );
    assert!(res.is_ok());

    let res = res.unwrap();
    assert_eq!(0, res.messages.len());

    // advance the chain by one month + 1 nano second and check that user can unlock tokens
    env.block.time = env.block.time.plus_nanos(ONE_MONTH_IN_NANO_SECONDS + 1);

    let res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::UnlockTokens { lock_ids: None },
    );
    assert!(res.is_ok());

    let res = res.unwrap();
    assert_eq!(1, res.messages.len());

    // check that all messages are BankMsg::Send
    for msg in res.messages.iter() {
        match msg.msg.clone() {
            CosmosMsg::Bank(bank_msg) => match bank_msg {
                BankMsg::Send { to_address, amount } => {
                    assert_eq!(info.sender.to_string(), *to_address);
                    assert_eq!(1, amount.len());
                    assert_eq!(user_token.denom, amount[0].denom);
                    assert_eq!(user_token.amount.u128() * 2, amount[0].amount.u128());
                }
                _ => panic!("expected BankMsg::Send message"),
            },
            _ => panic!("expected CosmosMsg::Bank msg"),
        }
    }
}

#[test]
fn unlock_tokens_pending_slashes_test() {
    // Use address different from "addr0000" since that one is used to send slashed amounts to it
    let user_address = "addr0001";
    let user_token = Coin::new(1000u64, IBC_DENOM_1.to_string());

    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
    );
    let (mut deps, mut env) = (mock_dependencies(grpc_query), mock_env());
    let info = get_message_info(&deps.api, user_address, &[user_token.clone()]);
    let instantiate_msg = get_default_instantiate_msg(&deps.api);

    let res = instantiate(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        instantiate_msg.clone(),
    );
    assert!(res.is_ok());

    set_default_validator_for_rounds(deps.as_mut(), 0, 100);

    // lock 1000 tokens for one month
    let msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());

    // check that user can not unlock tokens immediately
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::UnlockTokens { lock_ids: None },
    );
    assert!(res.is_ok());

    let res = res.unwrap();
    assert_eq!(0, res.messages.len());

    // Mock a pending slash
    LOCKS_PENDING_SLASHES
        .save(&mut deps.storage, 0, &Uint128::from(70u128))
        .unwrap();

    // advance the chain by one month + 1 nano second and check that user can unlock tokens
    env.block.time = env.block.time.plus_nanos(ONE_MONTH_IN_NANO_SECONDS + 1);

    let res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::UnlockTokens { lock_ids: None },
    );
    assert!(res.is_ok());

    let res = res.unwrap();
    // Should contain 2 BankMsg::Send messages, one for unlocked amount and one for slashed amount
    assert_eq!(2, res.messages.len());

    // check that all messages are BankMsg::Send
    for msg in res.messages.iter() {
        match msg.msg.clone() {
            CosmosMsg::Bank(bank_msg) => match bank_msg {
                BankMsg::Send { to_address, amount } => {
                    let expected_amount: u128 =
                        if to_address == instantiate_msg.slash_tokens_receiver_addr {
                            70
                        } else if to_address == info.sender.to_string() {
                            930
                        } else {
                            panic!("Unexpected recipient address: {to_address}");
                        };

                    assert_eq!(1, amount.len());
                    assert_eq!(amount[0].denom, user_token.denom);
                    assert_eq!(amount[0].amount.u128(), expected_amount);
                }
                _ => panic!("expected BankMsg::Send message"),
            },
            _ => panic!("expected CosmosMsg::Bank msg"),
        }
    }
}

#[test]
fn unlock_specific_tokens_test() {
    let user_address = "addr0000";
    let user_token = Coin::new(1000u64, IBC_DENOM_1.to_string());

    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
    );
    let (mut deps, mut env) = (mock_dependencies(grpc_query), mock_env());
    let info = get_message_info(&deps.api, user_address, &[user_token.clone()]);
    let msg = get_default_instantiate_msg(&deps.api);

    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());

    set_default_validator_for_rounds(deps.as_mut(), 0, 100);

    // Create 4 locks with specific durations
    let durations = [
        ONE_MONTH_IN_NANO_SECONDS,     // Lock 1
        ONE_MONTH_IN_NANO_SECONDS * 2, // Lock 2
        ONE_MONTH_IN_NANO_SECONDS,     // Lock 3
        ONE_MONTH_IN_NANO_SECONDS,     // Lock 4
    ];

    // Store the lock IDs as we create them
    let mut lock_ids = vec![];
    for duration in durations.iter() {
        let msg = ExecuteMsg::LockTokens {
            lock_duration: *duration,
            proof: None,
        };
        let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
        assert!(res.is_ok());

        let lock_id = res
            .unwrap()
            .attributes
            .iter()
            .find(|attr| attr.key == "lock_id")
            .map(|attr| attr.value.parse::<u64>().unwrap())
            .expect("lock_id not found in response");

        lock_ids.push(lock_id);
    }

    // Advance time by one month + 1 nanosecond
    env.block.time = env.block.time.plus_nanos(ONE_MONTH_IN_NANO_SECONDS + 1);

    // First attempt: unlock locks 1 and 4
    let unlock_msg = ExecuteMsg::UnlockTokens {
        lock_ids: Some(vec![lock_ids[0], lock_ids[3]]),
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), unlock_msg);
    assert!(res.is_ok());

    let res = res.unwrap();
    // Should have 1 message that sums up tokens from both locks
    assert_eq!(1, res.messages.len());

    // Verify the first attempt's messages and unlocked IDs
    let unlocked_ids: Vec<u64> = res
        .attributes
        .iter()
        .find(|attr| attr.key == "unlocked_lock_ids")
        .map(|attr| {
            attr.value
                .split(", ")
                .map(|id| id.parse::<u64>().unwrap())
                .collect()
        })
        .expect("unlocked_lock_ids not found in response");

    assert_eq!(unlocked_ids.len(), 2);
    assert!(unlocked_ids.contains(&lock_ids[0]));
    assert!(unlocked_ids.contains(&lock_ids[3]));

    // Verify first attempt's bank messages
    for msg in res.messages.iter() {
        match msg.msg.clone() {
            CosmosMsg::Bank(bank_msg) => match bank_msg {
                BankMsg::Send { to_address, amount } => {
                    assert_eq!(info.sender.to_string(), to_address);
                    assert_eq!(1, amount.len());
                    assert_eq!(user_token.denom, amount[0].denom);
                    assert_eq!(user_token.amount.u128() * 2, amount[0].amount.u128());
                }
                _ => panic!("expected BankMsg::Send message"),
            },
            _ => panic!("expected CosmosMsg::Bank msg"),
        }
    }

    // Second attempt: unlock locks 2 and 3
    let unlock_msg = ExecuteMsg::UnlockTokens {
        lock_ids: Some(vec![lock_ids[1], lock_ids[2]]),
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), unlock_msg);
    assert!(res.is_ok());

    let res = res.unwrap();
    // Should have 1 message (only lock 3 should be unlockable)
    assert_eq!(1, res.messages.len());

    // Verify the second attempt's unlocked IDs
    let unlocked_ids: Vec<u64> = res
        .attributes
        .iter()
        .find(|attr| attr.key == "unlocked_lock_ids")
        .map(|attr| {
            attr.value
                .split(", ")
                .map(|id| id.parse::<u64>().unwrap())
                .collect()
        })
        .expect("unlocked_lock_ids not found in response");

    assert_eq!(unlocked_ids.len(), 1);
    assert!(unlocked_ids.contains(&lock_ids[2]));
    assert!(!unlocked_ids.contains(&lock_ids[1])); // Lock 2 shouldn't be unlocked yet

    // Verify second attempt's bank message
    for msg in res.messages.iter() {
        match msg.msg.clone() {
            CosmosMsg::Bank(bank_msg) => match bank_msg {
                BankMsg::Send { to_address, amount } => {
                    assert_eq!(info.sender.to_string(), to_address);
                    assert_eq!(1, amount.len());
                    assert_eq!(user_token.denom, amount[0].denom);
                    assert_eq!(user_token.amount.u128(), amount[0].amount.u128());
                }
                _ => panic!("expected BankMsg::Send message"),
            },
            _ => panic!("expected CosmosMsg::Bank msg"),
        }
    }

    // Third attempt: try to unlock lock 2 again (should succeed but unlock nothing)
    let unlock_msg = ExecuteMsg::UnlockTokens {
        lock_ids: Some(vec![lock_ids[1]]),
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), unlock_msg);
    assert!(res.is_ok());

    let res = res.unwrap();
    // Should have 0 messages (lock 2 is still not expired)
    assert_eq!(0, res.messages.len());

    // Verify the third attempt's unlocked IDs (should be empty)
    let unlocked_ids = res
        .attributes
        .iter()
        .find(|attr| attr.key == "unlocked_lock_ids")
        .map(|attr| attr.value.trim())
        .expect("unlocked_lock_ids not found in response");

    assert!(unlocked_ids.is_empty());
}

#[test]
fn test_too_many_locks() {
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
    );
    let (mut deps, mut env) = (mock_dependencies(grpc_query), mock_env());
    let info = get_message_info(
        &deps.api,
        "addr0000",
        &[Coin::new(1000u64, IBC_DENOM_1.to_string())],
    );
    let msg = get_default_instantiate_msg(&deps.api);

    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());

    set_default_validator_for_rounds(deps.as_mut(), 0, 100);

    // lock tokens many times
    let lock_msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };
    for i in 0..MAX_LOCK_ENTRIES + 10 {
        let res = execute(deps.as_mut(), env.clone(), info.clone(), lock_msg.clone());
        if i < MAX_LOCK_ENTRIES {
            assert!(res.is_ok());
        } else {
            assert!(res.is_err());
            assert!(res
                .unwrap_err()
                .to_string()
                .contains("User has too many locks"));
        }
    }

    // now test that another user can still lock tokens
    let info2 = get_message_info(
        &deps.api,
        "addr0001",
        &[Coin::new(1000u64, IBC_DENOM_1.to_string())],
    );
    for i in 0..MAX_LOCK_ENTRIES + 10 {
        let res = execute(deps.as_mut(), env.clone(), info2.clone(), lock_msg.clone());
        if i < MAX_LOCK_ENTRIES {
            assert!(res.is_ok());
        } else {
            assert!(res.is_err());
            assert!(res
                .unwrap_err()
                .to_string()
                .contains("User has too many locks"));
        }
    }

    // now test that the first user can unlock tokens after we have passed enough time so that they are unlocked
    env.block.time = env.block.time.plus_nanos(ONE_MONTH_IN_NANO_SECONDS + 1);
    let unlock_msg = ExecuteMsg::UnlockTokens { lock_ids: None };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), unlock_msg.clone());
    assert!(res.is_ok());

    // now the first user can lock tokens again
    for i in 0..MAX_LOCK_ENTRIES + 10 {
        let res = execute(deps.as_mut(), env.clone(), info.clone(), lock_msg.clone());
        if i < MAX_LOCK_ENTRIES {
            assert!(res.is_ok());
        } else {
            assert!(res.is_err());
            assert!(res
                .unwrap_err()
                .to_string()
                .contains("User has too many locks"));
        }
    }
}

#[test]
fn max_locked_tokens_test() {
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
    );
    let (mut deps, mut env) = (mock_dependencies(grpc_query), mock_env());
    let mut info = get_message_info(&deps.api, "addr0000", &[]);

    let mut msg = get_default_instantiate_msg(&deps.api);
    msg.max_locked_tokens = Uint128::new(2000);
    msg.whitelist_admins = vec![get_address_as_str(&deps.api, "addr0001")];

    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());

    set_default_validator_for_rounds(deps.as_mut(), 0, 100);

    // total tokens locked after this action will be 1500
    info = get_message_info(
        &deps.api,
        "addr0000",
        &[Coin::new(1500u64, IBC_DENOM_1.to_string())],
    );
    let mut lock_msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), lock_msg.clone());
    assert!(res.is_ok());

    // total tokens locked after this action would be 3000, which is not allowed
    info = get_message_info(
        &deps.api,
        "addr0000",
        &[Coin::new(1500u64, IBC_DENOM_1.to_string())],
    );
    let res = execute(deps.as_mut(), env.clone(), info.clone(), lock_msg.clone());
    assert!(res.is_err());
    assert!(res
        .unwrap_err()
        .to_string()
        .contains("The limit for locking tokens has been reached. No more tokens can be locked."));

    // total tokens locked after this action will be 2000, which is the cap
    info = get_message_info(
        &deps.api,
        "addr0000",
        &[Coin::new(500u64, IBC_DENOM_1.to_string())],
    );
    lock_msg = ExecuteMsg::LockTokens {
        lock_duration: THREE_MONTHS_IN_NANO_SECONDS,
        proof: None,
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), lock_msg.clone());
    assert!(res.is_ok());

    // advance the chain by one month plus one nanosecond and unlock the first lockup
    env.block.time = env.block.time.plus_nanos(ONE_MONTH_IN_NANO_SECONDS + 1);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::UnlockTokens { lock_ids: None },
    );
    assert!(res.is_ok());

    // now a user can lock new 1500 tokens
    info = get_message_info(
        &deps.api,
        "addr0000",
        &[Coin::new(1500u64, IBC_DENOM_1.to_string())],
    );
    let res = execute(deps.as_mut(), env.clone(), info.clone(), lock_msg.clone());
    assert!(res.is_ok());

    // a privileged user can update the maximum allowed locked tokens, but only for the future
    info = get_message_info(&deps.api, "addr0001", &[]);
    let update_max_locked_tokens_msg = ExecuteMsg::UpdateConfig {
        config: UpdateConfigData {
            activate_at: env.block.time.minus_hours(1),
            max_locked_tokens: Some(3000),
            known_users_cap: None,
            max_deployment_duration: None,
            cw721_collection_info: None,
            lock_depth_limit: None,
            lock_expiry_duration_seconds: None,
            slash_percentage_threshold: None,
            slash_tokens_receiver_addr: None,
        },
    };
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        update_max_locked_tokens_msg.clone(),
    );
    assert!(res
        .unwrap_err()
        .to_string()
        .contains("Can not update config in the past."));

    // this time with a valid activation timestamp
    let update_max_locked_tokens_msg = ExecuteMsg::UpdateConfig {
        config: UpdateConfigData {
            activate_at: env.block.time,
            max_locked_tokens: Some(3000),
            known_users_cap: None,
            max_deployment_duration: None,
            cw721_collection_info: None,
            lock_depth_limit: None,
            lock_expiry_duration_seconds: None,
            slash_percentage_threshold: None,
            slash_tokens_receiver_addr: None,
        },
    };
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        update_max_locked_tokens_msg.clone(),
    );
    assert!(res.is_ok());

    // now a user can lock up to additional 1000 tokens
    info = get_message_info(
        &deps.api,
        "addr0002",
        &[Coin::new(1000u64, IBC_DENOM_1.to_string())],
    );
    let res = execute(deps.as_mut(), env.clone(), info.clone(), lock_msg.clone());
    assert!(res.is_ok());

    // but no more than the cap of 3000 tokens
    info = get_message_info(
        &deps.api,
        "addr0002",
        &[Coin::new(1u64, IBC_DENOM_1.to_string())],
    );
    let res = execute(deps.as_mut(), env.clone(), info.clone(), lock_msg.clone());
    assert!(res.is_err());
    assert!(res
        .unwrap_err()
        .to_string()
        .contains("The limit for locking tokens has been reached. No more tokens can be locked."));

    // increase the maximum allowed locked tokens by 500, starting in 1 hour
    info = get_message_info(&deps.api, "addr0001", &[]);
    let update_max_locked_tokens_msg = ExecuteMsg::UpdateConfig {
        config: UpdateConfigData {
            activate_at: env.block.time.plus_hours(1),
            max_locked_tokens: Some(3500),
            known_users_cap: None,
            max_deployment_duration: None,
            cw721_collection_info: None,
            lock_depth_limit: None,
            lock_expiry_duration_seconds: None,
            slash_percentage_threshold: None,
            slash_tokens_receiver_addr: None,
        },
    };
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        update_max_locked_tokens_msg,
    );
    assert!(res.is_ok());

    // try to lock additional 500 tokens before the time is reached to increase the cap
    info = get_message_info(
        &deps.api,
        "addr0002",
        &[Coin::new(500u64, IBC_DENOM_1.to_string())],
    );
    let res = execute(deps.as_mut(), env.clone(), info.clone(), lock_msg.clone());
    assert!(res.is_err());
    assert!(res
        .unwrap_err()
        .to_string()
        .contains("The limit for locking tokens has been reached. No more tokens can be locked."));

    // advance the chain by 1h 0m 1s and verify user can lock additional 500 tokens
    env.block.time = env.block.time.plus_seconds(3601);

    // now a user can lock up to additional 500 tokens
    let res = execute(deps.as_mut(), env.clone(), info.clone(), lock_msg.clone());
    assert!(res.is_ok());
}
