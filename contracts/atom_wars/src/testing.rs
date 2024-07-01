use crate::contract::MAX_LOCK_ENTRIES;
use crate::query::QueryMsg;
use crate::state::Tranche;
use crate::{
    contract::{
        compute_current_round_id, execute, instantiate, query_all_user_lockups, query_constants,
        query_proposal, query_round_total_power, query_round_tranche_proposals,
        query_top_n_proposals,
    },
    msg::{ExecuteMsg, InstantiateMsg},
};
use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
use cosmwasm_std::{BankMsg, CosmosMsg, Deps, Timestamp};
use cosmwasm_std::{Coin, StdError, StdResult};
use proptest::prelude::*;

pub const STATOM: &str = "ibc/B7864B03E1B9FD4F049243E92ABD691586F682137037A9F3FCA5222815620B3C";
pub const ONE_DAY_IN_NANO_SECONDS: u64 = 24 * 60 * 60 * 1000000000;
pub const TWO_WEEKS_IN_NANO_SECONDS: u64 = 14 * 24 * 60 * 60 * 1000000000;
pub const ONE_MONTH_IN_NANO_SECONDS: u64 = 2629746000000000; // 365 days / 12
pub const THREE_MONTHS_IN_NANO_SECONDS: u64 = 3 * ONE_MONTH_IN_NANO_SECONDS;

pub fn get_default_instantiate_msg() -> InstantiateMsg {
    InstantiateMsg {
        denom: STATOM.to_string(),
        round_length: TWO_WEEKS_IN_NANO_SECONDS,
        lock_epoch_length: ONE_MONTH_IN_NANO_SECONDS,
        tranches: vec![Tranche {
            tranche_id: 1,
            metadata: "tranche 1".to_string(),
        }],
        first_round_start: mock_env().block.time,
        initial_whitelist: vec![get_default_covenant_params()],
        whitelist_admins: vec![],
    }
}

pub fn get_default_covenant_params() -> crate::state::CovenantParams {
    crate::state::CovenantParams {
        pool_id: "pool_id".to_string(),
        outgoing_channel_id: "outgoing_channel_id".to_string(),
        funding_destination_name: "funding_destination_name".to_string(),
    }
}

#[test]
fn instantiate_test() {
    let (mut deps, env, info) = (mock_dependencies(), mock_env(), mock_info("addr0000", &[]));
    let msg = get_default_instantiate_msg();

    let res = instantiate(deps.as_mut(), env, info, msg.clone());
    assert!(res.is_ok());

    let res = query_constants(deps.as_ref());
    assert!(res.is_ok());

    let constants = res.unwrap();
    assert_eq!(msg.denom, constants.denom);
    assert_eq!(msg.round_length, constants.round_length);
}

#[test]
fn lock_tokens_basic_test() {
    let user_address = "addr0000";
    let (mut deps, env, info) = (
        mock_dependencies(),
        mock_env(),
        mock_info(user_address, &[]),
    );
    let msg = get_default_instantiate_msg();

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

    let res = query_all_user_lockups(deps.as_ref(), user_address.to_string(), 0, 2000);
    assert!(res.is_ok());
    let res = res.unwrap();
    assert_eq!(2, res.lockups.len());

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
    let msg = get_default_instantiate_msg();

    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());

    // lock 1000 tokens for one month
    let msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
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
                assert_eq!(1, amount.len()); // all lock entries summed-up into one
                assert_eq!(user_token.denom, amount[0].denom);
                assert_eq!(user_token.amount.u128() * 2, amount[0].amount.u128());
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
    let msg = get_default_instantiate_msg();

    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());

    let covenant_params_1 = get_default_covenant_params();
    let msg1 = ExecuteMsg::CreateProposal {
        tranche_id: 1,
        covenant_params: covenant_params_1.clone(),
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg1.clone());
    assert!(res.is_ok());

    let mut covenant_params_2 = get_default_covenant_params().clone();
    covenant_params_2.pool_id = "pool_id_2".to_string();
    covenant_params_2.outgoing_channel_id = "outgoing_channel_id_2".to_string();
    covenant_params_2.funding_destination_name = "funding_destination_name_2".to_string();

    let msg2 = ExecuteMsg::CreateProposal {
        tranche_id: 1,
        covenant_params: covenant_params_2.clone(),
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg2.clone());
    assert!(res.is_ok());

    let expected_round_id = 0;
    let res = query_round_tranche_proposals(deps.as_ref(), expected_round_id, 1, 0, 3000);
    assert!(res.is_ok(), "error: {:?}", res);

    let res = res.unwrap();
    assert_eq!(2, res.proposals.len());

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

    let (mut deps, mut env, info) = (
        mock_dependencies(),
        mock_env(),
        mock_info(user_address, &[user_token.clone()]),
    );
    let msg = get_default_instantiate_msg();

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
        tranche_id: 1,
        covenant_params: get_default_covenant_params(),
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());

    // vote for the proposal
    let proposal_id = 0;
    let msg = ExecuteMsg::Vote {
        tranche_id: 1,
        proposal_id,
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());

    let round_id = 0;
    let res = query_proposal(deps.as_ref(), round_id, 1, proposal_id);
    assert!(res.is_ok());
    assert_eq!(info.funds[0].amount.u128(), res.unwrap().power.u128());

    // advance the chain by two weeks + 1 nano second to move to the next round and try to unlock tokens
    env.block.time = env.block.time.plus_nanos(TWO_WEEKS_IN_NANO_SECONDS + 1);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::UnlockTokens {},
    );

    // user voted for a proposal in previous round so it won't be able to unlock tokens
    assert!(res.is_err());
    assert!(res.unwrap_err().to_string().contains(
        "Tokens can not be unlocked, user voted for at least one proposal in previous round"
    ));
}

