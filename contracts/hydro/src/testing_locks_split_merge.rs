use crate::contract::{execute, instantiate, query, query_user_voted_locks};
use crate::msg::{ExecuteMsg, ProposalToLockups};
use crate::query::{QueryMsg, TokensResponse};
use crate::state::{
    LOCKS_PENDING_SLASHES, PROPOSAL_MAP, PROPOSAL_TOTAL_MAP, VOTE_MAP_V2, VOTING_ALLOWED_ROUND,
};
use crate::testing::{
    get_address_as_str, get_default_instantiate_msg, get_message_info,
    set_default_validator_for_rounds, setup_st_atom_token_info_provider_mock, IBC_DENOM_1,
    IBC_DENOM_2, ONE_DAY_IN_NANO_SECONDS, ONE_MONTH_IN_NANO_SECONDS, ST_ATOM_ON_NEUTRON,
    ST_ATOM_ON_STRIDE, THREE_MONTHS_IN_NANO_SECONDS, VALIDATOR_1, VALIDATOR_1_LST_DENOM_1,
    VALIDATOR_2, VALIDATOR_2_LST_DENOM_1,
};
use crate::testing_lsm_integration::set_validator_infos_for_round;
use crate::testing_mocks::denom_trace_grpc_query_mock;
use cosmwasm_std::{from_json, testing::mock_env, Coin, Decimal, Uint128};
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::vec;

use crate::state::{LOCKS_MAP_V2, USER_LOCKS, USER_LOCKS_FOR_CLAIM};

