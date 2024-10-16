use std::collections::HashMap;

use crate::contract::{
    query_tranches, query_user_vote, query_whitelist, query_whitelist_admins, MAX_LOCK_ENTRIES,
};
use crate::msg::TrancheInfo;
use crate::testing_lsm_integration::set_validator_infos_for_round;
use crate::testing_mocks::{
    denom_trace_grpc_query_mock, mock_dependencies, no_op_grpc_query_mock, MockQuerier,
};
use crate::{
    contract::{
        compute_current_round_id, execute, instantiate, query_all_user_lockups, query_constants,
        query_proposal, query_round_total_power, query_round_tranche_proposals,
        query_top_n_proposals,
    },
    msg::{ExecuteMsg, InstantiateMsg},
};
use cosmwasm_std::testing::{mock_env, MockApi, MockStorage};
use cosmwasm_std::{BankMsg, CosmosMsg, Deps, DepsMut, MessageInfo, OwnedDeps, Timestamp, Uint128};
use cosmwasm_std::{Coin, StdError, StdResult};
use neutron_sdk::bindings::query::NeutronQuery;
use proptest::prelude::*;

pub const VALIDATOR_1: &str = "cosmosvaloper157v7tczs40axfgejp2m43kwuzqe0wsy0rv8puv";
pub const VALIDATOR_2: &str = "cosmosvaloper140l6y2gp3gxvay6qtn70re7z2s0gn57zfd832j";
pub const VALIDATOR_3: &str = "cosmosvaloper14upntdx8lf0f49t987mj99zksxnluanvu6x4lu";

pub const VALIDATOR_1_LST_DENOM_1: &str =
    "cosmosvaloper157v7tczs40axfgejp2m43kwuzqe0wsy0rv8puv/789";
pub const VALIDATOR_2_LST_DENOM_1: &str =
    "cosmosvaloper140l6y2gp3gxvay6qtn70re7z2s0gn57zfd832j/34205";
pub const VALIDATOR_3_LST_DENOM_1: &str =
    "cosmosvaloper14upntdx8lf0f49t987mj99zksxnluanvu6x4lu/13608";

// To get all IBC denom traces on some chain use:
//      BINARY q ibc-transfer denom-traces --node NODE_RPC
// Then find some denom trace whose base_denom is LST token and to obtain IBC denom use:
//      BINARY q ibc-transfer denom-hash PATH/BASE_DENOM --node NODE_RPC
// Note: the following IBC denoms do not match specific LST tokens on Neutron. They are just an arbitrary IBC denoms.
pub const IBC_DENOM_1: &str =
    "ibc/0EA38305D72BE22FD87E7C0D1002D36D59B59BC3C863078A54550F8E50C50EEE";
pub const IBC_DENOM_2: &str =
    "ibc/0BADD323A0FE849BCF0034BA8329771737EB54F2B6EA6F314A80520366338CFC";
pub const IBC_DENOM_3: &str =
    "ibc/0A5935F2493A9B8DE23899C4D30842B3E3DD69A147388D010F3C9BAA6D6C6D37";

pub const ONE_DAY_IN_NANO_SECONDS: u64 = 24 * 60 * 60 * 1000000000;
pub const TWO_WEEKS_IN_NANO_SECONDS: u64 = 14 * 24 * 60 * 60 * 1000000000;
pub const ONE_MONTH_IN_NANO_SECONDS: u64 = 2629746000000000; // 365 days / 12
pub const THREE_MONTHS_IN_NANO_SECONDS: u64 = 3 * ONE_MONTH_IN_NANO_SECONDS;

pub fn set_default_validator_for_rounds(
    deps: DepsMut<NeutronQuery>,
    start_round: u64,
    end_round: u64,
) {
    for round_id in start_round..end_round {
        let res =
            set_validator_infos_for_round(deps.storage, round_id, vec![VALIDATOR_1.to_string()]);
        assert!(res.is_ok());
    }
}

pub fn get_default_instantiate_msg(mock_api: &MockApi) -> InstantiateMsg {
    let user_address = get_address_as_str(mock_api, "addr0000");

    InstantiateMsg {
        round_length: TWO_WEEKS_IN_NANO_SECONDS,
        lock_epoch_length: ONE_MONTH_IN_NANO_SECONDS,
        tranches: vec![TrancheInfo {
            name: "tranche 1".to_string(),
            metadata: "tranche 1 metadata".to_string(),
        }],
        first_round_start: mock_env().block.time,
        max_locked_tokens: Uint128::new(1000000),
        initial_whitelist: vec![user_address.clone()],
        whitelist_admins: vec![],
        max_validator_shares_participating: 100,
        hub_connection_id: "connection-0".to_string(),
        hub_transfer_channel_id: "channel-0".to_string(),
        icq_update_period: 100,
        icq_managers: vec![user_address],
        is_in_pilot_mode: false,
    }
}

pub fn get_message_info(mock_api: &MockApi, sender: &str, funds: &[Coin]) -> MessageInfo {
    MessageInfo {
        sender: mock_api.addr_make(sender),
        funds: funds.to_vec(),
    }
}

pub fn get_address_as_str(mock_api: &MockApi, addr: &str) -> String {
    mock_api.addr_make(addr).to_string()
}

#[test]
fn instantiate_test() {
    let (mut deps, env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());
    let info = get_message_info(&deps.api, "addr0000", &[]);

    let msg = get_default_instantiate_msg(&deps.api);

    let res = instantiate(deps.as_mut(), env, info, msg.clone());
    assert!(res.is_ok());

    let res = query_constants(deps.as_ref());
    assert!(res.is_ok());

    let constants = res.unwrap().constants;
    assert_eq!(msg.round_length, constants.round_length);
}

