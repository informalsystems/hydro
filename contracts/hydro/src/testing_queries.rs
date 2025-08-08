use std::collections::HashMap;
use std::str::FromStr;

use crate::contract::{
    compute_current_round_id, query_all_user_lockups, query_all_user_lockups_with_tranche_infos,
    query_all_votes, query_all_votes_round_tranche, query_simulate_dtoken_amounts,
    query_specific_user_lockups, query_specific_user_lockups_with_tranche_infos, query_user_votes,
};
use crate::msg::{ProposalToLockups, TokenInfoProviderInstantiateMsg, TrancheInfo};
use crate::query::VoteEntry;
use crate::state::{
    DropTokenInfo, HeightRange, RoundLockPowerSchedule, ValidatorInfo, Vote, DROP_TOKEN_INFO,
    LOCKS_MAP_V2, ROUND_TO_HEIGHT_RANGE, SNAPSHOTS_ACTIVATION_HEIGHT, TOKEN_INFO_PROVIDERS,
    USER_LOCKS, VALIDATORS_INFO, VOTE_MAP_V2,
};
use crate::testing::{
    get_address_as_str, get_default_instantiate_msg, get_default_lsm_token_info_provider,
    get_message_info, set_default_validator_for_rounds, IBC_DENOM_1, IBC_DENOM_2,
    ONE_MONTH_IN_NANO_SECONDS, VALIDATOR_1, VALIDATOR_1_LST_DENOM_1, VALIDATOR_2,
    VALIDATOR_2_LST_DENOM_1, VALIDATOR_3,
};
use crate::testing_lsm_integration::set_validator_power_ratio;
use crate::testing_mocks::{
    denom_trace_grpc_query_mock, mock_dependencies, no_op_grpc_query_mock, MockQuerier,
};
use crate::token_manager::LSM_TOKEN_INFO_PROVIDER_ID;
use crate::utils::{load_current_constants, scale_lockup_power};
use crate::{
    contract::{execute, instantiate, query_expired_user_lockups, query_user_voting_power},
    msg::ExecuteMsg,
    state::LockEntryV2,
};
use cosmwasm_std::Decimal256;
use cosmwasm_std::{
    from_json, to_json_binary, Addr, ContractResult, Decimal, QuerierResult, StdError, StdResult,
    SystemError, SystemResult, Timestamp, Uint128, WasmQuery,
};
use cosmwasm_std::{
    testing::{mock_env, MockApi, MockStorage},
    Coin, Env, OwnedDeps,
};
use interface::drop_core::QueryMsg as DropQueryMsg;
use interface::drop_puppeteer::{
    Delegations, DelegationsResponse, DropDelegation, PuppeteerQueryMsg, QueryExtMsg,
};
use neutron_sdk::bindings::query::NeutronQuery;

pub type WasmQueryFunc = dyn Fn(&WasmQuery) -> QuerierResult;