#[test]
fn multi_tranches_test() {
    let (mut deps, env, info) = (
        mock_dependencies(),
        mock_env(),
        mock_info("addr0000", &[Coin::new(1000, STATOM.to_string())]),
    );
    let mut msg = get_default_instantiate_msg();
    msg.tranches = vec![
        Tranche {
            tranche_id: 1,
            metadata: "tranche 1".to_string(),
        },
        Tranche {
            tranche_id: 2,
            metadata: "tranche 2".to_string(),
        },
    ];

    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());

    // create two proposals for tranche 1
    let msg1 = ExecuteMsg::CreateProposal {
        tranche_id: 1,
        covenant_params: get_default_covenant_params(),
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg1.clone());
    assert!(res.is_ok());

    let msg2 = ExecuteMsg::CreateProposal {
        tranche_id: 1,
        covenant_params: get_default_covenant_params(),
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg2.clone());
    assert!(res.is_ok());

    // create two proposals for tranche 2
    let msg3 = ExecuteMsg::CreateProposal {
        tranche_id: 2,
        covenant_params: get_default_covenant_params(),
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg3.clone());
    assert!(res.is_ok());

    let msg4 = ExecuteMsg::CreateProposal {
        tranche_id: 2,
        covenant_params: get_default_covenant_params(),
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg4.clone());
    assert!(res.is_ok());

    // vote with user 1
    // lock some tokens to get voting power
    let msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok());

    // vote for the first proposal of tranche 1
    let msg = ExecuteMsg::Vote {
        tranche_id: 1,
        proposal_id: 0,
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());

    // vote for the first proposal of tranche 2
    let msg = ExecuteMsg::Vote {
        tranche_id: 2,
        proposal_id: 2,
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());

    // vote for the second proposal of tranche 2 with a different user, who also locks more toekns
    let info2 = mock_info("addr0001", &[Coin::new(2000, STATOM.to_string())]);
    let msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
    };
    let res = execute(deps.as_mut(), env.clone(), info2.clone(), msg);
    assert!(res.is_ok());

    let msg = ExecuteMsg::Vote {
        tranche_id: 2,
        proposal_id: 2,
    };
    let res = execute(deps.as_mut(), env.clone(), info2.clone(), msg.clone());
    assert!(res.is_ok());

    // vote for the so-far unvoted proposals with a new user with just 1 token
    let info3 = mock_info("addr0002", &[Coin::new(1, STATOM.to_string())]);
    let msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
    };
    let res = execute(deps.as_mut(), env.clone(), info3.clone(), msg);
    assert!(res.is_ok());

    let msg = ExecuteMsg::Vote {
        tranche_id: 1,
        proposal_id: 1,
    };
    let res = execute(deps.as_mut(), env.clone(), info3.clone(), msg.clone());
    assert!(res.is_ok());

    let msg = ExecuteMsg::Vote {
        tranche_id: 2,
        proposal_id: 3,
    };
    let res = execute(deps.as_mut(), env.clone(), info3.clone(), msg.clone());
    assert!(res.is_ok());

    // query voting powers
    // top proposals for tranche 1
    // (round 0, tranche 1, show 2 proposals)
    let res = query_top_n_proposals(deps.as_ref(), 0, 1, 2);
    assert!(res.is_ok());
    let res = res.unwrap();
    // check that there are two proposals
    assert_eq!(2, res.len(), "expected 2 proposals, got {:?}", res);
    // check that the voting power of the first proposal is 1000
    assert_eq!(1000, res[0].power.u128());
    // check that the voting power of the second proposal is 0
    assert_eq!(1, res[1].power.u128());

    // top proposals for tranche 2
    // (round 0, tranche 2, show 2 proposals)
    let res = query_top_n_proposals(deps.as_ref(), 0, 2, 2);
    assert!(res.is_ok());
    let res = res.unwrap();
    // check that there are two proposals
    assert_eq!(2, res.len(), "expected 2 proposals, got {:?}", res);
    // check that the voting power of the first proposal is 3000
    assert_eq!(3000, res[0].power.u128());
    // check that the voting power of the second proposal is 0
    assert_eq!(1, res[1].power.u128());
}

