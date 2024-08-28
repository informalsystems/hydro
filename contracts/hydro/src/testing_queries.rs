use std::collections::HashMap;

use crate::contract::{query_all_user_lockups, scale_lockup_power};
use crate::testing::{
    get_default_instantiate_msg, get_message_info, set_default_validator_for_rounds, IBC_DENOM_1,
    ONE_MONTH_IN_NANO_SECONDS, VALIDATOR_1_LST_DENOM_1,
};
use crate::testing_mocks::{denom_trace_grpc_query_mock, mock_dependencies, MockQuerier};
use crate::{
    contract::{execute, instantiate, query_expired_user_lockups, query_user_voting_power},
    msg::ExecuteMsg,
    state::LockEntry,
};
use cosmwasm_std::Uint128;
use cosmwasm_std::{
    testing::{mock_env, MockApi, MockStorage},
    Coin, Empty, Env, OwnedDeps,
};

#[test]
fn query_user_lockups_test() {
    let user_address = "addr0000";
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
    );
    let (mut deps, mut env) = (mock_dependencies(grpc_query), mock_env());
    let info = get_message_info(&deps.api, user_address, &[]);

    let msg = get_default_instantiate_msg(&deps.api);

    let res = instantiate(deps.as_mut(), env.clone(), info, msg.clone());
    assert!(res.is_ok());

    // simulate user locking 1000 tokens for 1 month, one day after the round started
    env.block.time = env.block.time.plus_days(1);

    set_default_validator_for_rounds(deps.as_mut(), 0, 100);

    let first_lockup_amount = 1000;
    let info = get_message_info(
        &deps.api,
        user_address,
        &[Coin::new(first_lockup_amount, IBC_DENOM_1.to_string())],
    );
    let msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok());

    // simulate user locking 2000 tokens for 3 months, two days after the round started
    env.block.time = env.block.time.plus_days(1);

    // set validators for new round
    set_default_validator_for_rounds(deps.as_mut(), 0, 100);

    let second_lockup_amount = 2000;
    let info = get_message_info(
        &deps.api,
        user_address,
        &[Coin::new(second_lockup_amount, IBC_DENOM_1.to_string())],
    );
    let msg = ExecuteMsg::LockTokens {
        lock_duration: 3 * ONE_MONTH_IN_NANO_SECONDS,
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok());

    // at this moment user doesn't have any expired lockups
    let expired_lockups = get_expired_user_lockups(&deps, env.clone(), info.sender.to_string());
    assert_eq!(0, expired_lockups.len());

    // but they should have 2 lockups
    let res = query_all_user_lockups(deps.as_ref(), env.clone(), info.sender.to_string(), 0, 2000);
    assert!(res.is_ok());
    let res = res.unwrap();

    assert_eq!(2, res.lockups.len());
    assert_eq!(
        first_lockup_amount,
        res.lockups[0].lock_entry.funds.amount.u128()
    );
    assert_eq!(
        second_lockup_amount,
        res.lockups[1].lock_entry.funds.amount.u128()
    );

    // check that the voting powers match
    assert_eq!(
        first_lockup_amount,
        res.lockups[0].current_voting_power.u128()
    );
    assert_eq!(
        // adjust for the 3 month lockup
        scale_lockup_power(
            ONE_MONTH_IN_NANO_SECONDS,
            3 * ONE_MONTH_IN_NANO_SECONDS,
            Uint128::new(second_lockup_amount),
        )
        .u128(),
        res.lockups[1].current_voting_power.u128()
    );

    // advance the chain for a month and verify that the first lockup has expired
    env.block.time = env.block.time.plus_nanos(ONE_MONTH_IN_NANO_SECONDS);
    let expired_lockups = get_expired_user_lockups(&deps, env.clone(), info.sender.to_string());
    assert_eq!(1, expired_lockups.len());
    assert_eq!(first_lockup_amount, expired_lockups[0].funds.amount.u128());

    let all_lockups =
        query_all_user_lockups(deps.as_ref(), env.clone(), info.sender.to_string(), 0, 2000);
    assert!(all_lockups.is_ok());

    let all_lockups = all_lockups.unwrap();
    assert_eq!(2, all_lockups.lockups.len()); // still 2 lockups
    assert_eq!(
        first_lockup_amount,
        all_lockups.lockups[0].lock_entry.funds.amount.u128()
    );
    assert_eq!(
        second_lockup_amount,
        all_lockups.lockups[1].lock_entry.funds.amount.u128()
    );

    // check that the first lockup has power 0
    assert_eq!(0, all_lockups.lockups[0].current_voting_power.u128());

    // second lockup still has 2 months left, so has power
    assert_eq!(
        // adjust for the remaining 2 month lockup
        scale_lockup_power(
            ONE_MONTH_IN_NANO_SECONDS,
            2 * ONE_MONTH_IN_NANO_SECONDS,
            Uint128::new(second_lockup_amount),
        )
        .u128(),
        all_lockups.lockups[1].current_voting_power.u128()
    );

    // advance the chain for 3 more months and verify that the second lockup has expired as well
    env.block.time = env.block.time.plus_nanos(3 * ONE_MONTH_IN_NANO_SECONDS);
    let expired_lockups = get_expired_user_lockups(&deps, env.clone(), info.sender.to_string());
    assert_eq!(2, expired_lockups.len());
    assert_eq!(first_lockup_amount, expired_lockups[0].funds.amount.u128());
    assert_eq!(second_lockup_amount, expired_lockups[1].funds.amount.u128());

    let all_lockups =
        query_all_user_lockups(deps.as_ref(), env.clone(), info.sender.to_string(), 0, 2000);

    assert!(all_lockups.is_ok());

    let all_lockups = all_lockups.unwrap();
    assert_eq!(2, all_lockups.lockups.len()); // still 2 lockups
    assert_eq!(
        first_lockup_amount,
        all_lockups.lockups[0].lock_entry.funds.amount.u128()
    );
    assert_eq!(
        second_lockup_amount,
        all_lockups.lockups[1].lock_entry.funds.amount.u128()
    );

    // check that both lockups have 0 voting power
    assert_eq!(0, all_lockups.lockups[0].current_voting_power.u128());
    assert_eq!(0, all_lockups.lockups[1].current_voting_power.u128());

    // set validators for this round once again
    set_default_validator_for_rounds(deps.as_mut(), 0, 100);

    // unlock the tokens and verify that the user doesn't have any expired lockups after that
    let msg = ExecuteMsg::UnlockTokens {};
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok());

    let expired_lockups = get_expired_user_lockups(&deps, env.clone(), info.sender.to_string());
    assert_eq!(0, expired_lockups.len());

    let all_lockups =
        query_all_user_lockups(deps.as_ref(), env.clone(), info.sender.to_string(), 0, 2000);
    assert!(all_lockups.is_ok());

    let all_lockups = all_lockups.unwrap();
    assert_eq!(0, all_lockups.lockups.len());
}