#[test]
fn query_user_lockups_test() {
    let user_address = "addr0000";
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
    );
    let (mut deps, mut env) = (mock_dependencies(grpc_query), mock_env());
    let info = get_message_info(&deps.api, user_address, &[]);

    let instantiate_msg: crate::msg::InstantiateMsg = get_default_instantiate_msg(&deps.api);
    let round_lock_power_schedule =
        RoundLockPowerSchedule::new(instantiate_msg.clone().round_lock_power_schedule);

    let res = instantiate(deps.as_mut(), env.clone(), info, instantiate_msg.clone());
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
        proof: None,
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok());

    // simulate user locking 2000 tokens for 3 months, two days after the round started
    env.block.time = env.block.time.plus_days(1);

    // set validators for new round
    set_default_validator_for_rounds(deps.as_mut(), 0, 100);

    let second_lockup_amount: u128 = 2000;
    let info = get_message_info(
        &deps.api,
        user_address,
        &[Coin::new(second_lockup_amount, IBC_DENOM_1.to_string())],
    );
    let msg = ExecuteMsg::LockTokens {
        lock_duration: 3 * ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok());

    // Query specific lockup by ID using query_specific_user_lockups
    let res = query_specific_user_lockups(
        &deps.as_ref(),
        &env,
        info.sender.to_string(),
        vec![0], // Query only the first lockup
    );
    assert!(res.is_ok());
    let res = res.unwrap();
    assert_eq!(1, res.lockups.len());
    assert_eq!(0, res.lockups[0].lock_entry.lock_id);
    assert_eq!(
        first_lockup_amount,
        res.lockups[0].lock_entry.funds.amount.u128()
    );

    // Query for a non-existent lockup ID
    let res = query_specific_user_lockups(
        &deps.as_ref(),
        &env,
        info.sender.to_string(),
        vec![999], // Non-existent lockup ID
    );
    assert!(res.is_ok());
    let res = res.unwrap();
    assert!(res.lockups.is_empty());

    // add two proposals with different deployment durations

    // proposal 1 has a 1 month deployment duration
    let msg1 = ExecuteMsg::CreateProposal {
        round_id: None,
        tranche_id: 1,
        title: "proposal title 1".to_string(),
        description: "proposal description 1".to_string(),
        deployment_duration: 1,
        minimum_atom_liquidity_request: Uint128::zero(),
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg1.clone());
    assert!(res.is_ok());

    // proposal 2 has a 3 month deployment duration
    let msg2 = ExecuteMsg::CreateProposal {
        round_id: None,
        tranche_id: 1,
        title: "proposal title 2".to_string(),
        description: "proposal description 2".to_string(),
        deployment_duration: 3,
        minimum_atom_liquidity_request: Uint128::zero(),
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg2.clone());
    assert!(res.is_ok());

    // vote for proposal 1 with the first lockup and for proposal 2 with the second lockup
    // vote for the first proposal
    let msg = ExecuteMsg::Vote {
        tranche_id: 1,
        proposals_votes: vec![
            ProposalToLockups {
                proposal_id: 0,
                lock_ids: vec![0],
            },
            ProposalToLockups {
                proposal_id: 1,
                lock_ids: vec![1],
            },
        ],
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());

    // at this moment user doesn't have any expired lockups
    let expired_lockups = get_expired_user_lockups(&deps, env.clone(), info.sender.to_string());
    assert_eq!(0, expired_lockups.len());

    // but they should have 2 lockups
    let res = query_all_user_lockups_with_tranche_infos(
        &deps.as_ref(),
        &env,
        info.sender.to_string(),
        0,
        2000,
    );
    assert!(res.is_ok());
    let res = res.unwrap();

    assert_eq!(2, res.lockups_with_per_tranche_infos.len());
    assert_eq!(
        first_lockup_amount,
        res.lockups_with_per_tranche_infos[0]
            .lock_with_power
            .lock_entry
            .funds
            .amount
            .u128()
    );
    assert_eq!(
        second_lockup_amount,
        res.lockups_with_per_tranche_infos[1]
            .lock_with_power
            .lock_entry
            .funds
            .amount
            .u128()
    );

    // check that the voting powers match
    assert_eq!(
        first_lockup_amount,
        res.lockups_with_per_tranche_infos[0]
            .lock_with_power
            .current_voting_power
            .u128()
    );
    assert_eq!(
        // adjust for the 3 month lockup
        scale_lockup_power(
            &round_lock_power_schedule,
            ONE_MONTH_IN_NANO_SECONDS,
            3 * ONE_MONTH_IN_NANO_SECONDS,
            Uint128::new(second_lockup_amount),
        )
        .u128(),
        res.lockups_with_per_tranche_infos[1]
            .lock_with_power
            .current_voting_power
            .u128()
    );

    // check that the votes on the lockups are correct
    assert!(res.lockups_with_per_tranche_infos[0].per_tranche_info[0]
        .current_voted_on_proposal
        .is_some_and(|x| x == 0),);
    assert!(res.lockups_with_per_tranche_infos[1].per_tranche_info[0]
        .current_voted_on_proposal
        .is_some_and(|x| x == 1),);

    // Query specific lockup by ID using query_specific_user_lockups_with_tranche_infos
    let res = query_specific_user_lockups_with_tranche_infos(
        &deps.as_ref(),
        &env,
        info.sender.to_string(),
        vec![1], // Query only the second lockup
    );
    assert!(res.is_ok());
    let res = res.unwrap();
    assert_eq!(1, res.lockups_with_per_tranche_infos.len());
    assert_eq!(
        1,
        res.lockups_with_per_tranche_infos[0]
            .lock_with_power
            .lock_entry
            .lock_id
    );
    assert_eq!(
        second_lockup_amount,
        res.lockups_with_per_tranche_infos[0]
            .lock_with_power
            .lock_entry
            .funds
            .amount
            .u128()
    );

    let res = query_specific_user_lockups_with_tranche_infos(
        &deps.as_ref(),
        &env,
        info.sender.to_string(),
        vec![999], // Non-existent lockup ID
    );
    assert!(res.is_ok());
    let res = res.unwrap();
    assert!(res.lockups_with_per_tranche_infos.is_empty());

    // advance the chain for a month and verify that the first lockup has expired
    env.block.time = env.block.time.plus_nanos(ONE_MONTH_IN_NANO_SECONDS);
    let expired_lockups = get_expired_user_lockups(&deps, env.clone(), info.sender.to_string());
    assert_eq!(1, expired_lockups.len());
    assert_eq!(first_lockup_amount, expired_lockups[0].funds.amount.u128());

    // adjust the validator power ratios to check that they are reflected properly in the result
    let constants = load_current_constants(&deps.as_ref(), &env).unwrap();
    let current_round_id = compute_current_round_id(&env, &constants).unwrap();
    set_validator_power_ratio(
        deps.as_mut().storage,
        current_round_id,
        VALIDATOR_1,
        Decimal::percent(50),
    );

    let all_lockups = query_all_user_lockups_with_tranche_infos(
        &deps.as_ref(),
        &env,
        info.sender.to_string(),
        0,
        2000,
    );
    assert!(all_lockups.is_ok());

    let all_lockups = all_lockups.unwrap();
    assert_eq!(2, all_lockups.lockups_with_per_tranche_infos.len()); // still 2 lockups
    assert_eq!(
        first_lockup_amount,
        all_lockups.lockups_with_per_tranche_infos[0]
            .lock_with_power
            .lock_entry
            .funds
            .amount
            .u128()
    );
    assert_eq!(
        second_lockup_amount,
        all_lockups.lockups_with_per_tranche_infos[1]
            .lock_with_power
            .lock_entry
            .funds
            .amount
            .u128()
    );

    // check that the first lockup has power 0
    assert_eq!(
        0,
        all_lockups.lockups_with_per_tranche_infos[0]
            .lock_with_power
            .current_voting_power
            .u128()
    );

    // second lockup still has 2 months left, so has power
    assert_eq!(
        // adjust for the remaining 2 month lockup
        scale_lockup_power(
            &round_lock_power_schedule,
            ONE_MONTH_IN_NANO_SECONDS,
            2 * ONE_MONTH_IN_NANO_SECONDS,
            Uint128::new(second_lockup_amount),
        )
        .u128()
            / 2, // adjusted for the 50% power ratio,
        all_lockups.lockups_with_per_tranche_infos[1]
            .lock_with_power
            .current_voting_power
            .u128()
    );

    // check that neither lockup has voted on a proposal
    // check that the votes on the lockups are correct
    assert!(
        all_lockups.lockups_with_per_tranche_infos[0].per_tranche_info[0]
            .current_voted_on_proposal
            .is_none(),
    );
    assert!(
        all_lockups.lockups_with_per_tranche_infos[1].per_tranche_info[0]
            .current_voted_on_proposal
            .is_none(),
    );

    // check that the next voting rounds are correct
    assert_eq!(
        1,
        all_lockups.lockups_with_per_tranche_infos[0].per_tranche_info[0]
            .next_round_lockup_can_vote
    );
    assert_eq!(
        3,
        all_lockups.lockups_with_per_tranche_infos[1].per_tranche_info[0]
            .next_round_lockup_can_vote
    );

    // check that the lockups are tied to the right proposals
    assert_eq!(
        None,
        all_lockups.lockups_with_per_tranche_infos[0].per_tranche_info[0].tied_to_proposal
    );

    assert_eq!(
        1,
        all_lockups.lockups_with_per_tranche_infos[1].per_tranche_info[0]
            .tied_to_proposal
            .unwrap()
    );

    // advance the chain for 3 more months and verify that the second lockup has expired as well
    env.block.time = env.block.time.plus_nanos(3 * ONE_MONTH_IN_NANO_SECONDS);
    let expired_lockups = get_expired_user_lockups(&deps, env.clone(), info.sender.to_string());
    assert_eq!(2, expired_lockups.len());
    assert_eq!(first_lockup_amount, expired_lockups[0].funds.amount.u128());
    assert_eq!(second_lockup_amount, expired_lockups[1].funds.amount.u128());

    let all_lockups = query_all_user_lockups_with_tranche_infos(
        &deps.as_ref(),
        &env,
        info.sender.to_string(),
        0,
        2000,
    );

    assert!(all_lockups.is_ok());

    let all_lockups = all_lockups.unwrap();
    assert_eq!(2, all_lockups.lockups_with_per_tranche_infos.len()); // still 2 lockups
    assert_eq!(
        first_lockup_amount,
        all_lockups.lockups_with_per_tranche_infos[0]
            .lock_with_power
            .lock_entry
            .funds
            .amount
            .u128()
    );
    assert_eq!(
        second_lockup_amount,
        all_lockups.lockups_with_per_tranche_infos[1]
            .lock_with_power
            .lock_entry
            .funds
            .amount
            .u128()
    );

    // check that both lockups have 0 voting power
    assert_eq!(
        0,
        all_lockups.lockups_with_per_tranche_infos[0]
            .lock_with_power
            .current_voting_power
            .u128()
    );
    assert_eq!(
        0,
        all_lockups.lockups_with_per_tranche_infos[1]
            .lock_with_power
            .current_voting_power
            .u128()
    );

    // check that neither lockup is tied to a proposal
    assert_eq!(
        None,
        all_lockups.lockups_with_per_tranche_infos[0].per_tranche_info[0].tied_to_proposal
    );

    assert_eq!(
        None,
        all_lockups.lockups_with_per_tranche_infos[1].per_tranche_info[0].tied_to_proposal
    );

    // unlock the tokens and verify that the user doesn't have any expired lockups after that
    let msg = ExecuteMsg::UnlockTokens { lock_ids: None };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok());

    let expired_lockups = get_expired_user_lockups(&deps, env.clone(), info.sender.to_string());
    assert_eq!(0, expired_lockups.len());

    let all_lockups =
        query_all_user_lockups(&deps.as_ref(), &env, info.sender.to_string(), 0, 2000);
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
        proof: None,
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
        proof: None,
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

    // advance the chain for 1 month to start a new round
    env.block.time = env.block.time.plus_nanos(ONE_MONTH_IN_NANO_SECONDS);

    // set the validators for the new round, again
    set_default_validator_for_rounds(deps.as_mut(), 0, 100);

    // first lockup expires 29 days before the round 1 ends, and the second
    // lockup expires 1 month and 2 days after the round 1 ends, so the
    // expected voting power multipler is 0 for first lockup and 1.25 for second lockup
    let voting_power = get_user_voting_power(&deps, env.clone(), info.sender.to_string());
    let expected_voting_power = second_lockup_amount + (second_lockup_amount / 4);
    assert_eq!(expected_voting_power, voting_power);
}