#[test]
fn deduplicate_whitelist_admins_test() {
    let (mut deps, env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());
    let info = get_message_info(&deps.api, "addr0000", &[]);
    let mut msg = get_default_instantiate_msg(&deps.api);
    let admin_address_1 = get_address_as_str(&deps.api, "admin3");
    let admin_address_2 = get_address_as_str(&deps.api, "admin2");

    msg.initial_whitelist = vec![
        admin_address_1.clone(),
        admin_address_2.clone(),
        admin_address_1.clone(),
    ];

    msg.whitelist_admins = vec![
        admin_address_1.clone(),
        admin_address_2.clone(),
        admin_address_1.clone(),
    ];
    let res = instantiate(deps.as_mut(), env, info, msg);
    assert!(res.is_ok());
    let whitelist = query_whitelist(deps.as_ref()).unwrap().whitelist;
    let whitelist_admins = query_whitelist_admins(deps.as_ref()).unwrap().admins;

    assert_eq!(whitelist.len(), 2);
    assert_eq!(whitelist[0].as_str(), admin_address_1);
    assert_eq!(whitelist[1].as_str(), admin_address_2);

    assert_eq!(whitelist_admins.len(), 2);
    assert_eq!(whitelist_admins[0].as_str(), admin_address_1);
    assert_eq!(whitelist_admins[1].as_str(), admin_address_2);
}

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
    };
    let res = execute(deps.as_mut(), env.clone(), info1.clone(), msg);
    assert!(res.is_ok(), "error: {:?}", res);

    let info2 = get_message_info(
        &deps.api,
        user_address,
        &[Coin::new(3000u64, IBC_DENOM_1.to_string())],
    );
    let msg = ExecuteMsg::LockTokens {
        lock_duration: THREE_MONTHS_IN_NANO_SECONDS,
    };
    let res = execute(deps.as_mut(), env.clone(), info2.clone(), msg);
    assert!(res.is_ok());

    let res = query_all_user_lockups(deps.as_ref(), env.clone(), info.sender.to_string(), 0, 2000);
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
    assert_eq!(2, res.messages.len());

    // check that all messages are BankMsg::Send
    for msg in res.messages.iter() {
        match msg.msg.clone() {
            CosmosMsg::Bank(bank_msg) => match bank_msg {
                BankMsg::Send { to_address, amount } => {
                    assert_eq!(info.sender.to_string(), *to_address);
                    assert_eq!(1, amount.len());
                    assert_eq!(user_token.denom, amount[0].denom);
                    assert_eq!(user_token.amount.u128(), amount[0].amount.u128());
                }
                _ => panic!("expected BankMsg::Send message"),
            },
            _ => panic!("expected CosmosMsg::Bank msg"),
        }
    }
}

#[test]
fn create_proposal_basic_test() {
    let user_address = "addr0000";
    let user_token = Coin::new(1000u64, IBC_DENOM_1.to_string());

    let (mut deps, env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());
    let info = get_message_info(&deps.api, user_address, &[user_token.clone()]);
    let msg = get_default_instantiate_msg(&deps.api);

    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());

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

    let expected_round_id = 0;
    let res = query_round_tranche_proposals(deps.as_ref(), expected_round_id, 1, 0, 3000);
    assert!(res.is_ok(), "error: {:?}", res);

    let res = res.unwrap();
    assert_eq!(2, res.proposals.len());

    let proposal = &res.proposals[0];
    assert_eq!(expected_round_id, proposal.round_id);
    assert_eq!(0, proposal.power.u128());

    let proposal = &res.proposals[1];
    assert_eq!(expected_round_id, proposal.round_id);
    assert_eq!(0, proposal.power.u128());

    // assert that the proposals are not added to top N proposals
    // immediately upon creation, as their voting power is 0
    let res = query_top_n_proposals(deps.as_ref(), expected_round_id, 1, 2);
    assert!(res.is_ok(), "error: {:?}", res);

    let res = res.unwrap();
    assert_eq!(0, res.proposals.len());
}

#[test]
fn vote_basic_test() {
    vote_test_with_start_time(mock_env().block.time, 0);
}