#[test]
fn test_lock_split_flow_multiple_rounds() {
    // Instantiate contract
    let user_address = "addr0000";
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
    );
    let (mut deps, mut env) = (
        crate::testing_mocks::mock_dependencies(grpc_query),
        mock_env(),
    );
    let info = get_message_info(&deps.api, user_address, &[]);
    let mut instantiate_msg = get_default_instantiate_msg(&deps.api);
    instantiate_msg.round_length = ONE_MONTH_IN_NANO_SECONDS;
    instantiate_msg.whitelist_admins = vec![get_address_as_str(&deps.api, user_address)];
    instantiate(deps.as_mut(), env.clone(), info.clone(), instantiate_msg).unwrap();

    set_default_validator_for_rounds(deps.as_mut(), 0, 3);

    let tranche_id = 1;
    let first_lock_id = 0;
    let second_lock_id = 1;
    let third_lock_id = 2;
    let fourth_lock_id = 3;
    let fifth_lock_id = 4;
    let first_proposal_id = 0;
    let second_proposal_id = 1;
    let third_proposal_id = 2;

    // Lock tokens in round 0
    let initial_lock_amount = Uint128::from(50000u128);
    let funds = vec![Coin::new(initial_lock_amount.u128(), IBC_DENOM_1)];
    let lock_info = get_message_info(&deps.api, user_address, &funds);
    env.block.time = env.block.time.plus_nanos(1);

    let lock_res = execute(
        deps.as_mut(),
        env.clone(),
        lock_info.clone(),
        ExecuteMsg::LockTokens {
            lock_duration: 3 * ONE_MONTH_IN_NANO_SECONDS,
            proof: None,
        },
    );
    assert!(lock_res.is_ok());

    // Create proposal in round 0 and vote for it with lockup from previous step
    let proposal_info = get_message_info(&deps.api, user_address, &[]);
    let create_prop_res = execute(
        deps.as_mut(),
        env.clone(),
        proposal_info.clone(),
        ExecuteMsg::CreateProposal {
            round_id: None,
            tranche_id,
            title: "Test Proposal".to_string(),
            description: "Test".to_string(),
            deployment_duration: 1,
            minimum_atom_liquidity_request: Uint128::zero(),
        },
    );
    assert!(create_prop_res.is_ok());

    let vote_info = get_message_info(&deps.api, user_address, &[]);
    let vote_res = execute(
        deps.as_mut(),
        env.clone(),
        vote_info.clone(),
        ExecuteMsg::Vote {
            tranche_id,
            proposals_votes: vec![ProposalToLockups {
                proposal_id: first_proposal_id,
                lock_ids: vec![first_lock_id],
            }],
        },
    );
    assert!(vote_res.is_ok());

    // Move to round 1
    env.block.time = env.block.time.plus_nanos(ONE_MONTH_IN_NANO_SECONDS);

    // Create a proposal and vote for it with the lockup created in step 2
    let create_prop_res2 = execute(
        deps.as_mut(),
        env.clone(),
        proposal_info.clone(),
        ExecuteMsg::CreateProposal {
            round_id: None,
            tranche_id,
            title: "Test Proposal 2".to_string(),
            description: "Test2".to_string(),
            deployment_duration: 1,
            minimum_atom_liquidity_request: Uint128::zero(),
        },
    );
    assert!(create_prop_res2.is_ok());

    let vote_res2 = execute(
        deps.as_mut(),
        env.clone(),
        vote_info.clone(),
        ExecuteMsg::Vote {
            tranche_id,
            proposals_votes: vec![ProposalToLockups {
                proposal_id: second_proposal_id,
                lock_ids: vec![first_lock_id],
            }],
        },
    );
    assert!(vote_res2.is_ok());

    // Move to round 2
    env.block.time = env.block.time.plus_nanos(ONE_MONTH_IN_NANO_SECONDS);

    // Create a proposal and vote for it with the lockup created before
    let create_prop_res3 = execute(
        deps.as_mut(),
        env.clone(),
        proposal_info.clone(),
        ExecuteMsg::CreateProposal {
            round_id: None,
            tranche_id,
            title: "Test Proposal 3".to_string(),
            description: "Test3".to_string(),
            deployment_duration: 1,
            minimum_atom_liquidity_request: Uint128::zero(),
        },
    );
    assert!(create_prop_res3.is_ok());

    let vote_res3 = execute(
        deps.as_mut(),
        env.clone(),
        vote_info.clone(),
        ExecuteMsg::Vote {
            tranche_id,
            proposals_votes: vec![ProposalToLockups {
                proposal_id: third_proposal_id,
                lock_ids: vec![first_lock_id],
            }],
        },
    );
    assert!(vote_res3.is_ok());

    let prop_power_before_split = PROPOSAL_MAP
        .load(&deps.storage, (2, tranche_id, third_proposal_id))
        .unwrap()
        .power;
    assert_eq!(prop_power_before_split, initial_lock_amount);

    // Mock a pending slash being attached to the first lockup
    LOCKS_PENDING_SLASHES
        .save(&mut deps.storage, first_lock_id, &Uint128::from(9000u128))
        .unwrap();

    // Split the lockup in round 2
    let split_amount_1 = Uint128::from(10000u128);
    let split_res = execute(
        deps.as_mut(),
        env.clone(),
        vote_info.clone(),
        ExecuteMsg::SplitLock {
            lock_id: first_lock_id,
            amount: split_amount_1,
        },
    );
    assert!(split_res.is_ok());

    // Verify that the lockup with id 0 is removed
    assert!(crate::state::LOCKS_MAP_V2
        .may_load(&deps.storage, first_lock_id)
        .unwrap()
        .is_none());

    // Check that both new locks exist and have correct amounts
    let first_lock = crate::state::LOCKS_MAP_V2
        .may_load(&deps.storage, second_lock_id)
        .unwrap()
        .unwrap();

    let second_lock = crate::state::LOCKS_MAP_V2
        .may_load(&deps.storage, third_lock_id)
        .unwrap()
        .unwrap();

    assert_eq!(
        first_lock.funds.amount,
        initial_lock_amount - split_amount_1
    );
    assert_eq!(second_lock.funds.amount, split_amount_1);
    assert_eq!(first_lock.owner, second_lock.owner);

    // Verify votes for current round (round 2) for both new lockups
    let round_id = 2;
    let vote_new_lock_1 = VOTE_MAP_V2
        .may_load(&deps.storage, ((round_id, tranche_id), second_lock_id))
        .unwrap()
        .unwrap();
    let vote_new_lock_2 = VOTE_MAP_V2
        .may_load(&deps.storage, ((round_id, tranche_id), third_lock_id))
        .unwrap()
        .unwrap();

    assert_eq!(vote_new_lock_1.prop_id, third_proposal_id);
    assert_eq!(vote_new_lock_2.prop_id, third_proposal_id);

    // Verify votes for new lockups in previous rounds (should exist with 0 power)
    verify_new_lock_expected_round_votes(
        &deps.storage,
        tranche_id,
        second_lock_id,
        &[(0, first_proposal_id), (1, second_proposal_id)],
    );
    verify_new_lock_expected_round_votes(
        &deps.storage,
        tranche_id,
        third_lock_id,
        &[(0, first_proposal_id), (1, second_proposal_id)],
    );

    let first_lock_voting_allowed = VOTING_ALLOWED_ROUND
        .load(&deps.storage, (tranche_id, second_lock_id))
        .unwrap();
    let second_lock_voting_allowed = VOTING_ALLOWED_ROUND
        .load(&deps.storage, (tranche_id, third_lock_id))
        .unwrap();

    assert_eq!(first_lock_voting_allowed, 3);
    assert_eq!(second_lock_voting_allowed, 3);

    // Verify that the proposal power is unchanged after the split
    let prop_power_after_split = PROPOSAL_MAP
        .load(&deps.storage, (2, tranche_id, third_proposal_id))
        .unwrap()
        .power;
    assert_eq!(prop_power_before_split, prop_power_after_split);

    // Verify that the pending slashes are correctly split between the new locks
    assert!(LOCKS_PENDING_SLASHES
        .may_load(&deps.storage, first_lock_id)
        .unwrap()
        .is_none());
    assert_eq!(
        LOCKS_PENDING_SLASHES
            .load(&deps.storage, second_lock_id)
            .unwrap()
            .u128(),
        7200
    );
    assert_eq!(
        LOCKS_PENDING_SLASHES
            .load(&deps.storage, third_lock_id)
            .unwrap()
            .u128(),
        1800
    );

    // Move to round 3
    env.block.time = env.block.time.plus_nanos(ONE_MONTH_IN_NANO_SECONDS);

    // Split the lockup 2 in round 3 when the lockup hasn't voted yet
    let split_amount_2 = Uint128::from(20000u128);
    let split_res = execute(
        deps.as_mut(),
        env.clone(),
        vote_info.clone(),
        ExecuteMsg::SplitLock {
            lock_id: second_lock_id,
            amount: split_amount_2,
        },
    );
    assert!(split_res.is_ok());

    // Verify that the lockup with id 1 is removed
    assert!(crate::state::LOCKS_MAP_V2
        .may_load(&deps.storage, second_lock_id)
        .unwrap()
        .is_none());

    // Verify that both new locks exist and have correct amounts
    let fourth_lock = crate::state::LOCKS_MAP_V2
        .may_load(&deps.storage, fourth_lock_id)
        .unwrap()
        .unwrap();
    let fifth_lock = crate::state::LOCKS_MAP_V2
        .may_load(&deps.storage, fifth_lock_id)
        .unwrap()
        .unwrap();

    assert_eq!(
        fourth_lock.funds.amount,
        initial_lock_amount - split_amount_1 - split_amount_2
    );
    assert_eq!(fifth_lock.funds.amount, split_amount_2);

    // Verify votes for new lockups in previous rounds (should exist with 0 power)
    verify_new_lock_expected_round_votes(
        &deps.storage,
        tranche_id,
        fourth_lock_id,
        &[
            (0, first_proposal_id),
            (1, second_proposal_id),
            (2, third_proposal_id),
        ],
    );
    verify_new_lock_expected_round_votes(
        &deps.storage,
        tranche_id,
        fifth_lock_id,
        &[
            (0, first_proposal_id),
            (1, second_proposal_id),
            (2, third_proposal_id),
        ],
    );

    let fourth_lock_voting_allowed = VOTING_ALLOWED_ROUND
        .load(&deps.storage, (tranche_id, fourth_lock_id))
        .unwrap();
    assert_eq!(fourth_lock_voting_allowed, 3);

    let fifth_lock_voting_allowed = VOTING_ALLOWED_ROUND
        .load(&deps.storage, (tranche_id, fifth_lock_id))
        .unwrap();
    assert_eq!(fifth_lock_voting_allowed, 3);

    // Verify that the query_user_voted_locks() returns the correct votes and powers.
    // This query is called from the Tribute SC when a user wants to claim tribute.
    let round_id = 0;
    let round_votes = query_user_voted_locks(
        deps.as_ref(),
        info.sender.to_string(),
        round_id,
        tranche_id,
        None,
    )
    .unwrap();

    assert_eq!(1, round_votes.voted_locks.len());

    let first_proposal_votes = round_votes.voted_locks[0].clone();
    assert_eq!(first_proposal_votes.0, first_proposal_id);

    // Initial lock was split resulting in 2 new locks, then one of the resulting locks was also split,
    // so there should be 5 votes for the first proposal, where only the vote belonging to the first lock has power.
    assert_eq!(first_proposal_votes.1.len(), 5);

    let first_lock_voting_power = Decimal::from_ratio(initial_lock_amount, 1u128)
        .checked_mul(Decimal::from_str("1.5").unwrap())
        .unwrap();
    assert_eq!(first_proposal_votes.1[0].lock_id, first_lock_id);
    assert_eq!(
        first_proposal_votes.1[0].vote_power,
        first_lock_voting_power
    );
    assert_eq!(first_proposal_votes.1[1].lock_id, second_lock_id);
    assert_eq!(first_proposal_votes.1[1].vote_power, Decimal::zero());
    assert_eq!(first_proposal_votes.1[2].lock_id, third_lock_id);
    assert_eq!(first_proposal_votes.1[2].vote_power, Decimal::zero());
    assert_eq!(first_proposal_votes.1[3].lock_id, fourth_lock_id);
    assert_eq!(first_proposal_votes.1[3].vote_power, Decimal::zero());
    assert_eq!(first_proposal_votes.1[4].lock_id, fifth_lock_id);
    assert_eq!(first_proposal_votes.1[4].vote_power, Decimal::zero());
}