#[test]
fn query_user_votes_test() {
    let env = mock_env();
    struct VoteToCreate {
        round_id: u64,
        tranche_id: u64,
        lock_id: u64,
        vote: Vote,
    }

    struct ValidatorInfoToCreate {
        round_id: u64,
        validator_info: ValidatorInfo,
    }

    struct TestCase {
        description: String,
        voter: Addr,
        votes_to_create: Vec<VoteToCreate>,
        validator_infos_to_create: Vec<ValidatorInfoToCreate>,
        expected_votes: StdResult<HashMap<u64, Decimal>>,
    }

    let round_id = 0;
    let tranche_id = 1;

    let deps = mock_dependencies(no_op_grpc_query_mock());
    let voter = deps.api.addr_make("addr0000");

    let first_proposal_id = 3;
    let second_proposal_id = 5;

    let test_cases = vec![
        TestCase {
            description: "votes with LSM shares from active round validators that were not slashed"
                .to_string(),
            voter: voter.clone(),
            votes_to_create: vec![
                VoteToCreate {
                    round_id,
                    tranche_id,
                    lock_id: 0,
                    vote: Vote {
                        prop_id: first_proposal_id,
                        time_weighted_shares: (
                            VALIDATOR_1.to_string(),
                            Decimal::from_ratio(500u128, Uint128::one()),
                        ),
                    },
                },
                VoteToCreate {
                    round_id,
                    tranche_id,
                    lock_id: 1,
                    vote: Vote {
                        prop_id: first_proposal_id,
                        time_weighted_shares: (
                            VALIDATOR_2.to_string(),
                            Decimal::from_ratio(300u128, Uint128::one()),
                        ),
                    },
                },
                VoteToCreate {
                    round_id,
                    tranche_id,
                    lock_id: 2,
                    vote: Vote {
                        prop_id: second_proposal_id,
                        time_weighted_shares: (
                            VALIDATOR_2.to_string(),
                            Decimal::from_ratio(700u128, Uint128::one()),
                        ),
                    },
                },
            ],
            validator_infos_to_create: vec![
                ValidatorInfoToCreate {
                    round_id,
                    validator_info: ValidatorInfo {
                        address: VALIDATOR_1.to_string(),
                        delegated_tokens: Uint128::zero(),
                        power_ratio: Decimal::from_str("1.0").unwrap(),
                    },
                },
                ValidatorInfoToCreate {
                    round_id,
                    validator_info: ValidatorInfo {
                        address: VALIDATOR_2.to_string(),
                        delegated_tokens: Uint128::zero(),
                        power_ratio: Decimal::from_str("1.0").unwrap(),
                    },
                },
            ],
            expected_votes: Ok(HashMap::from([
                (
                    first_proposal_id,
                    Decimal::from_ratio(800u128, Uint128::one()),
                ),
                (
                    second_proposal_id,
                    Decimal::from_ratio(700u128, Uint128::one()),
                ),
            ])),
        },
        TestCase {
            description:
                "votes with LSM shares from active round validators where some of them were slashed"
                    .to_string(),
            voter: voter.clone(),
            votes_to_create: vec![
                VoteToCreate {
                    round_id,
                    tranche_id,
                    lock_id: 0,
                    vote: Vote {
                        prop_id: first_proposal_id,
                        time_weighted_shares: (
                            VALIDATOR_1.to_string(),
                            Decimal::from_ratio(500u128, Uint128::one()),
                        ),
                    },
                },
                VoteToCreate {
                    round_id,
                    tranche_id,
                    lock_id: 1,
                    vote: Vote {
                        prop_id: first_proposal_id,
                        time_weighted_shares: (
                            VALIDATOR_2.to_string(),
                            Decimal::from_ratio(500u128, Uint128::one()),
                        ),
                    },
                },
                VoteToCreate {
                    round_id,
                    tranche_id,
                    lock_id: 2,
                    vote: Vote {
                        prop_id: second_proposal_id,
                        time_weighted_shares: (
                            VALIDATOR_2.to_string(),
                            Decimal::from_ratio(700u128, Uint128::one()),
                        ),
                    },
                },
            ],
            validator_infos_to_create: vec![
                ValidatorInfoToCreate {
                    round_id,
                    validator_info: ValidatorInfo {
                        address: VALIDATOR_1.to_string(),
                        delegated_tokens: Uint128::zero(),
                        power_ratio: Decimal::from_str("1.0").unwrap(),
                    },
                },
                ValidatorInfoToCreate {
                    round_id,
                    validator_info: ValidatorInfo {
                        address: VALIDATOR_2.to_string(),
                        delegated_tokens: Uint128::zero(),
                        power_ratio: Decimal::from_str("0.98").unwrap(),
                    },
                },
            ],
            expected_votes: Ok(HashMap::from([
                (
                    first_proposal_id,
                    Decimal::from_ratio(990u128, Uint128::one()),
                ),
                (
                    second_proposal_id,
                    Decimal::from_ratio(686u128, Uint128::one()),
                ),
            ])),
        },
        TestCase {
            description: "votes with LSM shares from only inactive round validators".to_string(),
            voter: voter.clone(),
            votes_to_create: vec![
                VoteToCreate {
                    round_id,
                    tranche_id,
                    lock_id: 0,
                    vote: Vote {
                        prop_id: first_proposal_id,
                        time_weighted_shares: (
                            VALIDATOR_1.to_string(),
                            Decimal::from_ratio(500u128, Uint128::one()),
                        ),
                    },
                },
                VoteToCreate {
                    round_id,
                    tranche_id,
                    lock_id: 1,
                    vote: Vote {
                        prop_id: first_proposal_id,
                        time_weighted_shares: (
                            VALIDATOR_2.to_string(),
                            Decimal::from_ratio(500u128, Uint128::one()),
                        ),
                    },
                },
            ],
            validator_infos_to_create: vec![ValidatorInfoToCreate {
                round_id,
                validator_info: ValidatorInfo {
                    address: VALIDATOR_3.to_string(),
                    delegated_tokens: Uint128::zero(),
                    power_ratio: Decimal::from_str("1.0").unwrap(),
                },
            }],
            expected_votes: Err(StdError::generic_err(
                "User didn't vote in the given round and tranche",
            )),
        },
        TestCase {
            description:
                "votes with LSM shares from some active and some inactive round validators"
                    .to_string(),
            voter: voter.clone(),
            votes_to_create: vec![
                VoteToCreate {
                    round_id,
                    tranche_id,
                    lock_id: 0,
                    vote: Vote {
                        prop_id: first_proposal_id,
                        time_weighted_shares: (
                            VALIDATOR_1.to_string(),
                            Decimal::from_ratio(500u128, Uint128::one()),
                        ),
                    },
                },
                VoteToCreate {
                    round_id,
                    tranche_id,
                    lock_id: 1,
                    vote: Vote {
                        prop_id: first_proposal_id,
                        time_weighted_shares: (
                            VALIDATOR_2.to_string(),
                            Decimal::from_ratio(300u128, Uint128::one()),
                        ),
                    },
                },
                VoteToCreate {
                    round_id,
                    tranche_id,
                    lock_id: 2,
                    vote: Vote {
                        prop_id: second_proposal_id,
                        time_weighted_shares: (
                            VALIDATOR_2.to_string(),
                            Decimal::from_ratio(700u128, Uint128::one()),
                        ),
                    },
                },
            ],
            validator_infos_to_create: vec![
                ValidatorInfoToCreate {
                    round_id,
                    validator_info: ValidatorInfo {
                        address: VALIDATOR_1.to_string(),
                        delegated_tokens: Uint128::zero(),
                        power_ratio: Decimal::from_str("0.95").unwrap(),
                    },
                },
                ValidatorInfoToCreate {
                    round_id,
                    validator_info: ValidatorInfo {
                        address: VALIDATOR_3.to_string(),
                        delegated_tokens: Uint128::zero(),
                        power_ratio: Decimal::from_str("1.0").unwrap(),
                    },
                },
            ],
            expected_votes: Ok(HashMap::from([(
                first_proposal_id,
                Decimal::from_ratio(475u128, Uint128::one()),
            )])),
        },
    ];

    for test_case in test_cases {
        println!("running test case: {}", test_case.description);
        let (mut deps, _env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());

        let height_range = HeightRange {
            lowest_known_height: 0,
            highest_known_height: env.block.height,
        };

        // We need to fill these stores or query_user_votes won't work
        ROUND_TO_HEIGHT_RANGE
            .save(&mut deps.storage, round_id, &height_range)
            .unwrap();

        SNAPSHOTS_ACTIVATION_HEIGHT
            .save(&mut deps.storage, &0)
            .unwrap();

        for vote_to_create in &test_case.votes_to_create {
            let mut lock_ids = USER_LOCKS
                .load(&deps.storage, test_case.voter.clone())
                .unwrap_or_default();
            if !lock_ids.contains(&vote_to_create.lock_id) {
                lock_ids.push(vote_to_create.lock_id);
                USER_LOCKS
                    .save(
                        &mut deps.storage,
                        test_case.voter.clone(),
                        &lock_ids,
                        env.block.height,
                    )
                    .unwrap();
            }
            let res = VOTE_MAP_V2.save(
                &mut deps.storage,
                (
                    (vote_to_create.round_id, vote_to_create.tranche_id),
                    vote_to_create.lock_id,
                ),
                &vote_to_create.vote,
            );
            assert!(res.is_ok(), "failed to save vote");
        }

        for validator_info_to_create in &test_case.validator_infos_to_create {
            let res = VALIDATORS_INFO.save(
                &mut deps.storage,
                (
                    validator_info_to_create.round_id,
                    validator_info_to_create.validator_info.address.clone(),
                ),
                &validator_info_to_create.validator_info,
            );
            assert!(res.is_ok(), "failed to save validator info");
        }

        let res = TOKEN_INFO_PROVIDERS.save(
            &mut deps.storage,
            LSM_TOKEN_INFO_PROVIDER_ID.to_string(),
            &get_default_lsm_token_info_provider(),
        );
        assert!(res.is_ok(), "failed to save token info provider");

        let res = query_user_votes(
            deps.as_ref(),
            round_id,
            tranche_id,
            test_case.voter.to_string(),
        );

        match test_case.expected_votes {
            Ok(expected_votes) => {
                assert!(res.is_ok(), "failed to get user votes");

                let user_votes = res.unwrap().votes;
                assert_eq!(
                    user_votes.len(),
                    expected_votes.len(),
                    "unexpected number of votes"
                );

                for user_vote in user_votes {
                    let expected_vote_power = expected_votes.get(&user_vote.prop_id);
                    assert!(
                        expected_vote_power.is_some(),
                        "query returned unexpected vote"
                    );

                    assert_eq!(user_vote.power, expected_vote_power.unwrap());
                }
            }
            Err(err) => {
                assert!(res.is_err(), "error expected but wasn't received");
                assert!(res.unwrap_err().to_string().contains(&err.to_string()));
            }
        }
    }
}