// If user already voted for some proposal, and then locks new tokens or refreshes the existing lock,
// the voting power on those proposals should get updated accordingly.
#[test]
fn proposal_power_change_on_lock_and_refresh_test() {
    let user_address = "addr0000";
    let user_token1 = Coin::new(1000u64, IBC_DENOM_1.to_string());
    let user_token2 = Coin::new(1000u64, IBC_DENOM_2.to_string());

    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([
            (IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string()),
            (IBC_DENOM_2.to_string(), VALIDATOR_2_LST_DENOM_1.to_string()),
        ]),
    );
    let (mut deps, mut env) = (mock_dependencies(grpc_query), mock_env());
    let info = get_message_info(&deps.api, user_address, &[user_token1.clone()]);

    let mut msg = get_default_instantiate_msg(&deps.api);
    msg.lock_epoch_length = TWO_WEEKS_IN_NANO_SECONDS;
    // add another tranche
    msg.tranches.push(TrancheInfo {
        name: "tranche 2".to_string(),
        metadata: "tranche 2 metadata".to_string(),
    });

    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());

    let res = set_validator_infos_for_round(
        deps.as_mut().storage,
        0,
        vec![VALIDATOR_1.to_string(), VALIDATOR_2.to_string()],
    );
    assert!(res.is_ok());

    // advance the chain by 1000 nano seconds to simulate locking during the round
    env.block.time = env.block.time.plus_nanos(1000);

    let msg = ExecuteMsg::LockTokens {
        lock_duration: TWO_WEEKS_IN_NANO_SECONDS,
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok());

    let prop_infos = vec![
        (
            1,
            "proposal title 1".to_string(),
            "proposal description 1".to_string(),
        ),
        (
            2,
            "proposal title 2".to_string(),
            "proposal description 2".to_string(),
        ),
        (
            2,
            "proposal title 3".to_string(),
            "proposal description 3".to_string(),
        ),
    ];

    for prop_info in prop_infos {
        let msg = ExecuteMsg::CreateProposal {
            tranche_id: prop_info.0,
            title: prop_info.1,
            description: prop_info.2,
        };

        let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone());
        assert!(res.is_ok());
    }

    let first_round_id = 0;
    let second_round_id = 1;

    let first_proposal_id = 0;
    let second_proposal_id = 1;
    let third_proposal_id = 2;
    let fourth_proposal_id = 3;

    let first_tranche_id = 1;
    let second_tranche_id = 2;

    // lock additional 1000 tokens before voting and verify this has no effect on proposals power
    let msg = ExecuteMsg::LockTokens {
        lock_duration: TWO_WEEKS_IN_NANO_SECONDS,
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok());

    let mut expected_voting_power = 0u128;
    assert_proposal_voting_power(
        &deps,
        first_round_id,
        first_tranche_id,
        first_proposal_id,
        expected_voting_power,
    );

    assert_proposal_voting_power(
        &deps,
        first_round_id,
        second_tranche_id,
        second_proposal_id,
        expected_voting_power,
    );

    assert_proposal_voting_power(
        &deps,
        first_round_id,
        second_tranche_id,
        third_proposal_id,
        expected_voting_power,
    );

    // vote for the first proposal in tranche 1
    let msg = ExecuteMsg::Vote {
        tranche_id: first_tranche_id,
        proposal_id: first_proposal_id,
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());

    // verify users vote for the first proposal in tranche 1
    expected_voting_power = 2000u128;

    let res = query_user_vote(
        deps.as_ref(),
        first_round_id,
        first_tranche_id,
        info.sender.to_string(),
    );
    assert!(res.is_ok(), "error: {:?}", res);
    assert_eq!(first_proposal_id, res.unwrap().vote.prop_id);

    assert_proposal_voting_power(
        &deps,
        first_round_id,
        first_tranche_id,
        first_proposal_id,
        expected_voting_power,
    );

    // vote for the second proposal in tranche 2
    let msg = ExecuteMsg::Vote {
        tranche_id: second_tranche_id,
        proposal_id: second_proposal_id,
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());

    // verify users vote for the second proposal in tranche 2
    let res = query_user_vote(
        deps.as_ref(),
        first_round_id,
        second_tranche_id,
        info.sender.to_string(),
    );
    assert!(res.is_ok(), "error: {:?}", res);
    assert_eq!(second_proposal_id, res.unwrap().vote.prop_id);

    assert_proposal_voting_power(
        &deps,
        first_round_id,
        second_tranche_id,
        second_proposal_id,
        expected_voting_power,
    );

    // verify that the proposal that user didn't vote for is unaffected
    assert_proposal_voting_power(
        &deps,
        first_round_id,
        second_tranche_id,
        third_proposal_id,
        0,
    );

    // lock additional 1000 tokens and verify that the voting power gets updated on both proposals
    let msg = ExecuteMsg::LockTokens {
        lock_duration: TWO_WEEKS_IN_NANO_SECONDS,
    };
    // lock LSM token that user already locked before
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok());

    expected_voting_power = 3000u128;

    // verify that the voting power increased for the first proposal
    assert_proposal_voting_power(
        &deps,
        first_round_id,
        first_tranche_id,
        first_proposal_id,
        expected_voting_power,
    );

    // verify that the voting power increased for the second proposal
    assert_proposal_voting_power(
        &deps,
        first_round_id,
        second_tranche_id,
        second_proposal_id,
        expected_voting_power,
    );

    // verify that the proposal that user didn't vote for is unaffected
    assert_proposal_voting_power(
        &deps,
        first_round_id,
        second_tranche_id,
        third_proposal_id,
        0,
    );

    // lock 1000 of a different LSM token
    let info = get_message_info(&deps.api, user_address, &[user_token2.clone()]);
    let msg = ExecuteMsg::LockTokens {
        lock_duration: TWO_WEEKS_IN_NANO_SECONDS,
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok());

    expected_voting_power = 4000u128;

    // verify that the voting power increased for the first proposal
    assert_proposal_voting_power(
        &deps,
        first_round_id,
        first_tranche_id,
        first_proposal_id,
        expected_voting_power,
    );

    // verify that the voting power increased for the second proposal
    assert_proposal_voting_power(
        &deps,
        first_round_id,
        second_tranche_id,
        second_proposal_id,
        expected_voting_power,
    );

    // verify that the proposal that user didn't vote for is unaffected
    assert_proposal_voting_power(
        &deps,
        first_round_id,
        second_tranche_id,
        third_proposal_id,
        0,
    );

    let first_lockup_id = 0;
    let second_lockup_id = 1;

    // refresh first lockup
    let msg = ExecuteMsg::RefreshLockDuration {
        lock_ids: vec![first_lockup_id],
        lock_duration: 3 * TWO_WEEKS_IN_NANO_SECONDS,
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok());

    expected_voting_power = 4500u128;

    // verify that the voting power increased for the first proposal
    assert_proposal_voting_power(
        &deps,
        first_round_id,
        first_tranche_id,
        first_proposal_id,
        expected_voting_power,
    );

    // verify that the voting power increased for the second proposal
    assert_proposal_voting_power(
        &deps,
        first_round_id,
        second_tranche_id,
        second_proposal_id,
        expected_voting_power,
    );

    // verify that the proposal that user didn't vote for is unaffected
    assert_proposal_voting_power(
        &deps,
        first_round_id,
        second_tranche_id,
        third_proposal_id,
        0,
    );

    // advance the chain by two weeks to move to the next round
    env.block.time = env.block.time.plus_nanos(TWO_WEEKS_IN_NANO_SECONDS);

    // create a new proposal in this round
    let msg = ExecuteMsg::CreateProposal {
        tranche_id: first_tranche_id,
        title: "proposal title 4".to_string(),
        description: "proposal description 4".to_string(),
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());

    // vote for the fourth proposal in tranche 1
    let msg = ExecuteMsg::Vote {
        tranche_id: first_tranche_id,
        proposal_id: fourth_proposal_id,
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());

    // verify users vote for the fourth proposal in tranche 1
    expected_voting_power = 1250u128;

    let res = query_user_vote(
        deps.as_ref(),
        second_round_id,
        first_tranche_id,
        info.sender.to_string(),
    );
    assert!(res.is_ok(), "error: {:?}", res);
    assert_eq!(fourth_proposal_id, res.unwrap().vote.prop_id);

    assert_proposal_voting_power(
        &deps,
        second_round_id,
        first_tranche_id,
        fourth_proposal_id,
        expected_voting_power,
    );

    // refresh second (expired) lockup
    let msg = ExecuteMsg::RefreshLockDuration {
        lock_ids: vec![second_lockup_id],
        lock_duration: 3 * TWO_WEEKS_IN_NANO_SECONDS,
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok());

    expected_voting_power = 2750u128;

    // verify that the voting power increased for the fourth proposal
    assert_proposal_voting_power(
        &deps,
        second_round_id,
        first_tranche_id,
        fourth_proposal_id,
        expected_voting_power,
    );
}

#[test]
fn past_start_time_test() {
    // check behaviour starting one round before the start
    vote_test_with_start_time(
        // make the first round start slightly more than one epoch length in the past
        mock_env()
            .block
            .time
            .minus_nanos(TWO_WEEKS_IN_NANO_SECONDS + ONE_DAY_IN_NANO_SECONDS),
        1,
    );

    // check behaviour starting with the first round not done yet
    vote_test_with_start_time(
        // make the first round start slightly less than one epoch length in the past
        mock_env()
            .block
            .time
            .minus_nanos(TWO_WEEKS_IN_NANO_SECONDS - ONE_DAY_IN_NANO_SECONDS),
        0, // round_id should be 0 because we are still during the first round
    );

    // check behaviour starting in round 100
    vote_test_with_start_time(
        // make the first round start slightly more than 100 epochs in the past
        mock_env()
            .block
            .time
            .minus_nanos(TWO_WEEKS_IN_NANO_SECONDS * 100 + ONE_DAY_IN_NANO_SECONDS),
        100,
    );
}