#[test]
fn test_merge_locks_flow() {
    // Instantiate contract
    let user_address = "addr0000";
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
    );
    let (mut deps, mut env) = (
        crate::testing_mocks::mock_dependencies(grpc_query),
        mock_env(),
    );
    let info = get_message_info(&deps.api, user_address, &[]);
    let mut instantiate_msg = get_default_instantiate_msg(&deps.api);
    instantiate_msg.round_length = ONE_MONTH_IN_NANO_SECONDS;
    instantiate_msg.whitelist_admins = vec![get_address_as_str(&deps.api, user_address)];
    instantiate(deps.as_mut(), env.clone(), info.clone(), instantiate_msg).unwrap();

    set_default_validator_for_rounds(deps.as_mut(), 0, 3);

    let tranche_id = 1;

    let lock_id_1 = 0;
    let lock_id_2 = 1;
    let lock_id_3 = 2;
    let _lock_id_4 = 3;
    let lock_id_5 = 4;
    let lock_id_6 = 5;
    let lock_id_7 = 6;
    let _lock_id_8 = 7;

    let proposal_id_1 = 0;
    let proposal_id_2 = 1;
    let proposal_id_3 = 2;
    let proposal_id_4 = 3;

    let mut lock_ids = vec![];
    let mut lock_amounts = vec![];

    // In round 0, create 8 lockups (4x1mo, 4x3mo)
    env.block.time = env.block.time.plus_nanos(ONE_DAY_IN_NANO_SECONDS);

    let one_month = ONE_MONTH_IN_NANO_SECONDS;
    let three_months = 3 * ONE_MONTH_IN_NANO_SECONDS;
    let base_amount = Uint128::from(10000u128);

    for i in 0..8 {
        let duration = if i < 4 { one_month } else { three_months };
        let amount = base_amount + Uint128::from(i as u128 * 1000);
        let funds = vec![Coin::new(amount.u128(), IBC_DENOM_1)];
        let lock_info = get_message_info(&deps.api, user_address, &funds);
        let lock_res = execute(
            deps.as_mut(),
            env.clone(),
            lock_info,
            ExecuteMsg::LockTokens {
                lock_duration: duration,
                proof: None,
            },
        );
        assert!(lock_res.is_ok());
        lock_ids.push(i as u64);
        lock_amounts.push(amount);
    }

    // Create 2 proposals: 1mo and 3mo deployment_duration
    let proposal_info = get_message_info(&deps.api, user_address, &[]);
    let create_prop1 = execute(
        deps.as_mut(),
        env.clone(),
        proposal_info.clone(),
        ExecuteMsg::CreateProposal {
            round_id: None,
            tranche_id,
            title: "Proposal 1".to_string(),
            description: "P1".to_string(),
            deployment_duration: 1,
            minimum_atom_liquidity_request: Uint128::zero(),
        },
    );
    assert!(create_prop1.is_ok());
    let create_prop2 = execute(
        deps.as_mut(),
        env.clone(),
        proposal_info.clone(),
        ExecuteMsg::CreateProposal {
            round_id: None,
            tranche_id,
            title: "Proposal 2".to_string(),
            description: "P2".to_string(),
            deployment_duration: 3,
            minimum_atom_liquidity_request: Uint128::zero(),
        },
    );
    assert!(create_prop2.is_ok());

    // Vote with 2 lockups for proposal 1, and 2 other for proposal 2
    let vote_info = get_message_info(&deps.api, user_address, &[]);
    let vote_res1 = execute(
        deps.as_mut(),
        env.clone(),
        vote_info.clone(),
        ExecuteMsg::Vote {
            tranche_id,
            proposals_votes: vec![
                ProposalToLockups {
                    proposal_id: proposal_id_1,
                    lock_ids: vec![lock_id_1, lock_id_2],
                },
                ProposalToLockups {
                    proposal_id: proposal_id_2,
                    lock_ids: vec![lock_id_5, lock_id_6],
                },
            ],
        },
    );
    assert!(vote_res1.is_ok());

    // Move to next round
    env.block.time = env.block.time.plus_nanos(ONE_MONTH_IN_NANO_SECONDS);

    // Create two new proposals (1mo and 3mo deployment duration)
    let create_prop3 = execute(
        deps.as_mut(),
        env.clone(),
        proposal_info.clone(),
        ExecuteMsg::CreateProposal {
            round_id: None,
            tranche_id,
            title: "Proposal 3".to_string(),
            description: "P3".to_string(),
            deployment_duration: 1,
            minimum_atom_liquidity_request: Uint128::zero(),
        },
    );
    assert!(create_prop3.is_ok());

    let create_prop4 = execute(
        deps.as_mut(),
        env.clone(),
        proposal_info.clone(),
        ExecuteMsg::CreateProposal {
            round_id: None,
            tranche_id,
            title: "Proposal 4".to_string(),
            description: "P4".to_string(),
            deployment_duration: 3,
            minimum_atom_liquidity_request: Uint128::zero(),
        },
    );
    assert!(create_prop4.is_ok());

    // Refresh 2 lockups before voting
    let refresh_info = get_message_info(&deps.api, user_address, &[]);
    let refresh_res1 = execute(
        deps.as_mut(),
        env.clone(),
        refresh_info.clone(),
        ExecuteMsg::RefreshLockDuration {
            lock_ids: vec![lock_id_3],
            lock_duration: ONE_MONTH_IN_NANO_SECONDS,
        },
    );
    assert!(refresh_res1.is_ok());

    let refresh_res2 = execute(
        deps.as_mut(),
        env.clone(),
        refresh_info.clone(),
        ExecuteMsg::RefreshLockDuration {
            lock_ids: vec![lock_id_7],
            lock_duration: 3 * ONE_MONTH_IN_NANO_SECONDS,
        },
    );
    assert!(refresh_res2.is_ok());

    // Use 2 lockups unused in round 0 to vote for two new proposals, separately
    let vote_res2 = execute(
        deps.as_mut(),
        env.clone(),
        vote_info.clone(),
        ExecuteMsg::Vote {
            tranche_id,
            proposals_votes: vec![
                ProposalToLockups {
                    proposal_id: proposal_id_3,
                    lock_ids: vec![lock_id_3],
                },
                ProposalToLockups {
                    proposal_id: proposal_id_4,
                    lock_ids: vec![lock_id_7],
                },
            ],
        },
    );
    assert!(vote_res2.is_ok());

    // Mock a pending slashes being attached to some lockups that are being merged
    let pending_slashes = [
        (lock_id_2, &Uint128::from(600u128)),
        (lock_id_5, &Uint128::from(300u128)),
    ];
    for pending_slash in &pending_slashes {
        LOCKS_PENDING_SLASHES
            .save(&mut deps.storage, pending_slash.0, pending_slash.1)
            .unwrap();
    }

    // Merge all 8 lockups into new lockup (provide some IDs twice)
    let mut merge_ids = lock_ids.clone();
    merge_ids.push(0);
    merge_ids.push(1);

    let merge_res = execute(
        deps.as_mut(),
        env.clone(),
        vote_info.clone(),
        ExecuteMsg::MergeLocks {
            lock_ids: merge_ids.clone(),
        },
    );
    assert!(merge_res.is_ok());

    // Verification

    // The new lockup should have id 8
    let new_lock_id = 8u64;
    let new_lock = LOCKS_MAP_V2
        .may_load(&deps.storage, new_lock_id)
        .unwrap()
        .unwrap();

    // Amount should be sum of all lock amounts
    let expected_amount: Uint128 = lock_amounts.iter().sum();
    assert_eq!(new_lock.funds.amount, expected_amount);

    // All old lockups should be removed
    for id in &lock_ids {
        assert!(LOCKS_MAP_V2.may_load(&deps.storage, *id).unwrap().is_none());
    }

    // USER_LOCKS should contain only the new lockup
    let user_addr = deps.api.addr_make(user_address);
    let user_locks = USER_LOCKS.load(&deps.storage, user_addr.clone()).unwrap();
    assert_eq!(user_locks, vec![new_lock_id]);

    // USER_LOCKS_FOR_CLAIM should contain all old lockups plus new one (old ones are retained)
    let user_locks_for_claim = USER_LOCKS_FOR_CLAIM.load(&deps.storage, user_addr).unwrap();
    let mut expected_claim_ids: HashSet<u64> = lock_ids.iter().copied().collect();
    expected_claim_ids.insert(new_lock_id);
    let actual_claim_ids: HashSet<u64> = user_locks_for_claim.into_iter().collect();
    assert_eq!(expected_claim_ids, actual_claim_ids);

    // Old lock votes (lock ids: 0, 1, 4, 5) in round 0 should remain in the storage
    // Also, 0-power vote should be created for new lockup
    let round_id = 0;
    let locks_voted_powers = HashMap::from([
        (
            lock_id_1,
            Decimal::from_ratio(Uint128::new(10000u128), Uint128::one()),
        ),
        (
            lock_id_2,
            Decimal::from_ratio(Uint128::new(11000u128), Uint128::one()),
        ),
        (
            lock_id_5,
            Decimal::from_ratio(Uint128::new(14000u128), Uint128::one())
                .checked_mul(Decimal::from_str("1.5").unwrap())
                .unwrap(),
        ),
        (
            lock_id_6,
            Decimal::from_ratio(Uint128::new(15000u128), Uint128::one())
                .checked_mul(Decimal::from_str("1.5").unwrap())
                .unwrap(),
        ),
        (new_lock_id, Decimal::zero()),
    ]);

    for lock_vote in &locks_voted_powers {
        let vote_r0 = VOTE_MAP_V2
            .may_load(&deps.storage, ((round_id, tranche_id), *lock_vote.0))
            .unwrap();
        assert!(vote_r0.is_some());
    }

    // Verify that the query_user_voted_locks() for round 0 returns the correct votes and powers.
    // This query is called from the Tribute SC when a user wants to claim tribute.
    let round_votes = query_user_voted_locks(
        deps.as_ref(),
        info.sender.to_string(),
        round_id,
        tranche_id,
        None,
    )
    .unwrap();
    assert_eq!(2, round_votes.voted_locks.len());

    let first_proposal_votes = round_votes
        .voted_locks
        .iter()
        .find(|prop_votes| prop_votes.0 == proposal_id_1)
        .unwrap();

    // Locks 0 and 1 voted for proposal_id_1, so there should be 2 votes.
    assert_eq!(first_proposal_votes.1.len(), 2);

    for voted_lock in &first_proposal_votes.1 {
        assert_eq!(
            voted_lock.vote_power,
            locks_voted_powers[&voted_lock.lock_id]
        );
    }

    let second_proposal_votes = round_votes
        .voted_locks
        .iter()
        .find(|prop_votes| prop_votes.0 == proposal_id_2)
        .unwrap();

    // Locks 4 and 5 voted for proposal_id_2, and 0-power vote was inserted for new lock, so there should be 3 votes.
    assert_eq!(second_proposal_votes.1.len(), 3);

    for voted_lock in &second_proposal_votes.1 {
        assert_eq!(
            voted_lock.vote_power,
            locks_voted_powers[&voted_lock.lock_id]
        );
    }

    // For round 1, only lock ids (2 and 6) voted, but their votes should be removed since they were merged into new lockup.
    // Vote for new lock should not be created since some of the merged lockups were not allowed to vote in round 1.
    // Also, lock ids 2 and 6 voted for different proposals, so no vote for new lockup in round 1 for that reason as well.
    let round_id = 1;
    for lock_id in [lock_id_3, lock_id_7, new_lock_id] {
        let vote_r0 = VOTE_MAP_V2
            .may_load(&deps.storage, ((round_id, tranche_id), lock_id))
            .unwrap();
        assert!(vote_r0.is_none());
    }

    // Verify that all pending slashes have been appended to the new lock entry.
    assert!(LOCKS_PENDING_SLASHES
        .may_load(&deps.storage, lock_id_2)
        .unwrap()
        .is_none());
    assert!(LOCKS_PENDING_SLASHES
        .may_load(&deps.storage, lock_id_5)
        .unwrap()
        .is_none());
    assert_eq!(
        LOCKS_PENDING_SLASHES
            .load(&deps.storage, new_lock_id)
            .unwrap()
            .u128(),
        900
    );
}