// Tests the `query_all_votes` function to ensure it correctly retrieves all stored votes,
// along with voter information, round ID, tranche ID, and lock ID.
#[test]
fn query_all_votes_test() {
    struct VoteToCreate {
        round_id: u64,
        tranche_id: u64,
        lock_id: u64,
        vote: Vote,
    }

    let round_id = 0;
    let tranche_id = 1;

    let mut deps = mock_dependencies(no_op_grpc_query_mock());
    let voter = deps.api.addr_make("addr0000");

    let first_proposal_id = 3;
    let second_proposal_id = 5;

    let start_from = 0;
    let limit = 10;

    let votes_to_create = vec![
        VoteToCreate {
            round_id,
            tranche_id,
            lock_id: 0,
            vote: Vote {
                prop_id: first_proposal_id,
                time_weighted_shares: (
                    VALIDATOR_1.to_string(),
                    Decimal::from_ratio(500u128, 1000u128),
                ),
            },
        },
        VoteToCreate {
            round_id,
            tranche_id,
            lock_id: 1,
            vote: Vote {
                prop_id: first_proposal_id,
                time_weighted_shares: (
                    VALIDATOR_2.to_string(),
                    Decimal::from_ratio(300u128, 1000u128),
                ),
            },
        },
        VoteToCreate {
            round_id,
            tranche_id,
            lock_id: 2,
            vote: Vote {
                prop_id: second_proposal_id,
                time_weighted_shares: (
                    VALIDATOR_2.to_string(),
                    Decimal::from_ratio(700u128, 1000u128),
                ),
            },
        },
    ];
    let env = mock_env();

    for vote_to_create in &votes_to_create {
        let res_lock = LOCKS_MAP_V2.save(
            &mut deps.storage,
            vote_to_create.lock_id,
            &LockEntryV2 {
                lock_id: vote_to_create.lock_id,
                funds: Coin::new(Uint128::from(1000u128), IBC_DENOM_1.to_string()),
                owner: voter.clone(),
                lock_start: Timestamp::from_seconds(10),
                lock_end: Timestamp::from_seconds(100),
            },
            env.block.height,
        );
        assert!(res_lock.is_ok(), "failed to save lock");
        let res = VOTE_MAP_V2.save(
            &mut deps.storage,
            (
                (vote_to_create.round_id, vote_to_create.tranche_id),
                vote_to_create.lock_id,
            ),
            &vote_to_create.vote,
        );
        assert!(res.is_ok(), "failed to save vote");
    }

    let res = query_all_votes(deps.as_ref(), start_from, limit);
    assert!(res.is_ok());
    let response = res.unwrap();
    assert_eq!(response.votes.len(), votes_to_create.len());

    for (i, vote_to_create) in votes_to_create.iter().enumerate() {
        let expected_vote = VoteEntry {
            round_id: vote_to_create.round_id,
            tranche_id: vote_to_create.tranche_id,
            sender_addr: voter.clone(),
            lock_id: vote_to_create.lock_id,
            vote: vote_to_create.vote.clone(),
        };
        assert_eq!(response.votes[i], expected_vote, "Mismatch at index {i}");
    }
}