// Locks tokens, creates two proposals, then votes for one, and switches the vote to the other.
// It will set the start time of the contract to the specified time, and will use the specified
// round id to query proposals and votes.
fn vote_test_with_start_time(start_time: Timestamp, current_round_id: u64) {
    let user_address = "addr0000";
    let user_token = Coin::new(1000u64, IBC_DENOM_1.to_string());

    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
    );
    let (mut deps, mut env) = (mock_dependencies(grpc_query), mock_env());
    let info = get_message_info(&deps.api, user_address, &[user_token.clone()]);
    let mut msg = get_default_instantiate_msg(&deps.api);
    msg.first_round_start = start_time;

    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());

    set_default_validator_for_rounds(deps.as_mut(), 0, 100);

    // lock some tokens to get voting power
    let msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok());

    let prop_infos = vec![
        (
            1,
            "proposal title 1".to_string(),
            "proposal description 1".to_string(),
        ),
        (
            1,
            "proposal title 2".to_string(),
            "proposal description 2".to_string(),
        ),
    ];

    for prop_info in prop_infos {
        let msg = ExecuteMsg::CreateProposal {
            tranche_id: prop_info.0,
            title: prop_info.1,
            description: prop_info.2,
        };

        let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone());
        assert!(res.is_ok());
    }

    // vote for the first proposal
    let first_proposal_id = 0;
    let msg = ExecuteMsg::Vote {
        tranche_id: 1,
        proposal_id: first_proposal_id,
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());

    // verify users vote for the first proposal
    let round_id = current_round_id;
    let tranche_id = 1;

    let res = query_user_vote(deps.as_ref(), round_id, tranche_id, info.sender.to_string());
    assert!(res.is_ok(), "error: {:?}", res);
    assert_eq!(first_proposal_id, res.unwrap().vote.prop_id);

    let res = query_proposal(deps.as_ref(), round_id, tranche_id, first_proposal_id);
    assert!(res.is_ok());
    assert_eq!(
        info.funds[0].amount.u128(),
        res.unwrap().proposal.power.u128()
    );

    // switch vote to the second proposal
    let second_proposal_id = 1;
    let msg = ExecuteMsg::Vote {
        tranche_id: 1,
        proposal_id: second_proposal_id,
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok(), "error: {:?}", res);

    // verify users vote for the second proposal
    let res = query_user_vote(deps.as_ref(), round_id, tranche_id, info.sender.to_string());
    assert!(res.is_ok());
    assert_eq!(second_proposal_id, res.unwrap().vote.prop_id);

    let res = query_proposal(deps.as_ref(), round_id, tranche_id, second_proposal_id);
    assert!(res.is_ok());
    assert_eq!(
        info.funds[0].amount.u128(),
        res.unwrap().proposal.power.u128()
    );

    // verify that the vote for the first proposal was removed
    let res = query_proposal(deps.as_ref(), round_id, tranche_id, first_proposal_id);
    assert!(res.is_ok());
    assert_eq!(0, res.unwrap().proposal.power.u128());

    // advance the chain by two weeks + 1 nano second to move to the next round and try to unlock tokens
    env.block.time = env.block.time.plus_nanos(TWO_WEEKS_IN_NANO_SECONDS + 1);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::UnlockTokens {},
    );

    // user voted for a proposal in previous round, but can unlock tokens
    assert!(res.is_ok());
}

#[test]
fn multi_tranches_test() {
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
    );
    let (mut deps, env) = (mock_dependencies(grpc_query), mock_env());
    let info = get_message_info(
        &deps.api,
        "addr0000",
        &[Coin::new(1000u64, IBC_DENOM_1.to_string())],
    );
    let mut msg = get_default_instantiate_msg(&deps.api);
    msg.tranches = vec![
        TrancheInfo {
            name: "tranche 1".to_string(),
            metadata: "tranche 1 metadata".to_string(),
        },
        TrancheInfo {
            name: "tranche 2".to_string(),
            metadata: "tranche 2 metadata".to_string(),
        },
    ];

    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());

    set_default_validator_for_rounds(deps.as_mut(), 0, 100);

    // create two proposals for tranche 1
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

    // create two proposals for tranche 2
    let msg3 = ExecuteMsg::CreateProposal {
        tranche_id: 2,
        title: "proposal title 3".to_string(),
        description: "proposal description 3".to_string(),
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg3.clone());
    assert!(res.is_ok());

    let msg4 = ExecuteMsg::CreateProposal {
        tranche_id: 2,
        title: "proposal title 4".to_string(),
        description: "proposal description 4".to_string(),
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
    let info2 = get_message_info(
        &deps.api,
        "addr0001",
        &[Coin::new(2000u64, IBC_DENOM_1.to_string())],
    );
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
    let info3 = get_message_info(
        &deps.api,
        "addr0002",
        &[Coin::new(1u64, IBC_DENOM_1.to_string())],
    );
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
    assert!(
        res.is_ok(),
        "error when querying top n proposals: {:?}",
        res
    );
    let res = res.unwrap().proposals;
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
    let res = res.unwrap().proposals;
    // check that there are two proposals
    assert_eq!(2, res.len(), "expected 2 proposals, got {:?}", res);
    // check that the voting power of the first proposal is 3000
    assert_eq!(3000, res[0].power.u128());
    // check that the voting power of the second proposal is 0
    assert_eq!(1, res[1].power.u128());
}

#[test]
fn test_query_round_tranche_proposals_pagination() {
    let (mut deps, env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());
    let info = get_message_info(
        &deps.api,
        "addr0000",
        &[Coin::new(1000u64, IBC_DENOM_1.to_string())],
    );
    let msg = get_default_instantiate_msg(&deps.api);

    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());

    // Create multiple proposals
    let num_proposals = 5;
    for i in 0..num_proposals {
        let create_proposal_msg = ExecuteMsg::CreateProposal {
            tranche_id: 1,
            title: format!("proposal title {}", i),
            description: format!("proposal description {}", i),
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
                proposal.title,
                format!("proposal title {}", *expected_proposal)
            );
        }
    }
}