#[test]
fn test_merge_locks_basic_validation() {
    // Instantiate contract
    let user_address_1 = "addr0000";
    let user_address_2 = "addr0001";
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([
            (IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string()),
            (IBC_DENOM_2.to_string(), VALIDATOR_2_LST_DENOM_1.to_string()),
        ]),
    );
    let (mut deps, env) = (
        crate::testing_mocks::mock_dependencies(grpc_query),
        mock_env(),
    );
    let info = get_message_info(&deps.api, user_address_1, &[]);
    let mut instantiate_msg = get_default_instantiate_msg(&deps.api);
    instantiate_msg.round_length = ONE_MONTH_IN_NANO_SECONDS;
    instantiate_msg.whitelist_admins = vec![get_address_as_str(&deps.api, user_address_1)];
    instantiate(deps.as_mut(), env.clone(), info.clone(), instantiate_msg).unwrap();

    // Setup validator for round 0
    set_validator_infos_for_round(
        &mut deps.storage,
        0,
        vec![VALIDATOR_1.to_string(), VALIDATOR_2.to_string()],
    )
    .unwrap();

    let lock_id_1 = 0;
    let lock_id_2 = 1;
    let lock_id_3 = 2;

    // In round 0, have 2 users create 3 lockups
    let base_amount = Uint128::from(10000u128);

    let user_funds = vec![
        (user_address_1, Coin::new(base_amount.u128(), IBC_DENOM_1)),
        (user_address_1, Coin::new(base_amount.u128(), IBC_DENOM_2)),
        (user_address_2, Coin::new(base_amount.u128(), IBC_DENOM_1)),
    ];

    for (user_address, funds) in user_funds {
        let lock_info = get_message_info(&deps.api, user_address, &[funds]);
        let lock_res = execute(
            deps.as_mut(),
            env.clone(),
            lock_info,
            ExecuteMsg::LockTokens {
                lock_duration: ONE_MONTH_IN_NANO_SECONDS,
                proof: None,
            },
        );
        assert!(lock_res.is_ok());
    }

    let user1_info = get_message_info(&deps.api, user_address_1, &[]);

    // Have user 1 try to merge only one lock
    let merge_res = execute(
        deps.as_mut(),
        env.clone(),
        user1_info.clone(),
        ExecuteMsg::MergeLocks {
            lock_ids: vec![lock_id_1],
        },
    );
    assert!(merge_res.is_err());
    assert!(merge_res
        .unwrap_err()
        .to_string()
        .contains("Must specify at least two lock IDs to merge."));

    // Have user 1 try to merge their two locks with different denoms
    let merge_res = execute(
        deps.as_mut(),
        env.clone(),
        user1_info.clone(),
        ExecuteMsg::MergeLocks {
            lock_ids: vec![lock_id_1, lock_id_2],
        },
    );
    assert!(merge_res.is_err());
    assert!(merge_res
        .unwrap_err()
        .to_string()
        .contains("Cannot merge locks with different denoms."));

    // Have user 1 try to merge one of the locks that belongs to  another user
    let merge_res = execute(
        deps.as_mut(),
        env.clone(),
        user1_info.clone(),
        ExecuteMsg::MergeLocks {
            lock_ids: vec![lock_id_1, lock_id_3],
        },
    );
    assert!(merge_res.is_err());
    assert!(merge_res.unwrap_err().to_string().contains("Unauthorized"));
}