// Tests the `query_all_votes_round_tranche` function to ensure it correctly retrieves votes alongside metadata
// for a specific round ID and tranche ID.
#[test]
fn query_all_votes_round_tranche_test() {
    struct VoteToCreate {
        round_id: u64,
        tranche_id: u64,
        lock_id: u64,
        vote: Vote,
    }

    let round_id = 1;
    let tranche_id = 1;

    let target_round_id = 2;
    let target_tranche_id = 2;

    let mut deps = mock_dependencies(no_op_grpc_query_mock());
    let voter = deps.api.addr_make("addr0000");

    let first_proposal_id = 3;
    let second_proposal_id = 5;

    let start_from = 0;
    let limit = 10;

    let votes_to_create = vec![
        VoteToCreate {
            round_id,
            tranche_id,
            lock_id: 0,
            vote: Vote {
                prop_id: first_proposal_id,
                time_weighted_shares: (
                    VALIDATOR_1.to_string(),
                    Decimal::from_ratio(500u128, 1000u128),
                ),
            },
        },
        VoteToCreate {
            round_id: target_round_id,
            tranche_id: target_tranche_id,
            lock_id: 1,
            vote: Vote {
                prop_id: first_proposal_id,
                time_weighted_shares: (
                    VALIDATOR_2.to_string(),
                    Decimal::from_ratio(300u128, 1000u128),
                ),
            },
        },
        VoteToCreate {
            round_id,
            tranche_id,
            lock_id: 2,
            vote: Vote {
                prop_id: second_proposal_id,
                time_weighted_shares: (
                    VALIDATOR_2.to_string(),
                    Decimal::from_ratio(700u128, 1000u128),
                ),
            },
        },
    ];
    let env = mock_env();

    for vote_to_create in &votes_to_create {
        let res_lock = LOCKS_MAP_V2.save(
            &mut deps.storage,
            vote_to_create.lock_id,
            &LockEntryV2 {
                lock_id: vote_to_create.lock_id,
                funds: Coin::new(Uint128::from(1000u128), IBC_DENOM_1.to_string()),
                owner: voter.clone(),
                lock_start: Timestamp::from_seconds(10),
                lock_end: Timestamp::from_seconds(100),
            },
            env.block.height,
        );
        assert!(res_lock.is_ok(), "failed to save lock");
        let res = VOTE_MAP_V2.save(
            &mut deps.storage,
            (
                (vote_to_create.round_id, vote_to_create.tranche_id),
                vote_to_create.lock_id,
            ),
            &vote_to_create.vote,
        );
        assert!(res.is_ok(), "failed to save vote");
    }

    let res = query_all_votes_round_tranche(
        deps.as_ref(),
        target_round_id,
        target_tranche_id,
        start_from,
        limit,
    );
    assert!(res.is_ok());
    let response = res.unwrap();
    assert_eq!(response.votes.len(), 1);

    // Expected vote
    let expected_vote = VoteEntry {
        round_id: target_round_id,
        tranche_id: target_tranche_id,
        sender_addr: voter.clone(), // Replace if sender is stored elsewhere
        lock_id: 1,
        vote: Vote {
            prop_id: first_proposal_id,
            time_weighted_shares: (
                VALIDATOR_2.to_string(),
                Decimal::from_ratio(300u128, 1000u128),
            ),
        },
    };

    // Assert that the response contains the expected vote
    assert_eq!(response.votes[0], expected_vote);
}