#[test]
fn query_user_voting_power_test() {
    let user_address = "addr0000";
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
    );
    let (mut deps, mut env) = (mock_dependencies(grpc_query), mock_env());
    let info = get_message_info(&deps.api, user_address, &[]);
    let msg = get_default_instantiate_msg(&deps.api);

    let res = instantiate(deps.as_mut(), env.clone(), info, msg.clone());
    assert!(res.is_ok());

    // simulate user locking 1000 tokens for 1 month, one day after the round started
    let mut env_new = env.clone();
    env_new.block.time = env_new.block.time.plus_days(1);

    set_default_validator_for_rounds(deps.as_mut(), 0, 100);

    let first_lockup_amount = 1000;
    let info = get_message_info(
        &deps.api,
        user_address,
        &[Coin::new(first_lockup_amount, IBC_DENOM_1.to_string())],
    );
    let msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
    };

    let res = execute(deps.as_mut(), env_new.clone(), info.clone(), msg);
    assert!(res.is_ok());

    // simulate user locking 2000 tokens for 3 months, two days after the round started
    env_new.block.time = env.block.time.plus_days(2);

    // set the validators for the new round
    set_default_validator_for_rounds(deps.as_mut(), 0, 100);

    let second_lockup_amount = 2000;
    let info = get_message_info(
        &deps.api,
        user_address,
        &[Coin::new(second_lockup_amount, IBC_DENOM_1.to_string())],
    );
    let msg = ExecuteMsg::LockTokens {
        lock_duration: 3 * ONE_MONTH_IN_NANO_SECONDS,
    };

    let res = execute(deps.as_mut(), env_new.clone(), info.clone(), msg);
    assert!(res.is_ok());

    // first lockup expires one day after the round 0 ends, and the second
    // lockup expires 2 months and 2 days after the round 0 ends, so the
    // expected voting power multipler is 1 for first lockup and 1.5 for second lockup
    let voting_power = get_user_voting_power(&deps, env.clone(), info.sender.to_string());
    let expected_voting_power =
        first_lockup_amount + second_lockup_amount + (second_lockup_amount / 2);
    assert_eq!(expected_voting_power, voting_power);

    // advance the chain for 1 month and start a new round
    env.block.time = env.block.time.plus_nanos(ONE_MONTH_IN_NANO_SECONDS);

    // set the validators for the new round, again
    set_default_validator_for_rounds(deps.as_mut(), 0, 100);

    // first lockup expires 29 days before the round 1 ends, and the second
    // lockup expires 1 month and 2 days after the round 1 ends, so the
    // expected voting power multipler is 0 for first lockup and 1.5 for second lockup
    let voting_power = get_user_voting_power(&deps, env.clone(), info.sender.to_string());
    let expected_voting_power = second_lockup_amount + (second_lockup_amount / 2);
    assert_eq!(expected_voting_power, voting_power);
}

fn get_expired_user_lockups(
    deps: &OwnedDeps<MockStorage, MockApi, MockQuerier, Empty>,
    env: Env,
    user_address: String,
) -> Vec<LockEntry> {
    let res = query_expired_user_lockups(
        deps.as_ref(),
        env.clone(),
        user_address.to_string(),
        0,
        2000,
    );
    assert!(res.is_ok());
    let res = res.unwrap();

    res.lockups
}

fn get_user_voting_power(
    deps: &OwnedDeps<MockStorage, MockApi, MockQuerier, Empty>,
    env: Env,
    user_address: String,
) -> u128 {
    let res = query_user_voting_power(deps.as_ref(), env, user_address.to_string());
    assert!(res.is_ok());

    res.unwrap().voting_power
}