// Verifies that proposal voting powers of those proposals that the merged lockups had voted for
// in the current round are being correctly updated in the following cases:
//      1. At least one of the lockups being merged isn't allowed to vote in current round,
//         while others have voted on some proposals
//      2. Two or more lockups being merged have already voted for the same proposal in the current round.
//      3. Two or more lockups being merged have already voted for different proposal in the current round.
#[test]
fn test_merge_locks_update_proposal_powers() {
    let user_address = "addr0000";
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
    );
    let (mut deps, mut env) = (
        crate::testing_mocks::mock_dependencies(grpc_query),
        mock_env(),
    );
    let info = get_message_info(&deps.api, user_address, &[]);
    let mut instantiate_msg = get_default_instantiate_msg(&deps.api);
    instantiate_msg.round_length = ONE_MONTH_IN_NANO_SECONDS;
    instantiate_msg.whitelist_admins = vec![get_address_as_str(&deps.api, user_address)];
    instantiate(deps.as_mut(), env.clone(), info.clone(), instantiate_msg).unwrap();

    set_default_validator_for_rounds(deps.as_mut(), 0, 3);

    let round_id_2 = 1;

    let tranche_id = 1;

    let lock_id_1 = 0;
    let lock_id_2 = 1;
    let lock_id_3 = 2;
    let lock_id_4 = 3;
    let lock_id_5 = 4;
    let lock_id_6 = 5;
    let merge_lock_id_7 = 6;
    let merge_lock_id_8 = 7;
    let merge_lock_id_9 = 8;

    let proposal_id_1 = 0;
    let proposal_id_2 = 1;
    let proposal_id_3 = 2;

    let mut lock_ids = vec![];
    let mut lock_amounts = vec![];

    // In round 0, create 6 lockups
    env.block.time = env.block.time.plus_nanos(ONE_DAY_IN_NANO_SECONDS);

    let base_amount = Uint128::from(10000u128);

    for i in 0..6 {
        let amount = base_amount + Uint128::from(i as u128 * 1000);
        let funds = vec![Coin::new(amount.u128(), IBC_DENOM_1)];
        let lock_info = get_message_info(&deps.api, user_address, &funds);
        let lock_res = execute(
            deps.as_mut(),
            env.clone(),
            lock_info,
            ExecuteMsg::LockTokens {
                lock_duration: THREE_MONTHS_IN_NANO_SECONDS,
                proof: None,
            },
        );
        assert!(lock_res.is_ok());
        lock_ids.push(i as u64);
        lock_amounts.push(amount);
    }

    // Create proposal 1
    let proposal_info = get_message_info(&deps.api, user_address, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        proposal_info.clone(),
        ExecuteMsg::CreateProposal {
            round_id: None,
            tranche_id,
            title: "Proposal 1".to_string(),
            description: "P1".to_string(),
            deployment_duration: 3,
            minimum_atom_liquidity_request: Uint128::zero(),
        },
    )
    .unwrap();

    // Vote for proposal 1 with lockup 1
    let vote_info = get_message_info(&deps.api, user_address, &[]);
    let vote_res1 = execute(
        deps.as_mut(),
        env.clone(),
        vote_info.clone(),
        ExecuteMsg::Vote {
            tranche_id,
            proposals_votes: vec![ProposalToLockups {
                proposal_id: proposal_id_1,
                lock_ids: vec![lock_id_1],
            }],
        },
    );
    assert!(vote_res1.is_ok());

    // Move to round 1
    env.block.time = env.block.time.plus_nanos(ONE_MONTH_IN_NANO_SECONDS);

    // Create proposal 2 in round 1
    let proposal_info = get_message_info(&deps.api, user_address, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        proposal_info.clone(),
        ExecuteMsg::CreateProposal {
            round_id: None,
            tranche_id,
            title: "Proposal 2".to_string(),
            description: "P2".to_string(),
            deployment_duration: 1,
            minimum_atom_liquidity_request: Uint128::zero(),
        },
    )
    .unwrap();

    // Create proposal 3 in round 1
    let proposal_info = get_message_info(&deps.api, user_address, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        proposal_info.clone(),
        ExecuteMsg::CreateProposal {
            round_id: None,
            tranche_id,
            title: "Proposal 3".to_string(),
            description: "P3".to_string(),
            deployment_duration: 1,
            minimum_atom_liquidity_request: Uint128::zero(),
        },
    )
    .unwrap();

    // Refresh lockups 2, 3, 4, 5 and 6 before voting
    let refresh_info = get_message_info(&deps.api, user_address, &[]);
    let refresh_res1 = execute(
        deps.as_mut(),
        env.clone(),
        refresh_info.clone(),
        ExecuteMsg::RefreshLockDuration {
            lock_ids: vec![lock_id_2, lock_id_3, lock_id_4, lock_id_5, lock_id_6],
            lock_duration: THREE_MONTHS_IN_NANO_SECONDS,
        },
    );
    assert!(refresh_res1.is_ok());

    // Vote for proposal 2 with lockups 2, 3, 4 and 5, and for proposal 3 with lockup 6
    let vote_info = get_message_info(&deps.api, user_address, &[]);
    let vote_res1 = execute(
        deps.as_mut(),
        env.clone(),
        vote_info.clone(),
        ExecuteMsg::Vote {
            tranche_id,
            proposals_votes: vec![
                ProposalToLockups {
                    proposal_id: proposal_id_2,
                    lock_ids: vec![lock_id_2, lock_id_3, lock_id_4, lock_id_5],
                },
                ProposalToLockups {
                    proposal_id: proposal_id_3,
                    lock_ids: vec![lock_id_6],
                },
            ],
        },
    );
    assert!(vote_res1.is_ok());

    verify_proposal_voting_power(&deps.storage, round_id_2, tranche_id, proposal_id_2, 75000);
    verify_proposal_voting_power(&deps.storage, round_id_2, tranche_id, proposal_id_3, 22500);

    let user_info = get_message_info(&deps.api, user_address, &[]);

    // Merge lockups 1 and 2
    let merge_res = execute(
        deps.as_mut(),
        env.clone(),
        user_info.clone(),
        ExecuteMsg::MergeLocks {
            lock_ids: vec![lock_id_1, lock_id_2],
        },
    );
    assert!(merge_res.is_ok());

    // Lockup 1 isn't allowed to vote in round 1, and since it has been merged with lockup 2
    // that voted for proposal 2, then the voting power originating from the lockup 2 should
    // also be removed from the proposal 2 power.
    verify_proposal_voting_power(&deps.storage, round_id_2, tranche_id, proposal_id_2, 58500);

    // There should be no vote for the newly created lockup 7.
    assert!(VOTE_MAP_V2
        .may_load(&deps.storage, ((round_id_2, tranche_id), merge_lock_id_7))
        .unwrap()
        .is_none());

    // Merge lockups 3 and 4
    let merge_res = execute(
        deps.as_mut(),
        env.clone(),
        user_info.clone(),
        ExecuteMsg::MergeLocks {
            lock_ids: vec![lock_id_3, lock_id_4],
        },
    );
    assert!(merge_res.is_ok());

    // Lockups 3 and 4 had both previously voted for the proposal 2,
    // so the voting power of the proposal should remain unchanged.
    verify_proposal_voting_power(&deps.storage, round_id_2, tranche_id, proposal_id_2, 58500);

    // Vote should be inserted for the newly created lockup and removed for merged lockups.
    assert!(VOTE_MAP_V2
        .may_load(&deps.storage, ((round_id_2, tranche_id), lock_id_3))
        .unwrap()
        .is_none());
    assert!(VOTE_MAP_V2
        .may_load(&deps.storage, ((round_id_2, tranche_id), lock_id_4))
        .unwrap()
        .is_none());
    assert!(VOTE_MAP_V2
        .may_load(&deps.storage, ((round_id_2, tranche_id), merge_lock_id_8))
        .unwrap()
        .is_some());

    // Merge lockups 5 and 6
    let merge_res = execute(
        deps.as_mut(),
        env.clone(),
        user_info.clone(),
        ExecuteMsg::MergeLocks {
            lock_ids: vec![lock_id_5, lock_id_6],
        },
    );
    assert!(merge_res.is_ok());

    // Lockups 5 and 6 had previously voted for the proposals 2 and 3, and after they have been merged,
    // the voting power of those proposals should be reduced, since those lockups votes were removed.
    verify_proposal_voting_power(&deps.storage, round_id_2, tranche_id, proposal_id_2, 37500);
    verify_proposal_voting_power(&deps.storage, round_id_2, tranche_id, proposal_id_3, 0);

    // There should be no votes for either old nor newly created lockup.
    assert!(VOTE_MAP_V2
        .may_load(&deps.storage, ((round_id_2, tranche_id), lock_id_5))
        .unwrap()
        .is_none());
    assert!(VOTE_MAP_V2
        .may_load(&deps.storage, ((round_id_2, tranche_id), lock_id_5))
        .unwrap()
        .is_none());
    assert!(VOTE_MAP_V2
        .may_load(&deps.storage, ((round_id_2, tranche_id), merge_lock_id_9))
        .unwrap()
        .is_none());
}