fn get_expired_user_lockups(
    deps: &OwnedDeps<MockStorage, MockApi, MockQuerier, NeutronQuery>,
    env: Env,
    user_address: String,
) -> Vec<LockEntryV2> {
    let res = query_expired_user_lockups(&deps.as_ref(), &env, user_address.to_string(), 0, 2000);
    assert!(res.is_ok());
    let res = res.unwrap();

    res.lockups
}

fn get_user_voting_power(
    deps: &OwnedDeps<MockStorage, MockApi, MockQuerier, NeutronQuery>,
    env: Env,
    user_address: String,
) -> u128 {
    let res = query_user_voting_power(deps.as_ref(), env, user_address.to_string());
    assert!(res.is_ok());

    res.unwrap().voting_power
}
#[test]
fn query_lock_votes_history_test() {
    let user_address = "addr0000";
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
    );
    let (mut deps, mut env) = (mock_dependencies(grpc_query), mock_env());
    let info = get_message_info(&deps.api, user_address, &[]);

    let mut instantiate_msg = get_default_instantiate_msg(&deps.api);
    instantiate_msg.round_length = ONE_MONTH_IN_NANO_SECONDS; // Set the round length to 1 month, to facilitate the tests when we advance 1 round
    instantiate_msg.whitelist_admins = vec![get_address_as_str(&deps.api, user_address)];
    instantiate(deps.as_mut(), env.clone(), info.clone(), instantiate_msg).unwrap();

    set_default_validator_for_rounds(deps.as_mut(), 0, 2);

    // Create some test locks
    let lock_funds = vec![Coin {
        denom: IBC_DENOM_1.to_string(),
        amount: Uint128::from(1000u128),
    }];

    // Create message infos beforehand to avoid borrowing issues
    let lock_info = get_message_info(&deps.api, user_address, &lock_funds);
    let proposal_info = get_message_info(&deps.api, user_address, &[]);
    let vote_info = get_message_info(&deps.api, user_address, &[]);

    // Create additional tranche (default instantiation creates tranche ID 1)
    execute(
        deps.as_mut(),
        env.clone(),
        proposal_info.clone(),
        ExecuteMsg::AddTranche {
            tranche: TrancheInfo {
                name: "Tranche 2".to_string(),
                metadata: "Second tranche".to_string(),
            },
        },
    )
    .unwrap();

    // We advance 1 nano second before locking, so we have correct voting power
    env.block.time = env.block.time.plus_nanos(1);

    // Lock tokens (creates lock_id 0)
    execute(
        deps.as_mut(),
        env.clone(),
        lock_info.clone(),
        ExecuteMsg::LockTokens {
            lock_duration: 3 * ONE_MONTH_IN_NANO_SECONDS, // 1500 VP at round 0, 1250 VP at round 1
            proof: None,
        },
    )
    .unwrap();

    // Lock more tokens (creates lock_id 1)
    execute(
        deps.as_mut(),
        env.clone(),
        lock_info.clone(),
        ExecuteMsg::LockTokens {
            lock_duration: 2 * ONE_MONTH_IN_NANO_SECONDS, // 1250 VP at round 0, 1000 VP at round 1
            proof: None,
        },
    )
    .unwrap();

    // Create some proposals
    execute(
        deps.as_mut(),
        env.clone(),
        proposal_info.clone(),
        ExecuteMsg::CreateProposal {
            round_id: None,
            tranche_id: 1,
            title: "Test Proposal 1".to_string(),
            description: "First test proposal".to_string(),
            deployment_duration: 1,
            minimum_atom_liquidity_request: Uint128::from(100u128),
        },
    )
    .unwrap();

    execute(
        deps.as_mut(),
        env.clone(),
        proposal_info.clone(),
        ExecuteMsg::CreateProposal {
            round_id: None,
            tranche_id: 2,
            title: "Test Proposal 2".to_string(),
            description: "Second test proposal".to_string(),
            deployment_duration: 1,
            minimum_atom_liquidity_request: Uint128::from(200u128),
        },
    )
    .unwrap();

    // Vote with lock_id 0 for proposal 0 in tranche 1
    execute(
        deps.as_mut(),
        env.clone(),
        vote_info.clone(),
        ExecuteMsg::Vote {
            tranche_id: 1,
            proposals_votes: vec![ProposalToLockups {
                proposal_id: 0,
                lock_ids: vec![0],
            }],
        },
    )
    .unwrap();

    // Vote with lock_id 1 for proposal 1 in tranche 2
    execute(
        deps.as_mut(),
        env.clone(),
        vote_info.clone(),
        ExecuteMsg::Vote {
            tranche_id: 2,
            proposals_votes: vec![ProposalToLockups {
                proposal_id: 1,
                lock_ids: vec![1],
            }],
        },
    )
    .unwrap();

    // Advance to next round to create more voting history
    let constants = load_current_constants(&deps.as_ref(), &env).unwrap();
    env.block.time = env.block.time.plus_nanos(ONE_MONTH_IN_NANO_SECONDS);
    let current_round_id = compute_current_round_id(&env, &constants).unwrap();
    assert_eq!(current_round_id, 1);

    set_default_validator_for_rounds(deps.as_mut(), 1, 2);

    // Create another proposal in current round (ID 1)
    execute(
        deps.as_mut(),
        env.clone(),
        proposal_info.clone(),
        ExecuteMsg::CreateProposal {
            round_id: None,
            tranche_id: 1,
            title: "Test Proposal 3".to_string(),
            description: "Third test proposal".to_string(),
            deployment_duration: 1,
            minimum_atom_liquidity_request: Uint128::from(100u128),
        },
    )
    .unwrap();

    // Vote with lock_id 0 again in round 1
    execute(
        deps.as_mut(),
        env.clone(),
        vote_info.clone(),
        ExecuteMsg::Vote {
            tranche_id: 1,
            proposals_votes: vec![ProposalToLockups {
                proposal_id: 2,
                lock_ids: vec![0],
            }],
        },
    )
    .unwrap();

    // Test: Query lock votes history for lock_id 0 (should have votes in rounds 0 and 1)
    let res = crate::contract::query_lock_votes_history(
        deps.as_ref(),
        env.clone(),
        0, // lock_id
        None,
        None,
        None, // no filters
    )
    .unwrap();

    // Should have 2 votes (round 0 and round 1, both in tranche 1, both for proposal 0)
    println!("{:?}", res.vote_history);
    assert_eq!(res.vote_history.len(), 2);

    // Check first vote (round 0) - VP 1500
    let vote1 = &res.vote_history[0];
    assert_eq!(vote1.round_id, 0);
    assert_eq!(vote1.tranche_id, 1);
    assert_eq!(vote1.proposal_id, 0);
    assert_eq!(vote1.vote_power, Decimal::from_ratio(1500u128, 1u128));

    // Check second vote (round 1) - VP 1250
    let vote2 = &res.vote_history[1];
    assert_eq!(vote2.round_id, 1);
    assert_eq!(vote2.tranche_id, 1);
    assert_eq!(vote2.proposal_id, 2);
    assert_eq!(vote2.vote_power, Decimal::from_ratio(1250u128, 1u128));

    // Test: Query lock votes history for lock_id 1 (should have vote only in round 0)
    let res = crate::contract::query_lock_votes_history(
        deps.as_ref(),
        env.clone(),
        1, // lock_id
        None,
        None,
        None, // no filters
    )
    .unwrap();

    // Should have 1 vote (round 0, tranche 2, proposal 1)
    assert_eq!(res.vote_history.len(), 1);
    let vote = &res.vote_history[0];
    assert_eq!(vote.round_id, 0);
    assert_eq!(vote.tranche_id, 2);
    assert_eq!(vote.proposal_id, 1);
    assert!(vote.vote_power > Decimal::zero());

    // Test: Query with round filter (only round 0)
    let res = crate::contract::query_lock_votes_history(
        deps.as_ref(),
        env.clone(),
        0,       // lock_id
        Some(0), // start_from_round_id
        Some(0), // stop_at_round_id (inclusive)
        None,    // no tranche filter
    )
    .unwrap();

    // Should have 1 vote (only round 0)
    assert_eq!(res.vote_history.len(), 1);
    assert_eq!(res.vote_history[0].round_id, 0);

    // Test: Query with tranche filter (only tranche 1)
    let res = crate::contract::query_lock_votes_history(
        deps.as_ref(),
        env.clone(),
        0, // lock_id
        None,
        None,    // no round filters
        Some(1), // tranche_id filter
    )
    .unwrap();

    // Should have 2 votes (both in tranche 1)
    assert_eq!(res.vote_history.len(), 2);
    for vote in &res.vote_history {
        assert_eq!(vote.tranche_id, 1);
    }

    // Test: Query for lock 2
    let res = crate::contract::query_lock_votes_history(
        deps.as_ref(),
        env.clone(),
        1, // lock_id
        None,
        None, // no round filters
        None, // tranche_id filter (lock 1 never voted in tranche 1)
    )
    .unwrap();

    // Should have 1 vote (tranche ID 2)
    assert_eq!(res.vote_history.len(), 1);
    assert_eq!(res.vote_history[0].round_id, 0);
    assert_eq!(res.vote_history[0].tranche_id, 2);

    // Test: Query with tranche filter for lock that didn't vote in that tranche
    let res = crate::contract::query_lock_votes_history(
        deps.as_ref(),
        env.clone(),
        1, // lock_id
        None,
        None,    // no round filters
        Some(1), // tranche_id filter (lock 1 never voted in tranche 1)
    )
    .unwrap();

    // Should have 0 votes
    assert_eq!(res.vote_history.len(), 0);

    // Test: Query non-existent lock
    let res = crate::contract::query_lock_votes_history(
        deps.as_ref(),
        env.clone(),
        999, // non-existent lock_id
        None,
        None,
        None,
    )
    .unwrap();

    // Should have 0 votes
    assert_eq!(res.vote_history.len(), 0);

    // Test: Invalid round range (start > end)
    let res = crate::contract::query_lock_votes_history(
        deps.as_ref(),
        env.clone(),
        0,       // lock_id
        Some(2), // start_from_round_id
        Some(1), // stop_at_round_id (smaller than start)
        None,
    );

    // Should return error
    assert!(res.is_err());
    assert!(res
        .unwrap_err()
        .to_string()
        .contains("start_round (2) must be less than or equal to end_round (1)"));
}

