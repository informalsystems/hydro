use crate::{
    contract::{
        execute, instantiate, query_all_user_lockups, query_constants, query_current_round,
        query_proposal, query_round_proposals, ONE_MONTH_IN_NANO_SECONDS,
    },
    ExecuteMsg, InstantiateMsg,
};
use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
use cosmwasm_std::Coin;
use cosmwasm_std::{BankMsg, CosmosMsg, Uint128};

const STATOM: &str = "ibc/B7864B03E1B9FD4F049243E92ABD691586F682137037A9F3FCA5222815620B3C";
const TWO_WEEKS_IN_NANO_SECONDS: u64 = 14 * 24 * 60 * 60 * 1000000000;
const THREE_MONTHS_IN_NANO_SECONDS: u64 = 3 * ONE_MONTH_IN_NANO_SECONDS;

#[test]
fn instantiate_test() {
    let (mut deps, env, info) = (mock_dependencies(), mock_env(), mock_info("addr0000", &[]));
    let msg = InstantiateMsg {
        denom: STATOM.to_string(),
        round_length: TWO_WEEKS_IN_NANO_SECONDS,
        total_pool: Uint128::zero(),
    };

    let res = instantiate(deps.as_mut(), env, info, msg.clone());
    assert!(res.is_ok());

    let res = query_constants(deps.as_ref());
    assert!(res.is_ok());

    let constants = res.unwrap();
    assert_eq!(msg.denom, constants.denom);
    assert_eq!(msg.round_length, constants.round_length);
    assert_eq!(msg.total_pool, constants.total_pool);
}

#[test]
fn lock_tokens_basic_test() {
    let user_address = "addr0000";
    let (mut deps, env, info) = (
        mock_dependencies(),
        mock_env(),
        mock_info(user_address, &[]),
    );
    let msg = InstantiateMsg {
        denom: STATOM.to_string(),
        round_length: TWO_WEEKS_IN_NANO_SECONDS,
        total_pool: Uint128::zero(),
    };

    let res = instantiate(deps.as_mut(), env.clone(), info, msg.clone());
    assert!(res.is_ok());

    let info1 = mock_info(user_address, &[Coin::new(1000, STATOM.to_string())]);
    let msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
    };
    let res = execute(deps.as_mut(), env.clone(), info1.clone(), msg);
    assert!(res.is_ok());

    let info2 = mock_info(user_address, &[Coin::new(3000, STATOM.to_string())]);
    let msg = ExecuteMsg::LockTokens {
        lock_duration: THREE_MONTHS_IN_NANO_SECONDS,
    };
    let res = execute(deps.as_mut(), env.clone(), info2.clone(), msg);
    assert!(res.is_ok());

    let res = query_all_user_lockups(deps.as_ref(), user_address.to_string());
    assert!(res.is_ok());
    let res = res.unwrap();
    assert_eq!(2, (&res).lockups.len());

    let lockup = &res.lockups[0];
    assert_eq!(info1.funds[0].amount.u128(), lockup.funds.amount.u128());
    assert_eq!(info1.funds[0].denom, lockup.funds.denom);
    assert_eq!(env.block.time, lockup.lock_start);
    assert_eq!(
        env.block.time.plus_nanos(ONE_MONTH_IN_NANO_SECONDS),
        lockup.lock_end
    );

    let lockup = &res.lockups[1];
    assert_eq!(info2.funds[0].amount.u128(), lockup.funds.amount.u128());
    assert_eq!(info2.funds[0].denom, lockup.funds.denom);
    assert_eq!(env.block.time, lockup.lock_start);
    assert_eq!(
        env.block.time.plus_nanos(THREE_MONTHS_IN_NANO_SECONDS),
        lockup.lock_end
    );
}