#[test]
fn duplicate_tranche_name_test() {
    // try to instantiate the contract with two tranches with the same name
    // this should fail
    let (mut deps, env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());
    let info = get_message_info(&deps.api, "addr0000", &[]);
    let mut msg = get_default_instantiate_msg(&deps.api);
    msg.tranches = vec![
        TrancheInfo {
            name: "tranche 1".to_string(),
            metadata: "tranche 1 metadata".to_string(),
        },
        TrancheInfo {
            name: "tranche 1".to_string(),
            metadata: "tranche 2 metadata".to_string(),
        },
    ];

    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_err());
    assert!(res
        .unwrap_err()
        .to_string()
        .to_lowercase()
        .contains("duplicate tranche name"));
}

#[test]
fn add_edit_tranche_test() {
    let (mut deps, env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());
    let admin_info = get_message_info(&deps.api, "addr0000", &[]);
    let mut msg = get_default_instantiate_msg(&deps.api);
    msg.tranches = vec![
        TrancheInfo {
            name: "tranche 1".to_string(),
            metadata: "tranche 1 metadata".to_string(),
        },
        TrancheInfo {
            name: "tranche 2".to_string(),
            metadata: "tranche 2 metadata".to_string(),
        },
    ];
    msg.whitelist_admins = vec![get_address_as_str(&deps.api, "addr0000")];

    let res = instantiate(deps.as_mut(), env.clone(), admin_info.clone(), msg);
    assert!(res.is_ok(), "error: {:?}", res);

    let tranches = query_tranches(deps.as_ref());
    assert_eq!(tranches.unwrap().tranches.len(), 2);

    // verify that only whitelist admins can add new tranches
    let non_admin_info = get_message_info(&deps.api, "addr0001", &[]);
    let msg = ExecuteMsg::AddTranche {
        tranche: TrancheInfo {
            name: "tranche 2".to_string(),
            metadata: "tranche 2 metadata".to_string(),
        },
    };

    let res = execute(deps.as_mut(), env.clone(), non_admin_info.clone(), msg);
    assert!(res.is_err());
    assert!(res
        .unwrap_err()
        .to_string()
        .to_lowercase()
        .contains("unauthorized"));

    // verify that the new tranche name must be unique
    let msg = ExecuteMsg::AddTranche {
        tranche: TrancheInfo {
            name: "tranche 2".to_string(),
            metadata: "tranche 3 metadata".to_string(),
        },
    };

    let res = execute(deps.as_mut(), env.clone(), admin_info.clone(), msg);
    assert!(res.is_err());
    assert!(res
        .unwrap_err()
        .to_string()
        .to_lowercase()
        .contains("tranche with the given name already exists"));

    // verify that a valid new tranche can be added
    let new_tranche_name = String::from("tranche 3");
    let new_tranche_metadata = String::from("tranche 3 metadata");

    let msg = ExecuteMsg::AddTranche {
        tranche: TrancheInfo {
            name: new_tranche_name.clone(),
            metadata: new_tranche_metadata.clone(),
        },
    };

    let res = execute(deps.as_mut(), env.clone(), admin_info.clone(), msg);
    assert!(res.is_ok());

    let tranches = query_tranches(deps.as_ref()).unwrap().tranches;
    assert_eq!(tranches.len(), 3);

    let new_tranche = tranches[2].clone();
    assert_eq!(new_tranche.id, 3);
    assert_eq!(new_tranche.name, new_tranche_name);
    assert_eq!(new_tranche.metadata, new_tranche_metadata);

    // verify that only whitelist admins can edit tranches
    let msg = ExecuteMsg::EditTranche {
        tranche_id: 3,
        tranche_name: Some("tranche 3".to_string()),
        tranche_metadata: Some("tranche 3 metadata".to_string()),
    };

    let res = execute(deps.as_mut(), env.clone(), non_admin_info, msg.clone());
    assert!(res.is_err());
    assert!(res
        .unwrap_err()
        .to_string()
        .to_lowercase()
        .contains("unauthorized"));

    // verify that tranche name and metadata gets updated
    let updated_tranche_name = "tranche 3 updated".to_string();
    let updated_tranche_metadata = "tranche 3 metadata updated".to_string();
    let msg = ExecuteMsg::EditTranche {
        tranche_id: 3,
        tranche_name: Some(updated_tranche_name.clone()),
        tranche_metadata: Some(updated_tranche_metadata.clone()),
    };

    let res = execute(deps.as_mut(), env.clone(), admin_info.clone(), msg);
    assert!(res.is_ok());

    let tranches = query_tranches(deps.as_ref()).unwrap().tranches;
    assert_eq!(tranches.len(), 3);

    let updated_tranche = tranches[2].clone();
    assert_eq!(updated_tranche.id, 3);
    assert_eq!(updated_tranche.name, updated_tranche_name);
    assert_eq!(updated_tranche.metadata, updated_tranche_metadata);
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
        let mut deps = mock_dependencies(no_op_grpc_query_mock());
        let mut msg = get_default_instantiate_msg(&deps.api);
        msg.round_length = round_length;
        msg.first_round_start = Timestamp::from_nanos(contract_start_time);

        let mut env = mock_env();
        env.block.time = Timestamp::from_nanos(contract_start_time);
        let info = get_message_info(&deps.api, "addr0000", &[]);
        let _ = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone()).unwrap();

        // set the time to the current time
        env.block.time = Timestamp::from_nanos(current_time);

        let constants = query_constants(deps.as_ref());
        assert!(constants.is_ok());

        let round_id = compute_current_round_id(&env, &constants.unwrap().constants);
        assert_eq!(expected_round_id, round_id);
    }
}