#[test]
fn test_query_round_tranche_proposals_pagination() {
    let (mut deps, env, info) = (
        mock_dependencies(),
        mock_env(),
        mock_info("addr0000", &[Coin::new(1000, STATOM.to_string())]),
    );
    let mut msg = get_default_instantiate_msg();

    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());

    // Create multiple proposals
    let num_proposals = 5;
    for i in 0..num_proposals {
        let create_proposal_msg = ExecuteMsg::CreateProposal {
            tranche_id: 1,
            covenant_params: crate::state::CovenantParams {
                pool_id: format!("Pool ID {}", i),
                outgoing_channel_id: format!("Outgoing Channel ID {}", i),
                funding_destination_name: format!("Funding Destination Name {}", i),
            },
        };
        let _ = execute(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            create_proposal_msg,
        )
        .unwrap();
    }

    // Define test cases for start_after and limit with expected results
    let test_cases = vec![
        ((0, 2), vec![0, 1]), // Start from the beginning and get 2 elements -> we expect element 0 and 1
        ((0, 2), vec![0, 1]), // Start from the beginning and get 2 elements -> we expect element 0 and 1
        ((2, 2), vec![2, 3]), // Start from the second element, limit 2 -> we expect element 2 and 3
        ((4, 2), vec![4]),    // Start from the last element, limit 2 -> we expect element 4
        ((0, 5), vec![0, 1, 2, 3, 4]), // get the whole list -> we expect all elements
        ((0, 10), vec![0, 1, 2, 3, 4]), // get the whole list and the limit is even bigger -> we expect all elements
        ((2, 5), vec![2, 3, 4]), // Start from the middle, limit 5 -> we expect elements 2, 3, and 4
        ((4, 5), vec![4]),       // Start from the end, limit 5 -> we expect element 4
        ((5, 2), vec![]),        // start after the list is over -> we expect an empty list
        ((0, 0), vec![]),        // limit to 0 -> we expect an empty list
    ];

    // Test pagination for different start_after and limit values
    for ((start_after, limit), expected_proposals) in test_cases {
        let response =
            query_round_tranche_proposals(deps.as_ref(), 0, 1, start_after, limit).unwrap();

        // Check that pagination works correctly
        let proposals = response.proposals;
        assert_eq!(proposals.len(), expected_proposals.len());
        for (proposal, expected_proposal) in proposals.iter().zip(expected_proposals.iter()) {
            assert_eq!(
                proposal.covenant_params.pool_id,
                format!("Pool ID {}", *expected_proposal)
            );
        }
    }
}