#[test]
fn unlock_tokens_basic_test() {
    let user_address = "addr0000";
    let user_token = Coin::new(1000, STATOM.to_string());

    let (mut deps, mut env, info) = (
        mock_dependencies(),
        mock_env(),
        mock_info(user_address, &[user_token.clone()]),
    );
    let msg = InstantiateMsg {
        denom: STATOM.to_string(),
        round_length: TWO_WEEKS_IN_NANO_SECONDS,
        total_pool: Uint128::zero(),
    };

    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());

    let msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok());

    // check that user can not unlock tokens immediately
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::UnlockTokens {},
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
        ExecuteMsg::UnlockTokens {},
    );
    assert!(res.is_ok());

    let res = res.unwrap();
    assert_eq!(1, res.messages.len());

    match &res.messages[0].msg {
        CosmosMsg::Bank(bank_msg) => match bank_msg {
            BankMsg::Send { to_address, amount } => {
                assert_eq!(user_address.to_string(), *to_address);
                assert_eq!(1, amount.len());
                assert_eq!(user_token.denom, amount[0].denom);
                assert_eq!(user_token.amount.u128(), amount[0].amount.u128());
            }
            _ => panic!("expected BankMsg::Send message"),
        },
        _ => panic!("expected CosmosMsg::Bank msg"),
    };
}

#[test]
fn create_proposal_basic_test() {
    let user_address = "addr0000";
    let user_token = Coin::new(1000, STATOM.to_string());

    let (mut deps, env, info) = (
        mock_dependencies(),
        mock_env(),
        mock_info(user_address, &[user_token.clone()]),
    );
    let msg = InstantiateMsg {
        denom: STATOM.to_string(),
        round_length: TWO_WEEKS_IN_NANO_SECONDS,
        total_pool: Uint128::zero(),
    };

    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());

    let covenant_params_1 = "first proposal";
    let msg1 = ExecuteMsg::CreateProposal {
        covenant_params: covenant_params_1.to_string(),
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg1.clone());
    assert!(res.is_ok());

    let covenant_params_2 = "second proposal";
    let msg2 = ExecuteMsg::CreateProposal {
        covenant_params: covenant_params_2.to_string(),
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg2.clone());
    assert!(res.is_ok());

    let expected_round_id = 0;
    let res = query_round_proposals(deps.as_ref(), expected_round_id);
    assert!(res.is_ok());

    let res = res.unwrap();
    assert_eq!(2, (&res).proposals.len());

    let proposal = &res.proposals[0];
    assert_eq!(expected_round_id, proposal.round_id);
    assert_eq!(covenant_params_1, proposal.covenant_params);

    let proposal = &res.proposals[1];
    assert_eq!(expected_round_id, proposal.round_id);
    assert_eq!(covenant_params_2, proposal.covenant_params);
}

#[test]
fn vote_basic_test() {
    let user_address = "addr0000";
    let user_token = Coin::new(1000, STATOM.to_string());

    let (mut deps, env, info) = (
        mock_dependencies(),
        mock_env(),
        mock_info(user_address, &[user_token.clone()]),
    );
    let msg = InstantiateMsg {
        denom: STATOM.to_string(),
        round_length: TWO_WEEKS_IN_NANO_SECONDS,
        total_pool: Uint128::zero(),
    };

    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());

    // lock some tokens to get voting power
    let msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok());

    // create a new proposal
    let msg = ExecuteMsg::CreateProposal {
        covenant_params: "proposal".to_string(),
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());

    // vote for the proposal
    let proposal_id = 0;
    let msg = ExecuteMsg::Vote { proposal_id };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());

    let round_id = 0;
    let res = query_proposal(deps.as_ref(), round_id, proposal_id);
    assert!(res.is_ok());
    assert_eq!(info.funds[0].amount.u128(), res.unwrap().power.u128());
}

#[test]
fn end_round_basic_test() {
    let (mut deps, mut env, info) = (mock_dependencies(), mock_env(), mock_info("addr0000", &[]));
    let msg = InstantiateMsg {
        denom: STATOM.to_string(),
        round_length: TWO_WEEKS_IN_NANO_SECONDS,
        total_pool: Uint128::zero(),
    };

    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());

    let res = query_current_round(deps.as_ref());
    assert!(res.is_ok());
    assert_eq!(0, res.unwrap().round_id);

    // verify that the round can not be ended before the end time of round is reached
    env.block.time = env.block.time.plus_nanos(1001);
    let msg = ExecuteMsg::EndRound {};
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_err());
    assert!(res
        .unwrap_err()
        .to_string()
        .contains("Round has not ended yet"));

    // additionally advance the chain by the round length and verify that the round can now be ended
    env.block.time = env.block.time.plus_nanos(TWO_WEEKS_IN_NANO_SECONDS);
    let msg = ExecuteMsg::EndRound {};
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());

    let res = query_current_round(deps.as_ref());
    assert!(res.is_ok());
    assert_eq!(1, res.unwrap().round_id);
}
