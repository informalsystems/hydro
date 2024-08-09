use crate::testing::ONE_MONTH_IN_NANO_SECONDS;
use crate::{
    contract::{execute, instantiate, query_expired_user_lockups, query_user_voting_power},
    msg::ExecuteMsg,
    state::LockEntry,
    testing::{get_default_instantiate_msg, DEFAULT_DENOM},
};
use cosmwasm_std::{
    testing::{mock_dependencies, mock_env, mock_info, MockApi, MockQuerier, MockStorage},
    Coin, Empty, Env, OwnedDeps,
};

#[test]
fn query_expired_user_lockups_test() {
    let user_address = "addr0000";
    let (mut deps, mut env, info) = (
        mock_dependencies(),
        mock_env(),
        mock_info(user_address, &[]),
    );

    let msg = get_default_instantiate_msg();

    let res = instantiate(deps.as_mut(), env.clone(), info, msg.clone());
    assert!(res.is_ok());

    // simulate user locking 1000 tokens for 1 month, one day after the round started
    env.block.time = env.block.time.plus_days(1);
    let first_lockup_amount = 1000;
    let info = mock_info(
        user_address,
        &[Coin::new(first_lockup_amount, DEFAULT_DENOM.to_string())],
    );
    let msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok());

    // simulate user locking 2000 tokens for 3 months, two days after the round started
    env.block.time = env.block.time.plus_days(1);
    let second_lockup_amount = 2000;
    let info = mock_info(
        user_address,
        &[Coin::new(second_lockup_amount, DEFAULT_DENOM.to_string())],
    );
    let msg = ExecuteMsg::LockTokens {
        lock_duration: 3 * ONE_MONTH_IN_NANO_SECONDS,
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok());

    // at this moment user doesn't have any expired lockups
    let expired_lockups = get_expired_user_lockups(&deps, env.clone(), user_address.to_string());
    assert_eq!(0, expired_lockups.len());

    // advance the chain for a month and verify that the first lockup has expired
    env.block.time = env.block.time.plus_nanos(ONE_MONTH_IN_NANO_SECONDS);
    let expired_lockups = get_expired_user_lockups(&deps, env.clone(), user_address.to_string());
    assert_eq!(1, expired_lockups.len());
    assert_eq!(first_lockup_amount, expired_lockups[0].funds.amount.u128());

    // advance the chain for 3 more months and verify that the second lockup has expired as well
    env.block.time = env.block.time.plus_nanos(3 * ONE_MONTH_IN_NANO_SECONDS);
    let expired_lockups = get_expired_user_lockups(&deps, env.clone(), user_address.to_string());
    assert_eq!(2, expired_lockups.len());
    assert_eq!(first_lockup_amount, expired_lockups[0].funds.amount.u128());
    assert_eq!(second_lockup_amount, expired_lockups[1].funds.amount.u128());

    // unlock the tokens and verify that the user doesn't have any expired lockups after that
    let msg = ExecuteMsg::UnlockTokens {};
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok());

    let expired_lockups = get_expired_user_lockups(&deps, env.clone(), user_address.to_string());
    assert_eq!(0, expired_lockups.len());
}

#[test]
fn query_user_voting_power_test() {
    let user_address = "addr0000";
    let (mut deps, mut env, info) = (
        mock_dependencies(),
        mock_env(),
        mock_info(user_address, &[]),
    );

    let msg = get_default_instantiate_msg();

    let res = instantiate(deps.as_mut(), env.clone(), info, msg.clone());
    assert!(res.is_ok());

    // simulate user locking 1000 tokens for 1 month, one day after the round started
    let mut env_new = env.clone();
    env_new.block.time = env_new.block.time.plus_days(1);
    let first_lockup_amount = 1000;
    let info = mock_info(
        user_address,
        &[Coin::new(first_lockup_amount, DEFAULT_DENOM.to_string())],
    );
    let msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
    };

    let res = execute(deps.as_mut(), env_new.clone(), info.clone(), msg);
    assert!(res.is_ok());

    // simulate user locking 2000 tokens for 3 months, two days after the round started
    env_new.block.time = env.block.time.plus_days(2);
    let second_lockup_amount = 2000;
    let info = mock_info(
        user_address,
        &[Coin::new(second_lockup_amount, DEFAULT_DENOM.to_string())],
    );
    let msg = ExecuteMsg::LockTokens {
        lock_duration: 3 * ONE_MONTH_IN_NANO_SECONDS,
    };

    let res = execute(deps.as_mut(), env_new.clone(), info.clone(), msg);
    assert!(res.is_ok());

    // first lockup expires one day after the round 0 ends, and the second
    // lockup expires 2 months and 2 days after the round 0 ends, so the
    // expected voting power multipler is 1 for first lockup and 1.5 for second lockup
    let voting_power = get_user_voting_power(&deps, env.clone(), user_address.to_string());
    let expected_voting_power =
        first_lockup_amount + second_lockup_amount + (second_lockup_amount / 2);
    assert_eq!(expected_voting_power, voting_power);

    // advance the chain for 1 month and start a new round
    env.block.time = env.block.time.plus_nanos(ONE_MONTH_IN_NANO_SECONDS);

    // first lockup expires 29 days before the round 1 ends, and the second
    // lockup expires 1 month and 2 days after the round 1 ends, so the
    // expected voting power multipler is 0 for first lockup and 1.5 for second lockup
    let voting_power = get_user_voting_power(&deps, env.clone(), user_address.to_string());
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

    res.unwrap()
}