#[test]
fn duplicate_tranche_id_test() {
    // try to instantiate the contract with two tranches with the same id
    // this should fail
    let (mut deps, env, info) = (mock_dependencies(), mock_env(), mock_info("addr0000", &[]));
    let mut msg = get_default_instantiate_msg();
    msg.tranches = vec![
        Tranche {
            tranche_id: 1,
            metadata: "tranche 1".to_string(),
        },
        Tranche {
            tranche_id: 1,
            metadata: "tranche 2".to_string(),
        },
    ];

    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_err());
    assert!(res
        .unwrap_err()
        .to_string()
        .to_lowercase()
        .contains("duplicate tranche id"));
}
#[test]
fn test_round_id_computation() {
    let test_cases: Vec<(u64, u64, u64, StdResult<u64>)> = vec![
        (
            0,     // contract start time
            1000,  // round length
            500,   // current time
            Ok(0), // expected round_id
        ),
        (
            1000,  // contract start time
            1000,  // round length
            1500,  // current time
            Ok(0), // expected round_id
        ),
        (
            0,     // contract start time
            1000,  // round length
            2500,  // current time
            Ok(2), // expected round_id
        ),
        (
            0,     // contract start time
            2000,  // round length
            6000,  // current time
            Ok(3), // expected round_id
        ),
        (
            10000, // contract start time
            5000,  // round length
            12000, // current time
            Ok(0), // expected round_id
        ),
        (
            3000,                                                              // contract start time
            1000,                                                              // round length
            2000,                                                              // current time
            Err(StdError::generic_err("The first round has not started yet")), // expected error
        ),
    ];

    for (contract_start_time, round_length, current_time, expected_round_id) in test_cases {
        // instantiate the contract
        let mut deps = mock_dependencies();
        let mut msg = get_default_instantiate_msg();
        msg.round_length = round_length;
        msg.first_round_start = Timestamp::from_nanos(contract_start_time);

        let mut env = mock_env();
        env.block.time = Timestamp::from_nanos(contract_start_time);
        let info = mock_info("addr0000", &[]);
        let _ = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone()).unwrap();

        // set the time to the current time
        env.block.time = Timestamp::from_nanos(current_time);

        let constants = query_constants(deps.as_ref());
        assert!(constants.is_ok());

        let round_id = compute_current_round_id(&env, &constants.unwrap());
        assert_eq!(expected_round_id, round_id);
    }
}

#[test]
fn total_voting_power_tracking_test() {
    let user_address = "addr0000";
    let (mut deps, mut env, info) = (
        mock_dependencies(),
        mock_env(),
        mock_info(user_address, &[]),
    );

    let mut msg = get_default_instantiate_msg();

    // align round length with lock epoch length for easier calculations
    msg.round_length = ONE_MONTH_IN_NANO_SECONDS;

    let res = instantiate(deps.as_mut(), env.clone(), info, msg.clone());
    assert!(res.is_ok());

    let info1 = mock_info(user_address, &[Coin::new(10, STATOM.to_string())]);
    let msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
    };
    let res = execute(deps.as_mut(), env.clone(), info1.clone(), msg);
    assert!(res.is_ok());

    // user locks 10 tokens for one month, so it will have 1x voting power in the first round only
    let expected_total_voting_powers = [(0, Some(10)), (1, None)];
    verify_expected_voting_power(deps.as_ref(), &expected_total_voting_powers);

    // advance the chain by 10 days and have user lock more tokens
    env.block.time = env.block.time.plus_nanos(10 * ONE_DAY_IN_NANO_SECONDS);

    let info2 = mock_info(user_address, &[Coin::new(20, STATOM.to_string())]);
    let msg = ExecuteMsg::LockTokens {
        lock_duration: THREE_MONTHS_IN_NANO_SECONDS,
    };
    let res = execute(deps.as_mut(), env.clone(), info2.clone(), msg);
    assert!(res.is_ok());

    // user locks 20 additional tokens for three months, so the expectation is:
    // round:         0      1       2       3
    // power:       10+30   0+30    0+20    0+0
    let expected_total_voting_powers = [(0, Some(40)), (1, Some(30)), (2, Some(20)), (3, None)];
    verify_expected_voting_power(deps.as_ref(), &expected_total_voting_powers);

    // advance the chain by 25 more days to move to round 1 and have user refresh second lockup to 6 months
    env.block.time = env.block.time.plus_nanos(25 * ONE_DAY_IN_NANO_SECONDS);

    let info3 = mock_info(user_address, &[]);
    let msg = ExecuteMsg::RefreshLockDuration {
        lock_id: 1,
        lock_duration: 2 * THREE_MONTHS_IN_NANO_SECONDS,
    };
    let res = execute(deps.as_mut(), env.clone(), info3.clone(), msg);
    assert!(res.is_ok());

    // user relocks second lockup worth 20 tokens for six months in round 1, so the expectation is (note that round 0 is not affected):
    // round:         0       1       2       3       4       5       6       7
    // power:       10+30    0+40    0+40    0+40    0+30    0+30    0+20    0+0
    let expected_total_voting_powers = [
        (0, Some(40)),
        (1, Some(40)),
        (2, Some(40)),
        (3, Some(40)),
        (4, Some(30)),
        (5, Some(30)),
        (6, Some(20)),
        (7, None),
    ];
    verify_expected_voting_power(deps.as_ref(), &expected_total_voting_powers);

    // advance the chain by 5 more days and have user lock 50 more tokens for three months
    env.block.time = env.block.time.plus_nanos(5 * ONE_DAY_IN_NANO_SECONDS);

    let info2 = mock_info(user_address, &[Coin::new(50, STATOM.to_string())]);
    let msg = ExecuteMsg::LockTokens {
        lock_duration: THREE_MONTHS_IN_NANO_SECONDS,
    };
    let res = execute(deps.as_mut(), env.clone(), info2.clone(), msg);
    assert!(res.is_ok());

    // user locks 50 additional tokens in round 1 for three months, so the expectation is (note that round 0 is not affected):
    // round:         0        1          2          3          4         5         6         7
    // power:       10+30    0+40+75    0+40+75    0+40+50    0+30+0    0+30+0    0+20+0    0+0+0
    let expected_total_voting_powers = [
        (0, Some(40)),
        (1, Some(115)),
        (2, Some(115)),
        (3, Some(90)),
        (4, Some(30)),
        (5, Some(30)),
        (6, Some(20)),
        (7, None),
    ];
    verify_expected_voting_power(deps.as_ref(), &expected_total_voting_powers);
}