fn verify_new_lock_expected_round_votes(
    storage: &dyn cosmwasm_std::Storage,
    tranche_id: u64,
    lock_id: u64,
    expected_round_votes: &[(u64, u64)],
) {
    for (round_id, expected_prop_id) in expected_round_votes {
        let vote = VOTE_MAP_V2
            .may_load(storage, ((*round_id, tranche_id), lock_id))
            .unwrap()
            .unwrap();

        assert_eq!(vote.prop_id, *expected_prop_id);
        assert_eq!(vote.time_weighted_shares.1, Decimal::zero());
    }
}

fn verify_proposal_voting_power(
    storage: &dyn cosmwasm_std::Storage,
    round_id: u64,
    tranche_id: u64,
    proposal_id: u64,
    expected_voting_power: u128,
) {
    let proposal_power = PROPOSAL_MAP
        .load(storage, (round_id, tranche_id, proposal_id))
        .unwrap()
        .power
        .u128();
    assert_eq!(proposal_power, expected_voting_power);

    let proposal_power = PROPOSAL_TOTAL_MAP.load(storage, proposal_id).unwrap();
    assert_eq!(proposal_power.to_uint_floor().u128(), expected_voting_power);
}

#[test]
fn test_split_merge_locks_query_all_tokens_behavior() {
    let user_address = "addr0000";
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
    let (mut deps, env) = (
        crate::testing_mocks::mock_dependencies(grpc_query),
        mock_env(),
    );
    let info = get_message_info(&deps.api, user_address, &[]);
    let mut instantiate_msg = get_default_instantiate_msg(&deps.api);
    instantiate_msg.round_length = ONE_MONTH_IN_NANO_SECONDS;
    instantiate_msg.whitelist_admins = vec![get_address_as_str(&deps.api, user_address)];
    instantiate(deps.as_mut(), env.clone(), info.clone(), instantiate_msg).unwrap();

    // Setup ST_ATOM token info provider (non-LSM)
    let token_info_provider_addr = deps.api.addr_make("token_info_provider_1");
    setup_st_atom_token_info_provider_mock(&mut deps, token_info_provider_addr, Decimal::one());

    set_default_validator_for_rounds(deps.as_mut(), 0, 3);

    // Create a non-LSM lockup (using ST_ATOM_ON_NEUTRON which is not LSM)
    let non_lsm_lock_info = get_message_info(
        &deps.api,
        user_address,
        &[Coin::new(50000u64, ST_ATOM_ON_NEUTRON.to_string())],
    );
    let lock_msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };
    let lock_res = execute(
        deps.as_mut(),
        env.clone(),
        non_lsm_lock_info,
        lock_msg.clone(),
    );
    assert!(lock_res.is_ok(), "Failed to create non-LSM lock");

    // Create an LSM lockup (using IBC_DENOM_1 which maps to LSM)
    let lsm_lock_info = get_message_info(
        &deps.api,
        user_address,
        &[Coin::new(60000u64, IBC_DENOM_1.to_string())],
    );
    let lock_res = execute(deps.as_mut(), env.clone(), lsm_lock_info, lock_msg);
    assert!(lock_res.is_ok(), "Failed to create LSM lock");

    // Query all tokens before split - should only show the non-LSM lockup (ID 0)
    let query_msg = QueryMsg::AllTokens {
        start_after: None,
        limit: None,
    };
    let query_res = query(deps.as_ref(), env.clone(), query_msg.clone());
    assert!(query_res.is_ok(), "Failed to query all tokens before split");
    let tokens_before: TokensResponse = from_json(query_res.unwrap()).unwrap();
    assert_eq!(
        tokens_before.tokens.len(),
        1,
        "Expected 1 token before split (non-LSM only)"
    );
    assert_eq!(
        tokens_before.tokens,
        vec!["0".to_string()],
        "Expected token 0 before split"
    );

    // Split the non-LSM lockup (ID 0) into two parts
    let split_amount = Uint128::from(20000u128); // Results in 20000 and 30000
    let split_msg = ExecuteMsg::SplitLock {
        lock_id: 0,
        amount: split_amount,
    };
    let split_res = execute(deps.as_mut(), env.clone(), info.clone(), split_msg);
    assert!(split_res.is_ok(), "Failed to split non-LSM lock");

    // Query all tokens after splitting non-LSM lock - should show both parts (IDs 2 and 3)
    let query_res = query(deps.as_ref(), env.clone(), query_msg.clone());
    assert!(
        query_res.is_ok(),
        "Failed to query all tokens after non-LSM split"
    );
    let tokens_after_non_lsm_split: TokensResponse = from_json(query_res.unwrap()).unwrap();
    assert_eq!(
        tokens_after_non_lsm_split.tokens.len(),
        2,
        "Expected 2 tokens after non-LSM split"
    );
    assert_eq!(
        tokens_after_non_lsm_split.tokens,
        vec!["2".to_string(), "3".to_string()],
        "Expected tokens 2 and 3 after non-LSM split"
    );

    // Split the LSM lockup (ID 1) into two parts
    let lsm_split_amount = Uint128::from(25000u128); // Results in 25000 and 35000
    let split_msg = ExecuteMsg::SplitLock {
        lock_id: 1,
        amount: lsm_split_amount,
    };
    let split_res = execute(deps.as_mut(), env.clone(), info, split_msg);
    assert!(split_res.is_ok(), "Failed to split LSM lock");

    // Query all tokens after splitting LSM lock - should still only show the non-LSM tokens (IDs 2 and 3)
    // The split LSM parts (IDs 4 and 5) should NOT appear since they're still LSM
    let query_res = query(deps.as_ref(), env.clone(), query_msg.clone());
    assert!(
        query_res.is_ok(),
        "Failed to query all tokens after LSM split"
    );
    let tokens_after_splits: TokensResponse = from_json(query_res.unwrap()).unwrap();
    assert_eq!(
        tokens_after_splits.tokens.len(),
        2,
        "Expected 2 tokens after LSM split (LSM splits should not appear)"
    );
    assert_eq!(
        tokens_after_splits.tokens,
        vec!["2".to_string(), "3".to_string()],
        "Expected only non-LSM tokens 2 and 3 after LSM split"
    );

    // Now test merging: merge the two non-LSM split parts (IDs 2 and 3) back together
    let info = get_message_info(&deps.api, user_address, &[]);
    let merge_msg = ExecuteMsg::MergeLocks {
        lock_ids: vec![2, 3],
    };
    let merge_res = execute(deps.as_mut(), env.clone(), info.clone(), merge_msg);
    assert!(merge_res.is_ok(), "Failed to merge non-LSM locks");

    // Query all tokens after merging non-LSM locks - should show single merged lock (ID 6)
    let query_res = query(deps.as_ref(), env.clone(), query_msg.clone());
    assert!(
        query_res.is_ok(),
        "Failed to query all tokens after non-LSM merge"
    );
    let tokens_after_non_lsm_merge: TokensResponse = from_json(query_res.unwrap()).unwrap();
    assert_eq!(
        tokens_after_non_lsm_merge.tokens.len(),
        1,
        "Expected 1 token after non-LSM merge"
    );
    assert_eq!(
        tokens_after_non_lsm_merge.tokens,
        vec!["6".to_string()],
        "Expected merged token 6 after non-LSM merge"
    );

    // Merge the two LSM split parts (IDs 4 and 5) back together
    let merge_msg = ExecuteMsg::MergeLocks {
        lock_ids: vec![4, 5],
    };
    let merge_res = execute(deps.as_mut(), env.clone(), info, merge_msg);
    assert!(merge_res.is_ok(), "Failed to merge LSM locks");

    // Query all tokens after merging LSM locks - should still only show the non-LSM merged lock (ID 6)
    // The merged LSM lock (ID 7) should NOT appear since it's still LSM
    let query_res = query(deps.as_ref(), env, query_msg);
    assert!(
        query_res.is_ok(),
        "Failed to query all tokens after LSM merge"
    );
    let tokens_final: TokensResponse = from_json(query_res.unwrap()).unwrap();
    assert_eq!(
        tokens_final.tokens.len(),
        1,
        "Expected 1 token after LSM merge (LSM merge should not appear)"
    );
    assert_eq!(
        tokens_final.tokens,
        vec!["6".to_string()],
        "Expected only non-LSM merged token 6 after LSM merge"
    );
}