#[test]
fn total_voting_power_tracking_test() {
    let user_address = "addr0000";

    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
    );
    let (mut deps, mut env) = (mock_dependencies(grpc_query), mock_env());
    let info = get_message_info(&deps.api, user_address, &[]);
    let mut msg = get_default_instantiate_msg(&deps.api);

    // align round length with lock epoch length for easier calculations
    msg.round_length = ONE_MONTH_IN_NANO_SECONDS;

    let res = instantiate(deps.as_mut(), env.clone(), info, msg.clone());
    assert!(res.is_ok());

    set_default_validator_for_rounds(deps.as_mut(), 0, 100);

    let info1 = get_message_info(
        &deps.api,
        user_address,
        &[Coin::new(10u64, IBC_DENOM_1.to_string())],
    );
    let msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
    };
    let res = execute(deps.as_mut(), env.clone(), info1.clone(), msg);
    assert!(res.is_ok());

    // user locks 10 tokens for one month, so it will have 1x voting power in the first round only
    let expected_total_voting_powers = [(0, 10), (1, 0)];
    verify_expected_voting_power(deps.as_ref(), &expected_total_voting_powers);

    // advance the chain by 10 days and have user lock more tokens
    env.block.time = env.block.time.plus_nanos(10 * ONE_DAY_IN_NANO_SECONDS);

    let info2 = get_message_info(
        &deps.api,
        user_address,
        &[Coin::new(20u64, IBC_DENOM_1.to_string())],
    );
    let msg = ExecuteMsg::LockTokens {
        lock_duration: THREE_MONTHS_IN_NANO_SECONDS,
    };
    let res = execute(deps.as_mut(), env.clone(), info2.clone(), msg);
    assert!(res.is_ok());

    // user locks 20 additional tokens for three months, so the expectation is:
    // round:         0      1       2       3
    // power:       10+30   0+25    0+20    0+0
    let expected_total_voting_powers = [(0, 40), (1, 25), (2, 20), (3, 0)];
    verify_expected_voting_power(deps.as_ref(), &expected_total_voting_powers);

    // advance the chain by 25 more days to move to round 1 and have user refresh second lockup to 6 months
    env.block.time = env.block.time.plus_nanos(25 * ONE_DAY_IN_NANO_SECONDS);

    let info3 = get_message_info(&deps.api, user_address, &[]);
    let msg = ExecuteMsg::RefreshLockDuration {
        lock_ids: vec![1],
        lock_duration: 2 * THREE_MONTHS_IN_NANO_SECONDS,
    };
    let res = execute(deps.as_mut(), env.clone(), info3.clone(), msg);
    assert!(res.is_ok());

    // user relocks second lockup worth 20 tokens for six months in round 1, so the expectation is (note that round 0 is not affected):
    // round:         0       1       2       3       4       5       6       7
    // power:       10+30    0+40    0+40    0+40    0+30    0+25    0+20    0+0
    let expected_total_voting_powers = [
        (0, 40),
        (1, 40),
        (2, 40),
        (3, 40),
        (4, 30),
        (5, 25),
        (6, 20),
        (7, 0),
    ];
    verify_expected_voting_power(deps.as_ref(), &expected_total_voting_powers);

    // advance the chain by 5 more days and have user lock 50 more tokens for three months
    env.block.time = env.block.time.plus_nanos(5 * ONE_DAY_IN_NANO_SECONDS);

    let info2 = get_message_info(
        &deps.api,
        user_address,
        &[Coin::new(50u64, IBC_DENOM_1.to_string())],
    );
    let msg = ExecuteMsg::LockTokens {
        lock_duration: THREE_MONTHS_IN_NANO_SECONDS,
    };
    let res = execute(deps.as_mut(), env.clone(), info2.clone(), msg);
    assert!(res.is_ok());

    // user locks 50 additional tokens in round 1 for three months, so the expectation is (note that round 0 is not affected):
    // round:         0        1          2          3          4         5         6         7
    // power:       10+30    0+40+75    0+40+62    0+40+50    0+30+0    0+25+0    0+20+0    0+0+0
    let expected_total_voting_powers = [
        (0, 40),
        (1, 115),
        (2, 102),
        (3, 90),
        (4, 30),
        (5, 25),
        (6, 20),
        (7, 0),
    ];
    verify_expected_voting_power(deps.as_ref(), &expected_total_voting_powers);
}

fn verify_expected_voting_power(deps: Deps<NeutronQuery>, expected_powers: &[(u64, u128)]) {
    for expected_power in expected_powers {
        let res = query_round_total_power(deps, expected_power.0);

        assert!(res.is_ok());
        let res = res.unwrap();
        assert_eq!(expected_power.1, res.total_voting_power.u128());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))] // set the number of test cases to run
    #[test]
    fn relock_proptest(old_lock_remaining_time: u64, new_lock_duration: u8) {
        let grpc_query = denom_trace_grpc_query_mock(
            "transfer/channel-0".to_string(),
            HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
        );

        let (mut deps, mut env) = (
            mock_dependencies(grpc_query),
            mock_env(),
        );
        let info = get_message_info(&deps.api, "addr0001", &[Coin::new(1000u64, IBC_DENOM_1.to_string())]);
        let msg = get_default_instantiate_msg(&deps.api);

        let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
        assert!(res.is_ok());

        set_default_validator_for_rounds(deps.as_mut(), 0, 100);

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
        let info2 = get_message_info(&deps.api, "addr0002", &[]);
        let msg = ExecuteMsg::RefreshLockDuration {
            lock_ids: vec![0],
            lock_duration: new_lock_duration,
        };
        let res = execute(deps.as_mut(), env.clone(), info2.clone(), msg);

        // different user cannot refresh the lock
        assert!(res.is_err(), "different user should not be able to refresh the lock: {:?}", res);

        // refresh the lock duration
        let info = get_message_info(&deps.api, "addr0001", &[]);
        let msg = ExecuteMsg::RefreshLockDuration {
            lock_ids: vec![0],
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
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), lock_msg.clone());
    assert!(res.is_ok());

    // advance the chain by one month plus one nanosecond and unlock the first lockup
    env.block.time = env.block.time.plus_nanos(ONE_MONTH_IN_NANO_SECONDS + 1);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::UnlockTokens {},
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

    // a privileged user can update the maximum allowed locked tokens
    info = get_message_info(&deps.api, "addr0001", &[]);
    let update_max_locked_tokens_msg = ExecuteMsg::UpdateMaxLockedTokens {
        max_locked_tokens: 3000,
    };
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        update_max_locked_tokens_msg,
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
}