#[test]
fn simulate_dtoken_amounts() {
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-1".to_string(),
        HashMap::from([
            (IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string()),
            (IBC_DENOM_2.to_string(), VALIDATOR_2_LST_DENOM_1.to_string()),
        ]),
    );
    let (mut deps, mut env) = (mock_dependencies(grpc_query), mock_env());
    let user_address = deps.api.addr_make("addr0000");
    let info = get_message_info(&deps.api, user_address.as_ref(), &[]);

    let mut instantiate_msg: crate::msg::InstantiateMsg = get_default_instantiate_msg(&deps.api);
    instantiate_msg.token_info_providers[0] = TokenInfoProviderInstantiateMsg::LSM {
        max_validator_shares_participating: 100,
        hub_connection_id: "connection-0".to_string(),
        hub_transfer_channel_id: "channel-1".to_string(),
        icq_update_period: 100,
    };

    let res = instantiate(deps.as_mut(), env.clone(), info, instantiate_msg.clone());
    assert!(res.is_ok());

    // simulate user locking 1000 tokens for 1 month, one day after the round started
    env.block.time = env.block.time.plus_days(1);

    set_default_validator_for_rounds(deps.as_mut(), 0, 100);

    let first_lockup_amount: u128 = 1000;
    let info = get_message_info(
        &deps.api,
        user_address.as_ref(),
        &[Coin::new(first_lockup_amount, IBC_DENOM_1.to_string())],
    );
    let msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok());

    let ids: Vec<u64> = vec![1, 2, 3];

    USER_LOCKS
        .save(
            &mut deps.storage,
            user_address.clone(),
            &ids,
            env.block.height,
        )
        .unwrap();

    // simulate user locking 2000 tokens for 3 months, two days after the round started
    env.block.time = env.block.time.plus_days(1);

    // set validators for new round
    set_default_validator_for_rounds(deps.as_mut(), 0, 100);

    let second_lockup_amount: u128 = 2000;
    let info = get_message_info(
        &deps.api,
        user_address.as_ref(),
        &[Coin::new(second_lockup_amount, IBC_DENOM_1.to_string())],
    );
    let msg = ExecuteMsg::LockTokens {
        lock_duration: 3 * ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok());

    let res_lock = LOCKS_MAP_V2.save(
        &mut deps.storage,
        1,
        &LockEntryV2 {
            lock_id: 1,
            funds: Coin::new(Uint128::from(1000u128), IBC_DENOM_1.to_string()),
            owner: user_address.clone(),
            lock_start: Timestamp::from_seconds(10),
            lock_end: Timestamp::from_seconds(100),
        },
        env.block.height,
    );
    assert!(res_lock.is_ok(), "failed to save lock");

    let res_lock_2 = LOCKS_MAP_V2.save(
        &mut deps.storage,
        2,
        &LockEntryV2 {
            lock_id: 2,
            funds: Coin::new(Uint128::from(2000u128), IBC_DENOM_1.to_string()),
            owner: user_address.clone(),
            lock_start: Timestamp::from_seconds(10),
            lock_end: Timestamp::from_seconds(100),
        },
        env.block.height,
    );
    assert!(res_lock_2.is_ok(), "failed to save lock");

    let res_lock_3 = LOCKS_MAP_V2.save(
        &mut deps.storage,
        3,
        &LockEntryV2 {
            lock_id: 3,
            funds: Coin::new(Uint128::from(2000u128), IBC_DENOM_2.to_string()),
            owner: user_address.clone(),
            lock_start: Timestamp::from_seconds(10),
            lock_end: Timestamp::from_seconds(100),
        },
        env.block.height,
    );
    assert!(res_lock_3.is_ok(), "failed to save lock");

    let drop_address = deps.api.addr_make("drop");
    let puppeteer_address = deps.api.addr_make("puppeteer");
    let d_token_denom =
        "factory/neutron1k6hr0f83e7un2wjf29cspk7j69jrnskk65k3ek2nj9dztrlzpj6q00rtsa/udatom"
            .to_string();

    let drop_token_info = DropTokenInfo {
        address: drop_address,
        d_token_denom,
        puppeteer_address,
    };

    let drop_contract = DROP_TOKEN_INFO.save(&mut deps.storage, &drop_token_info);
    assert!(drop_contract.is_ok(), "failed to save drop contract info");

    let current_ratio = Decimal::from_str("1.15").unwrap();

    let mock_puppeteer_response = DelegationsResponse {
        delegations: Delegations {
            delegations: vec![
                DropDelegation {
                    delegator: Addr::unchecked(
                        "cosmos1srjdd7y6duuukmaenghucasqlycddcc65qdj34k6spq8pwk4h6ms7j4w4j",
                    ),
                    validator: "cosmosvaloper196ax4vc0lwpxndu9dyhvca7jhxp70rmcvrj90c".to_string(),
                    amount: Coin {
                        denom: "uatom".to_string(),
                        amount: 96_581_728_232u128.into(),
                    },
                    share_ratio: Decimal256::from_str("0.999800011043535397").unwrap(),
                },
                DropDelegation {
                    delegator: Addr::unchecked(
                        "cosmos1srjdd7y6duuukmaenghucasqlycddcc65qdj34k6spq8pwk4h6ms7j4w4j",
                    ),
                    validator: "cosmosvaloper157v7tczs40axfgejp2m43kwuzqe0wsy0rv8puv".to_string(),
                    amount: Coin {
                        denom: "uatom".to_string(),
                        amount: 706_769_074_698u128.into(),
                    },
                    share_ratio: Decimal256::from_str("1").unwrap(),
                },
            ],
        },
        remote_height: 26227986,
        local_height: 27676281,
        timestamp: Timestamp::from_nanos(1750217292399260448),
    };
    deps.querier
        .update_wasm(drop_mock(current_ratio, mock_puppeteer_response));

    let res =
        query_simulate_dtoken_amounts(&deps.as_ref(), vec![1, 2, 3], user_address.to_string());
    assert!(res.is_ok());
    let res = res.unwrap();
    assert_eq!(res.dtokens_response.len(), 3);
    assert_eq!(res.dtokens_response[0].dtoken_amount, "869".to_string());
    assert_eq!(res.dtokens_response[1].dtoken_amount, "1739".to_string());
    assert!(res.dtokens_response[2].dtoken_amount.contains("error"));
}
pub fn drop_mock(
    current_ratio: Decimal,
    puppeteer_response: DelegationsResponse,
) -> Box<WasmQueryFunc> {
    Box::new(move |req| match req {
        WasmQuery::Smart {
            contract_addr: _,
            msg,
        } => {
            // First try DropQueryMsg::ExchangeRate
            if let Ok(DropQueryMsg::ExchangeRate {}) = from_json(msg) {
                return SystemResult::Ok(ContractResult::Ok(
                    to_json_binary(&current_ratio).unwrap(),
                ));
            }

            // Then try PuppeteerQueryMsg::Extension::Delegations
            if let Ok(PuppeteerQueryMsg::Extension {
                msg: QueryExtMsg::Delegations {},
            }) = from_json(msg)
            {
                return SystemResult::Ok(ContractResult::Ok(
                    to_json_binary(&puppeteer_response).unwrap(),
                ));
            }

            SystemResult::Err(SystemError::Unknown {})
        }
        _ => SystemResult::Err(SystemError::Unknown {}),
    })
}