fn verify_expected_voting_power(deps: Deps, expected_powers: &[(u64, Option<u128>)]) {
    for expected_power in expected_powers {
        let res = query_round_total_power(deps, expected_power.0);

        match expected_power.1 {
            Some(total_power) => {
                assert!(res.is_ok());
                let res = res.unwrap();
                assert_eq!(total_power, res.u128());
            }
            None => {
                assert!(res.is_err());
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10000))] // set the number of test cases to run
    #[test]
    fn relock_proptest(old_lock_remaining_time: u64, new_lock_duration: u8) {
        let (mut deps, mut env, info) = (
            mock_dependencies(),
            mock_env(),
            mock_info("addr0001", &[Coin::new(1000, STATOM.to_string())]),
        );
        let msg = get_default_instantiate_msg();

        let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
        assert!(res.is_ok());

        // get the new lock duration
        // list of plausible values, plus a value that should give an error every time (0)
        let possible_lock_durations = [0, ONE_MONTH_IN_NANO_SECONDS, ONE_MONTH_IN_NANO_SECONDS * 3, ONE_MONTH_IN_NANO_SECONDS * 6, ONE_MONTH_IN_NANO_SECONDS * 12];
        let new_lock_duration = possible_lock_durations[new_lock_duration as usize % possible_lock_durations.len()];

        // old lock remaining time must be at most 12 months, so we take the modulo
        let old_lock_remaining_time = old_lock_remaining_time % (ONE_MONTH_IN_NANO_SECONDS * 12);

        // lock the tokens for 12 months
        let msg = ExecuteMsg::LockTokens {
            lock_duration: ONE_MONTH_IN_NANO_SECONDS * 12,
        };

        let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
        assert!(res.is_ok());

        // set the time so that old_lock_remaining_time remains on the old lock
        env.block.time = env.block.time.plus_nanos(12 * ONE_MONTH_IN_NANO_SECONDS - old_lock_remaining_time);

        // try to refresh the lock duration as a different user
        let info2 = mock_info("addr0002", &[]);
        let msg = ExecuteMsg::RefreshLockDuration {
            lock_id: 0,
            lock_duration: new_lock_duration,
        };
        let res = execute(deps.as_mut(), env.clone(), info2.clone(), msg);

        // different user cannot refresh the lock
        assert!(res.is_err(), "different user should not be able to refresh the lock: {:?}", res);

        // refresh the lock duration
        let info = mock_info("addr0001", &[]);
        let msg = ExecuteMsg::RefreshLockDuration {
            lock_id: 0,
            lock_duration: new_lock_duration,
        };
        let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);

        // if we try to refresh the lock with a duration of 0, it should fail
        if new_lock_duration == 0 {
            assert!(res.is_err());
            return Ok(()); // end the test
        }

        // if we tried to make the lock_end sooner, it should fail
        if new_lock_duration < old_lock_remaining_time {
            assert!(res.is_err());
            return Ok(()); // end the test
        }

        // otherwise, succeed
        assert!(res.is_ok());
    }
}

#[test]
fn test_too_many_locks() {
    let (mut deps, mut env, info) = (
        mock_dependencies(),
        mock_env(),
        mock_info("addr0000", &[Coin::new(1000, STATOM.to_string())]),
    );
    let mut msg = get_default_instantiate_msg();

    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());

    // lock tokens many times
    let lock_msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
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
    let info2 = mock_info("addr0001", &[Coin::new(1000, STATOM.to_string())]);
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
    let unlock_msg = ExecuteMsg::UnlockTokens {};
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