#[test]
fn contract_pausing_test() {
    let (mut deps, env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());
    let mut info = get_message_info(&deps.api, "addr0000", &[]);

    let whitelist_admin = "addr0001";
    let mut msg = get_default_instantiate_msg(&deps.api);
    msg.whitelist_admins = vec![get_address_as_str(&deps.api, whitelist_admin)];

    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());

    // verify that non-privileged user can not pause the contract
    let msg = ExecuteMsg::Pause {};
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_err());
    assert!(res.unwrap_err().to_string().contains("Unauthorized"));

    // verify that privileged user can pause the contract
    info = get_message_info(&deps.api, whitelist_admin, &[]);
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());

    let constants = query_constants(deps.as_ref());
    assert!(constants.is_ok());
    assert!(constants.unwrap().constants.paused);

    // verify that no action can be executed while the contract is paused
    let msgs = vec![
        ExecuteMsg::LockTokens { lock_duration: 0 },
        ExecuteMsg::RefreshLockDuration {
            lock_ids: vec![0],
            lock_duration: 0,
        },
        ExecuteMsg::UnlockTokens {},
        ExecuteMsg::CreateProposal {
            tranche_id: 0,
            title: "".to_string(),
            description: "".to_string(),
        },
        ExecuteMsg::Vote {
            tranche_id: 0,
            proposal_id: 0,
        },
        ExecuteMsg::AddAccountToWhitelist {
            address: whitelist_admin.to_string(),
        },
        ExecuteMsg::RemoveAccountFromWhitelist {
            address: whitelist_admin.to_string(),
        },
        ExecuteMsg::UpdateMaxLockedTokens {
            max_locked_tokens: 0,
        },
        ExecuteMsg::Pause {},
        ExecuteMsg::AddTranche {
            tranche: TrancheInfo {
                name: String::new(),
                metadata: String::new(),
            },
        },
        ExecuteMsg::EditTranche {
            tranche_id: 1,
            tranche_name: Some(String::new()),
            tranche_metadata: Some(String::new()),
        },
        ExecuteMsg::CreateICQsForValidators { validators: vec![] },
        ExecuteMsg::AddICQManager {
            address: whitelist_admin.to_string(),
        },
        ExecuteMsg::RemoveICQManager {
            address: whitelist_admin.to_string(),
        },
        ExecuteMsg::WithdrawICQFunds {
            amount: Uint128::new(50),
        },
    ];

    for msg in msgs {
        let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone());
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("Paused"));
    }
}

// This test verifies that only whitelisted addresses can submit proposals
#[test]
pub fn whitelist_proposal_submission_test() {
    let (mut deps, env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());
    let mut info = get_message_info(&deps.api, "addr0000", &[]);

    let whitelist_admin = "addr0001";
    let mut msg = get_default_instantiate_msg(&deps.api);
    msg.whitelist_admins = vec![get_address_as_str(&deps.api, whitelist_admin)];

    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());

    // try to submit a proposal with a non-whitelisted address
    info = get_message_info(&deps.api, "addr0002", &[]);
    let proposal_msg = ExecuteMsg::CreateProposal {
        tranche_id: 1,
        title: "proposal title".to_string(),
        description: "proposal description".to_string(),
    };

    let res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        proposal_msg.clone(),
    );
    // ensure we get an error
    assert!(res.is_err());
    assert!(res.unwrap_err().to_string().contains("Unauthorized"));

    // ensure there is no proposal
    let res = query_proposal(deps.as_ref(), 0, 1, 0);
    assert!(res.is_err());

    // try to submit a proposal with a whitelisted address
    info = get_message_info(&deps.api, "addr0000", &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        proposal_msg.clone(),
    );
    assert!(res.is_ok(), "error: {:?}", res);

    // now, the proposal should exist
    let res = query_proposal(deps.as_ref(), 0, 1, 0);
    assert!(res.is_ok(), "error: {:?}", res);

    // add the first sender to the whitelist
    info = get_message_info(&deps.api, whitelist_admin, &[]);
    let msg = ExecuteMsg::AddAccountToWhitelist {
        address: get_address_as_str(&deps.api, "addr0002"),
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok(), "error: {:?}", res);

    // now, try to submit the proposal again as the first sender
    info = get_message_info(&deps.api, "addr0002", &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        proposal_msg.clone(),
    );
    assert!(res.is_ok(), "error: {:?}", res);

    // now, there should be a second proposal (with id 1)
    let res = query_proposal(deps.as_ref(), 0, 1, 1);
    assert!(res.is_ok(), "error: {:?}", res);
}

fn assert_proposal_voting_power(
    deps: &OwnedDeps<MockStorage, MockApi, MockQuerier, NeutronQuery>,
    round_id: u64,
    tranche_id: u64,
    proposal_id: u64,
    expected_voting_power: u128,
) {
    let res = query_proposal(deps.as_ref(), round_id, tranche_id, proposal_id);
    assert!(res.is_ok());
    assert_eq!(expected_voting_power, res.unwrap().proposal.power.u128());
}

// This test verifies that when the contract is in pilot mode,
// the possible lock durations are restricted to the durations allowed during
// pilot rounds (just 1 round in this case).
#[test]
pub fn pilot_round_lock_duration_test() {
    struct TestCase {
        lock_duration: u64,
        expect_error: bool,
    }

    let test_cases = vec![
        TestCase {
            lock_duration: ONE_MONTH_IN_NANO_SECONDS,
            expect_error: false,
        },
        TestCase {
            lock_duration: ONE_MONTH_IN_NANO_SECONDS * 2,
            expect_error: true,
        },
        TestCase {
            lock_duration: ONE_MONTH_IN_NANO_SECONDS * 3,
            expect_error: true,
        },
        TestCase {
            lock_duration: ONE_MONTH_IN_NANO_SECONDS * 6,
            expect_error: true,
        },
        TestCase {
            lock_duration: ONE_MONTH_IN_NANO_SECONDS * 12,
            expect_error: true,
        },
    ];

    for case in test_cases {
        let grpc_query = denom_trace_grpc_query_mock(
            "transfer/channel-0".to_string(),
            HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
        );
        let (mut deps, env) = (mock_dependencies(grpc_query), mock_env());
        let mut info: MessageInfo = get_message_info(&deps.api, "addr0000", &[]);

        let whitelist_admin = "addr0001";
        let mut msg = get_default_instantiate_msg(&deps.api);
        msg.whitelist_admins = vec![get_address_as_str(&deps.api, whitelist_admin)];
        msg.is_in_pilot_mode = true;
        msg.round_length = ONE_DAY_IN_NANO_SECONDS;
        msg.lock_epoch_length = ONE_MONTH_IN_NANO_SECONDS;

        let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
        assert!(res.is_ok());

        set_default_validator_for_rounds(deps.as_mut(), 0, 100);

        // try to lock tokens for the specified duration
        info = get_message_info(
            &deps.api,
            "addr0000",
            &[Coin::new(1000u64, IBC_DENOM_1.to_string())],
        );

        let lock_msg = ExecuteMsg::LockTokens {
            lock_duration: case.lock_duration,
        };

        let res = execute(deps.as_mut(), env.clone(), info.clone(), lock_msg.clone());

        if case.expect_error {
            assert!(
                res.is_err(),
                "Expected error for lock_duration: {}",
                case.lock_duration
            );
            assert!(res
                .err()
                .unwrap()
                .to_string()
                .contains("Lock duration must be 1 epoch"),);
        } else {
            assert!(
                res.is_ok(),
                "Expected success for lock_duration: {}; error: {}",
                case.lock_duration,
                res.err().unwrap()
            );
        }
    }
}

struct TestCase {
    name: &'static str,
    lock_ids: Vec<u64>,
    new_lock_duration: u64,
    expected_error: Option<String>,
    // expected_new_lock_durations is a list of tuples, where the first element is the sender address,
    // and the second element is a list of the expected remaining lock durations for the locks
    expected_new_lock_durations: Vec<(String, Vec<u64>)>,
}

// This test checks the behaviour when refreshing multiple locks at once.
// It creates multiple locks in different rounds and then tries to refresh subsets of them.
// It checks:
// * a case where multiple locks are successfully refreshed together
// * a case where one of the locks that are being refreshed would get shorter, so this case should fail
// * a case where the list of locks is empty
// * that a user cannot include a lock id for a lock belonging to a different user
#[test]
fn test_refresh_multiple_locks() {
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
    );
    let (mut deps, mut env) = (mock_dependencies(grpc_query), mock_env());
    let sender = "addr0000";
    let other_sender = "addr0001";
    let info = get_message_info(&deps.api, sender, &[]);

    // Define test cases
    let test_cases = vec![
        TestCase {
            name: "Empty lock_ids",
            lock_ids: vec![],
            new_lock_duration: ONE_MONTH_IN_NANO_SECONDS * 3,
            expected_error: Some("No lock_ids provided".to_string()),
            expected_new_lock_durations: vec![
                (other_sender.to_string(), vec![8]),
                (sender.to_string(), vec![9, 4, 2]),
            ],
        },
        TestCase {
            name: "Shortening locks",
            lock_ids: vec![1, 2, 3],
            new_lock_duration: ONE_MONTH_IN_NANO_SECONDS, // shorter than the remaining duration
            expected_error: Some("Shortening locks is not allowed".to_string()),
            expected_new_lock_durations: vec![
                (other_sender.to_string(), vec![8]),
                (sender.to_string(), vec![9, 4, 2]),
            ],
        },
        TestCase {
            name: "Successful refresh of multiple locks",
            lock_ids: vec![2, 3],
            new_lock_duration: ONE_MONTH_IN_NANO_SECONDS * 6, // longer than the remaining duration
            expected_error: None,
            expected_new_lock_durations: vec![
                (other_sender.to_string(), vec![8]),
                (sender.to_string(), vec![9, 6, 6]),
            ],
        },
        TestCase {
            name: "Successful refresh of a single lock",
            lock_ids: vec![3],
            new_lock_duration: ONE_MONTH_IN_NANO_SECONDS * 3,
            expected_error: None,
            expected_new_lock_durations: vec![
                (other_sender.to_string(), vec![8]),
                (sender.to_string(), vec![9, 4, 3]),
            ],
        },
        TestCase {
            name: "Refresh other users lock",
            lock_ids: vec![0, 1, 2, 3],
            new_lock_duration: ONE_MONTH_IN_NANO_SECONDS * 12,
            expected_error: Some("not found".to_string()),
            expected_new_lock_durations: vec![
                (other_sender.to_string(), vec![8]),
                (sender.to_string(), vec![9, 4, 2]),
            ],
        },
    ];

    // Execute test cases
    for case in test_cases {
        println!("Running test case: {}", case.name);
        let mut msg = get_default_instantiate_msg(&deps.api);
        msg.is_in_pilot_mode = false;
        msg.lock_epoch_length = ONE_MONTH_IN_NANO_SECONDS;
        msg.round_length = ONE_MONTH_IN_NANO_SECONDS;

        let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
        assert!(res.is_ok());

        set_default_validator_for_rounds(deps.as_mut(), 0, 100);

        // Create multiple locks with different durations, starting times, and senders
        let lock_durations = [
            (ONE_MONTH_IN_NANO_SECONDS * 12, other_sender),
            (ONE_MONTH_IN_NANO_SECONDS * 12, sender),
            (ONE_MONTH_IN_NANO_SECONDS * 6, sender),
            (ONE_MONTH_IN_NANO_SECONDS * 3, sender),
        ];

        for &(duration, locker) in lock_durations.iter() {
            let info = get_message_info(
                &deps.api,
                locker,
                &[Coin::new(1000u64, IBC_DENOM_1.to_string())],
            );
            let lock_msg = ExecuteMsg::LockTokens {
                lock_duration: duration,
            };
            let res = execute(deps.as_mut(), env.clone(), info.clone(), lock_msg);
            assert!(
                res.is_ok(),
                "Lock creation failed for duration: {} with error: {}",
                duration,
                res.err().unwrap()
            );

            // Advance time for each lock
            env.block.time = env.block.time.plus_nanos(ONE_MONTH_IN_NANO_SECONDS);
        }

        // now, the locks should have remaining times of 9, 4, and 2 months (and the other senders lockup has 8 months remaining)
        let refresh_msg = ExecuteMsg::RefreshLockDuration {
            lock_ids: case.lock_ids.clone(),
            lock_duration: case.new_lock_duration,
        };

        let res = execute(deps.as_mut(), env.clone(), info.clone(), refresh_msg);

        match &case.expected_error {
            Some(expected_error) => {
                assert!(
                    res.is_err(),
                    "Expected error for lock_ids: {:?}, new_lock_duration: {}",
                    case.lock_ids,
                    case.new_lock_duration
                );
                let error = res.unwrap_err().to_string();
                assert!(
                    error.contains(expected_error),
                    "Expected error message to contain: {}, but was: {}",
                    expected_error,
                    error
                );
            }
            None => {
                assert!(
                    res.is_ok(),
                    "Expected success for lock_ids: {:?}, new_lock_duration: {}; error: {}",
                    case.lock_ids,
                    case.new_lock_duration,
                    res.err().unwrap()
                );
            }
        }

        // Verify the new lock durations
        for (sender, expected_durations) in &case.expected_new_lock_durations {
            let lockups = query_all_user_lockups(
                deps.as_ref(),
                env.clone(),
                get_address_as_str(&deps.api, sender),
                0,
                100,
            )
            .unwrap()
            .lockups;
            for (i, &expected_duration) in expected_durations.iter().enumerate() {
                let expected_nanos = expected_duration * ONE_MONTH_IN_NANO_SECONDS;
                let remaining_lock_duration = lockups[i]
                    .lock_entry
                    .lock_end
                    .minus_nanos(env.block.time.nanos());
                assert_eq!(
                    expected_nanos,
                    remaining_lock_duration.nanos(),
                    "Lock duration mismatch for lock_id: {}, expected: {}, actual: {}",
                    i,
                    expected_nanos,
                    remaining_lock_duration.nanos()
                );
            }
        }
    }
}
