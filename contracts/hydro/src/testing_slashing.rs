use crate::contract::{execute, instantiate, query_round_total_power};
use crate::msg::{ExecuteMsg, TrancheInfo};
use crate::slashing::query_slashable_token_num_for_voting_on_proposal;
use crate::state::{
    LockEntryV2, LOCKED_TOKENS, LOCKS_MAP_V2, LOCKS_PENDING_SLASHES, PROPOSAL_MAP, USER_LOCKS,
    VOTE_MAP_V2, VOTING_ALLOWED_ROUND,
};
use crate::testing::{
    get_address_as_str, get_d_atom_denom_info_mock_data, get_default_instantiate_msg,
    get_message_info, get_validator_info_mock_data, setup_lsm_token_info_provider_mock,
    setup_multiple_token_info_provider_mocks, D_ATOM_ON_NEUTRON, IBC_DENOM_1, IBC_DENOM_2,
    IBC_DENOM_3, LSM_TOKEN_PROVIDER_ADDR, ONE_DAY_IN_NANO_SECONDS, ONE_MONTH_IN_NANO_SECONDS,
    VALIDATOR_1, VALIDATOR_1_LST_DENOM_1, VALIDATOR_2, VALIDATOR_2_LST_DENOM_1, VALIDATOR_3,
    VALIDATOR_3_LST_DENOM_1,
};
use crate::testing_mocks::{denom_trace_grpc_query_mock, MockQuerier};
use cosmwasm_std::testing::{MockApi, MockStorage};
use cosmwasm_std::{testing::mock_env, Coin, Decimal, Uint128};
use cosmwasm_std::{Env, OwnedDeps, StdResult, Storage};
use neutron_sdk::bindings::query::NeutronQuery;
use std::collections::HashMap;
use std::str::FromStr;

#[test]
fn pending_slashes_accumulation_test() {
    let user1 = "addr0000";
    let user2 = "addr0001";
    let user3 = "addr0002";

    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([
            (IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string()),
            (IBC_DENOM_2.to_string(), VALIDATOR_2_LST_DENOM_1.to_string()),
            (IBC_DENOM_3.to_string(), VALIDATOR_3_LST_DENOM_1.to_string()),
        ]),
    );
    let (mut deps, mut env) = (
        crate::testing_mocks::mock_dependencies(grpc_query),
        mock_env(),
    );

    // Instantiate contract with 2 tranches and allowed locking periods of 1, 2 and 3 rounds
    let mut instantiate_msg = get_default_instantiate_msg(&deps.api);
    instantiate_msg.round_length = ONE_MONTH_IN_NANO_SECONDS;
    instantiate_msg.tranches = vec![TrancheInfo {
        name: "Tranche 1".to_string(),
        metadata: "".to_string(),
    }];
    instantiate_msg.round_lock_power_schedule = vec![
        (1, Decimal::one()),
        (2, Decimal::from_str("1.25").unwrap()),
        (3, Decimal::from_str("1.5").unwrap()),
    ];
    instantiate_msg.whitelist_admins = vec![get_address_as_str(&deps.api, user1)];

    let instantiate_info = get_message_info(&deps.api, user1, &[]);
    instantiate(
        deps.as_mut(),
        env.clone(),
        instantiate_info,
        instantiate_msg,
    )
    .unwrap();

    let lsm_token_info_provider_addr = deps.api.addr_make(LSM_TOKEN_PROVIDER_ADDR);
    setup_lsm_token_info_provider_mock(
        &mut deps,
        lsm_token_info_provider_addr.clone(),
        vec![
            (
                0,
                vec![
                    (VALIDATOR_1.to_string(), Decimal::one()),
                    (VALIDATOR_2.to_string(), Decimal::one()),
                    (VALIDATOR_3.to_string(), Decimal::one()),
                ],
            ),
            (
                1,
                vec![
                    (VALIDATOR_1.to_string(), Decimal::one()),
                    (VALIDATOR_2.to_string(), Decimal::one()),
                    (VALIDATOR_3.to_string(), Decimal::one()),
                ],
            ),
        ],
        true,
    );

    // Start round 0
    env.block.time = env.block.time.plus_nanos(1000);
    env.block.height += 100;

    let round_1 = 0;
    let tranche_1 = 1;
    let proposal_1 = 0;

    let user1_lock_1 = 0;
    let user1_lock_2 = 1;
    let user1_lock_3 = 2;
    let user1_lock_4 = 3;
    let user1_lock_5 = 4;
    let user1_lock_6 = 5;

    let user2_lock_1 = 6;
    let user2_lock_2 = 7;
    let user2_lock_3 = 8;
    let user2_lock_4 = 9;
    let user2_lock_5 = 10;
    let user2_lock_6 = 11;

    let user3_lock_1 = 12;
    let user3_lock_2 = 13;
    let user3_lock_3 = 14;
    let user3_lock_4 = 15;
    let user3_lock_5 = 16;
    let user3_lock_6 = 17;

    // user1, user2 and user3 create 2 lockups for each of the possible locking periods (1, 2 and 3 rounds)
    let lock_periods = [
        ONE_MONTH_IN_NANO_SECONDS,
        ONE_MONTH_IN_NANO_SECONDS,
        2 * ONE_MONTH_IN_NANO_SECONDS,
        2 * ONE_MONTH_IN_NANO_SECONDS,
        3 * ONE_MONTH_IN_NANO_SECONDS,
        3 * ONE_MONTH_IN_NANO_SECONDS,
    ];

    let lock_amount_initial = 1000;
    let users_locking_tokens = [
        (user1, IBC_DENOM_1),
        (user2, IBC_DENOM_2),
        (user3, IBC_DENOM_3),
    ];
    for &user_locking_tokens in &users_locking_tokens {
        for &period in &lock_periods {
            let funds = vec![Coin::new(lock_amount_initial, user_locking_tokens.1)];
            let info = get_message_info(&deps.api, user_locking_tokens.0, &funds);
            let res = execute(
                deps.as_mut(),
                env.clone(),
                info,
                ExecuteMsg::LockTokens {
                    lock_duration: period,
                    proof: None,
                },
            );
            assert!(res.is_ok());
        }
    }

    // Submit proposal_1 in tranche_1
    let proposal_info = get_message_info(&deps.api, user1, &[]);
    let create_prop_res = execute(
        deps.as_mut(),
        env.clone(),
        proposal_info.clone(),
        ExecuteMsg::CreateProposal {
            round_id: None,
            tranche_id: tranche_1,
            title: "Proposal 1".to_string(),
            description: "".to_string(),
            deployment_duration: 1,
            minimum_atom_liquidity_request: Uint128::zero(),
        },
    );
    assert!(create_prop_res.is_ok());

    // user1 votes on proposal_1 with lock ids: 0, 2, 4
    vote_for_proposal(
        &mut deps,
        &env,
        user1,
        tranche_1,
        proposal_1,
        vec![user1_lock_1, user1_lock_3, user1_lock_5],
    );

    // user2 votes for proposal_1 with lock ids: 6, 8, 10
    vote_for_proposal(
        &mut deps,
        &env,
        user2,
        tranche_1,
        proposal_1,
        vec![user2_lock_1, user2_lock_3, user2_lock_5],
    );

    // Move to round 1
    env.block.time = env.block.time.plus_nanos(ONE_MONTH_IN_NANO_SECONDS);
    env.block.height += 100000;

    // Verify the maximum number of tokens that can be slashed for voting on proposal_1
    verify_expected_slashable_token_num(
        &deps,
        &env,
        round_1,
        &[(tranche_1, proposal_1, 6 * lock_amount_initial)],
    );

    // Slash proposal_1 in (round 0, tranche 1) by 11%
    // Since it gets slashed by 11%, and the threshold for applying the actual slashing is 50%,
    // this should only add information about the pending slashes to all lockups that voted.
    let slash_info = get_message_info(&deps.api, user1, &[]);
    let slash_res = execute(
        deps.as_mut(),
        env.clone(),
        slash_info.clone(),
        ExecuteMsg::SlashProposalVoters {
            round_id: round_1,
            tranche_id: tranche_1,
            proposal_id: proposal_1,
            slash_percent: Decimal::from_str("0.11").unwrap(),
            start_from: 0,
            limit: 1000,
        },
    );
    assert!(slash_res.is_ok());

    let expected_pending_slash_amount = 110;
    // Verify that the lockups still hold the initial amounts and that the pending slashes are attached
    verify_locks_and_pending_slashes(
        &deps.storage,
        vec![
            (
                user1_lock_1,
                lock_amount_initial,
                expected_pending_slash_amount,
            ),
            (user1_lock_2, lock_amount_initial, 0),
            (
                user1_lock_3,
                lock_amount_initial,
                expected_pending_slash_amount,
            ),
            (user1_lock_4, lock_amount_initial, 0),
            (
                user1_lock_5,
                lock_amount_initial,
                expected_pending_slash_amount,
            ),
            (user1_lock_6, lock_amount_initial, 0),
            (
                user2_lock_1,
                lock_amount_initial,
                expected_pending_slash_amount,
            ),
            (user2_lock_2, lock_amount_initial, 0),
            (
                user2_lock_3,
                lock_amount_initial,
                expected_pending_slash_amount,
            ),
            (user2_lock_4, lock_amount_initial, 0),
            (
                user2_lock_5,
                lock_amount_initial,
                expected_pending_slash_amount,
            ),
            (user2_lock_6, lock_amount_initial, 0),
            // user3 didn't vote, but check its lockups anyway
            (user3_lock_1, lock_amount_initial, 0),
            (user3_lock_2, lock_amount_initial, 0),
            (user3_lock_3, lock_amount_initial, 0),
            (user3_lock_4, lock_amount_initial, 0),
            (user3_lock_5, lock_amount_initial, 0),
            (user3_lock_6, lock_amount_initial, 0),
        ],
    );

    // Then slash the same proposal for additional 37.5% and verify that the pending slashes are updated
    let slash_res = execute(
        deps.as_mut(),
        env.clone(),
        slash_info.clone(),
        ExecuteMsg::SlashProposalVoters {
            round_id: round_1,
            tranche_id: 1,
            proposal_id: proposal_1,
            slash_percent: Decimal::from_str("0.375").unwrap(),
            start_from: 0,
            limit: 1000,
        },
    );
    assert!(slash_res.is_ok());
    assert_eq!(slash_res.unwrap().messages.len(), 0);

    let expected_pending_slash_amount = 485;

    // Lockups should still hold initial amount, but the pending slashes should increase
    verify_locks_and_pending_slashes(
        &deps.storage,
        vec![
            (
                user1_lock_1,
                lock_amount_initial,
                expected_pending_slash_amount,
            ),
            (
                user1_lock_3,
                lock_amount_initial,
                expected_pending_slash_amount,
            ),
            (
                user1_lock_5,
                lock_amount_initial,
                expected_pending_slash_amount,
            ),
            (
                user2_lock_1,
                lock_amount_initial,
                expected_pending_slash_amount,
            ),
            (
                user2_lock_3,
                lock_amount_initial,
                expected_pending_slash_amount,
            ),
            (
                user2_lock_5,
                lock_amount_initial,
                expected_pending_slash_amount,
            ),
        ],
    );
}

#[test]
fn slash_when_threshold_is_reached_test() {
    let user1 = "addr0000";
    let user2 = "addr0001";
    let user3 = "addr0002";

    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([
            (IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string()),
            (IBC_DENOM_2.to_string(), VALIDATOR_2_LST_DENOM_1.to_string()),
            (IBC_DENOM_3.to_string(), VALIDATOR_3_LST_DENOM_1.to_string()),
        ]),
    );
    let (mut deps, mut env) = (
        crate::testing_mocks::mock_dependencies(grpc_query),
        mock_env(),
    );

    // Instantiate contract with 2 tranches and allowed locking periods of 1, 2 and 3 rounds
    let mut instantiate_msg = get_default_instantiate_msg(&deps.api);
    instantiate_msg.round_length = ONE_MONTH_IN_NANO_SECONDS;
    instantiate_msg.tranches = vec![
        TrancheInfo {
            name: "Tranche 1".to_string(),
            metadata: "".to_string(),
        },
        TrancheInfo {
            name: "Tranche 2".to_string(),
            metadata: "".to_string(),
        },
    ];
    instantiate_msg.round_lock_power_schedule = vec![
        (1, Decimal::one()),
        (2, Decimal::from_str("1.25").unwrap()),
        (3, Decimal::from_str("1.5").unwrap()),
    ];
    instantiate_msg.whitelist_admins = vec![get_address_as_str(&deps.api, user1)];

    let instantiate_info = get_message_info(&deps.api, user1, &[]);
    instantiate(
        deps.as_mut(),
        env.clone(),
        instantiate_info,
        instantiate_msg.clone(),
    )
    .unwrap();

    let lsm_token_info_provider_addr = deps.api.addr_make(LSM_TOKEN_PROVIDER_ADDR);
    setup_lsm_token_info_provider_mock(
        &mut deps,
        lsm_token_info_provider_addr.clone(),
        vec![
            (
                0,
                vec![
                    (VALIDATOR_1.to_string(), Decimal::one()),
                    (VALIDATOR_2.to_string(), Decimal::one()),
                    (VALIDATOR_3.to_string(), Decimal::one()),
                ],
            ),
            (
                1,
                vec![
                    (VALIDATOR_1.to_string(), Decimal::one()),
                    (VALIDATOR_2.to_string(), Decimal::one()),
                    (VALIDATOR_3.to_string(), Decimal::one()),
                ],
            ),
        ],
        true,
    );

    // Start round 0
    env.block.time = env.block.time.plus_nanos(1000);
    env.block.height += 100;

    let round_1 = 0;

    let tranche_1 = 1;
    let tranche_2 = 2;

    // Round 0 proposals
    let proposal_1 = 0;
    let proposal_2 = 1;

    let user1_lock_1 = 0;
    let user1_lock_2 = 1;
    let user1_lock_3 = 2;
    let user1_lock_4 = 3;
    let user1_lock_5 = 4;
    let user1_lock_6 = 5;

    let user2_lock_1 = 6;
    let user2_lock_2 = 7;
    let user2_lock_3 = 8;
    let user2_lock_4 = 9;
    let user2_lock_5 = 10;
    let user2_lock_6 = 11;

    let user3_lock_1 = 12;
    let user3_lock_2 = 13;
    let user3_lock_3 = 14;
    let user3_lock_4 = 15;
    let user3_lock_5 = 16;
    let user3_lock_6 = 17;

    // user1, user2 and user3 create 2 lockups for each of the possible locking periods (1, 2 and 3 rounds)
    let lock_periods = [
        ONE_MONTH_IN_NANO_SECONDS,
        ONE_MONTH_IN_NANO_SECONDS,
        2 * ONE_MONTH_IN_NANO_SECONDS,
        2 * ONE_MONTH_IN_NANO_SECONDS,
        3 * ONE_MONTH_IN_NANO_SECONDS,
        3 * ONE_MONTH_IN_NANO_SECONDS,
    ];

    let lock_amount_initial = 1000;
    let users_locking_tokens = [
        (user1, IBC_DENOM_1),
        (user2, IBC_DENOM_2),
        (user3, IBC_DENOM_3),
    ];
    for &user_locking_tokens in &users_locking_tokens {
        for &period in &lock_periods {
            let funds = vec![Coin::new(lock_amount_initial, user_locking_tokens.1)];
            let info = get_message_info(&deps.api, user_locking_tokens.0, &funds);
            let res = execute(
                deps.as_mut(),
                env.clone(),
                info,
                ExecuteMsg::LockTokens {
                    lock_duration: period,
                    proof: None,
                },
            );
            assert!(res.is_ok());
        }
    }

    // Submit two proposals, one in each tranche
    let proposal_info = get_message_info(&deps.api, user1, &[]);
    for create_proposal_info in &[("Proposal 1", tranche_1), ("Proposal 2", tranche_2)] {
        let create_prop_res = execute(
            deps.as_mut(),
            env.clone(),
            proposal_info.clone(),
            ExecuteMsg::CreateProposal {
                round_id: None,
                tranche_id: create_proposal_info.1,
                title: create_proposal_info.0.to_string(),
                description: "".to_string(),
                deployment_duration: 1,
                minimum_atom_liquidity_request: Uint128::zero(),
            },
        );
        assert!(create_prop_res.is_ok());
    }

    // user1 votes on proposal_1 (tranche 1) and proposal_2 (tranche 2) with lock ids: 0, 2, 4
    vote_for_proposal(
        &mut deps,
        &env,
        user1,
        tranche_1,
        proposal_1,
        vec![user1_lock_1, user1_lock_3, user1_lock_5],
    );
    vote_for_proposal(
        &mut deps,
        &env,
        user1,
        tranche_2,
        proposal_2,
        vec![user1_lock_1, user1_lock_3, user1_lock_5],
    );

    // user2 votes for proposal_1 (tranche 1) and proposal_2 (tranche 2) with lock ids: 6, 8, 10
    vote_for_proposal(
        &mut deps,
        &env,
        user2,
        tranche_1,
        proposal_1,
        vec![user2_lock_1, user2_lock_3, user2_lock_5],
    );
    vote_for_proposal(
        &mut deps,
        &env,
        user2,
        tranche_2,
        proposal_2,
        vec![user2_lock_1, user2_lock_3, user2_lock_5],
    );

    // user3 votes for proposal_1 (tranche 1) and proposal_2 (tranche 2) with lock ids: 12, 14, 16
    vote_for_proposal(
        &mut deps,
        &env,
        user3,
        tranche_1,
        proposal_1,
        vec![user3_lock_1, user3_lock_3, user3_lock_5],
    );
    vote_for_proposal(
        &mut deps,
        &env,
        user3,
        tranche_2,
        proposal_2,
        vec![user3_lock_1, user3_lock_3, user3_lock_5],
    );

    // After user3 had voted, set its validator power ratio to 0, effectively bringing user3 votes to 0 power.
    // When it comes to slashing the proposal, such lockups will not be slashed since they did not contribute
    // to the proposal voting power, and also didn't receive any tributes for their votes.
    setup_lsm_token_info_provider_mock(
        &mut deps,
        lsm_token_info_provider_addr.clone(),
        vec![
            (
                0,
                vec![
                    (VALIDATOR_1.to_string(), Decimal::one()),
                    (VALIDATOR_2.to_string(), Decimal::one()),
                ],
            ),
            (
                1,
                vec![
                    (VALIDATOR_1.to_string(), Decimal::one()),
                    (VALIDATOR_2.to_string(), Decimal::one()),
                    (VALIDATOR_3.to_string(), Decimal::one()),
                ],
            ),
        ],
        true,
    );

    // Move to round 1
    env.block.time = env.block.time.plus_nanos(ONE_MONTH_IN_NANO_SECONDS);
    env.block.height += 100000;

    // Verify the maximum number of tokens that can be slashed for voting on proposals 1 and 2
    verify_expected_slashable_token_num(
        &deps,
        &env,
        round_1,
        &[
            (tranche_1, proposal_1, 6 * lock_amount_initial),
            (tranche_2, proposal_2, 6 * lock_amount_initial),
        ],
    );

    // Slash proposal_1 from tranche_1
    // Since it gets slashed by 25%, and the threshold for applying the actual slashing is 50%,
    // this should just add information about the pending slashes to all lockups that voted.
    let slash_info = get_message_info(&deps.api, user1, &[]);
    let slash_res = execute(
        deps.as_mut(),
        env.clone(),
        slash_info.clone(),
        ExecuteMsg::SlashProposalVoters {
            round_id: round_1,
            tranche_id: tranche_1,
            proposal_id: proposal_1,
            slash_percent: Decimal::from_str("0.25").unwrap(),
            start_from: 0,
            limit: 1000,
        },
    );
    assert!(slash_res.is_ok());

    // Get total locked tokens before slashing is applied
    let total_locked_before = LOCKED_TOKENS.load(&deps.storage).unwrap();

    // Then slash proposal_2 from tranche_2 for additional 30% and verify that the actual slashing occures.
    let slash_res = execute(
        deps.as_mut(),
        env.clone(),
        slash_info.clone(),
        ExecuteMsg::SlashProposalVoters {
            round_id: round_1,
            tranche_id: tranche_2,
            proposal_id: proposal_2,
            slash_percent: Decimal::from_str("0.3").unwrap(),
            start_from: 0,
            limit: 1000,
        },
    );

    assert!(slash_res.is_ok());
    let submsgs = slash_res.unwrap().messages;
    assert_eq!(submsgs.len(), 1);

    // All lockups have initial amount of 1000 and are slashed with 55%
    let expected_slashed_amount = 550;

    match submsgs[0].clone().msg {
        cosmwasm_std::CosmosMsg::Bank(bank_msg) => {
            match bank_msg {
                cosmwasm_std::BankMsg::Send { to_address, amount } => {
                    assert_eq!(to_address, instantiate_msg.slash_tokens_receiver_addr);

                    assert_eq!(amount.len(), 2);

                    for slashed_tokens in amount {
                        if slashed_tokens.denom != IBC_DENOM_1
                            && slashed_tokens.denom != IBC_DENOM_2
                        {
                            panic!(
                                "slashed unexpected tokens with denom: {}",
                                slashed_tokens.denom
                            );
                        }

                        // Both user1 and user2 had 3 lokcups slashed by 550 tokens
                        assert_eq!(slashed_tokens.amount.u128(), 3 * expected_slashed_amount)
                    }
                }
                _ => panic!("unexpected BankMsg type"),
            }
        }
        _ => panic!("unexpected SubMsg type"),
    }

    let lock_amount_after_slash_1 = lock_amount_initial - expected_slashed_amount;

    // Verify that amounts held by slashed lockups are reduced and that the pending slashes are removed
    verify_locks_and_pending_slashes(
        &deps.storage,
        vec![
            (user1_lock_1, lock_amount_after_slash_1, 0),
            (user1_lock_2, lock_amount_initial, 0),
            (user1_lock_3, lock_amount_after_slash_1, 0),
            (user1_lock_4, lock_amount_initial, 0),
            (user1_lock_5, lock_amount_after_slash_1, 0),
            (user1_lock_6, lock_amount_initial, 0),
            (user2_lock_1, lock_amount_after_slash_1, 0),
            (user2_lock_2, lock_amount_initial, 0),
            (user2_lock_3, lock_amount_after_slash_1, 0),
            (user2_lock_4, lock_amount_initial, 0),
            (user2_lock_5, lock_amount_after_slash_1, 0),
            (user2_lock_6, lock_amount_initial, 0),
            // user3 votes have 0 power, therefore no slashing is applied to them
            (user3_lock_1, lock_amount_initial, 0),
            (user3_lock_2, lock_amount_initial, 0),
            (user3_lock_3, lock_amount_initial, 0),
            (user3_lock_4, lock_amount_initial, 0),
            (user3_lock_5, lock_amount_initial, 0),
            (user3_lock_6, lock_amount_initial, 0),
        ],
    );

    let total_locked_after = LOCKED_TOKENS.load(&deps.storage).unwrap();

    // Total of 6 lockups were slashed by 550 tokens each
    assert_eq!(
        total_locked_before,
        total_locked_after + 6 * expected_slashed_amount
    );

    // Verify the maximum number of tokens that can be slashed for voting on proposals 1 and 2.
    // Since actual slashing was applied and all lockups that voted were slashed by 550 tokens,
    // the maximum number of tokens that can be slashed is reduced to 450 per each lockup.
    verify_expected_slashable_token_num(
        &deps,
        &env,
        round_1,
        &[
            (tranche_1, proposal_1, 6 * lock_amount_after_slash_1),
            (tranche_2, proposal_2, 6 * lock_amount_after_slash_1),
        ],
    );

    // Then slash proposal_2 from tranche_2 again with 17% and verify that pending slashes are attached again.
    let slash_res = execute(
        deps.as_mut(),
        env.clone(),
        slash_info.clone(),
        ExecuteMsg::SlashProposalVoters {
            round_id: 0,
            tranche_id: tranche_2,
            proposal_id: proposal_2,
            slash_percent: Decimal::from_str("0.17").unwrap(),
            start_from: 0,
            limit: 1000,
        },
    );
    assert!(slash_res.is_ok());

    // 17% slash is applied to the amount held by the lockup at the time of voting.
    let expected_pending_slash_amount = 170;

    // Verify that pending slashes are applied
    verify_locks_and_pending_slashes(
        &deps.storage,
        vec![
            (
                user1_lock_1,
                lock_amount_after_slash_1,
                expected_pending_slash_amount,
            ),
            (user1_lock_2, lock_amount_initial, 0),
            (
                user1_lock_3,
                lock_amount_after_slash_1,
                expected_pending_slash_amount,
            ),
            (user1_lock_4, lock_amount_initial, 0),
            (
                user1_lock_5,
                lock_amount_after_slash_1,
                expected_pending_slash_amount,
            ),
            (user1_lock_6, lock_amount_initial, 0),
            (
                user2_lock_1,
                lock_amount_after_slash_1,
                expected_pending_slash_amount,
            ),
            (user2_lock_2, lock_amount_initial, 0),
            (
                user2_lock_3,
                lock_amount_after_slash_1,
                expected_pending_slash_amount,
            ),
            (user2_lock_4, lock_amount_initial, 0),
            (
                user2_lock_5,
                lock_amount_after_slash_1,
                expected_pending_slash_amount,
            ),
            (user2_lock_6, lock_amount_initial, 0),
            // user3 votes have 0 power, therefore no slashing is applied to them
            (user3_lock_1, lock_amount_initial, 0),
            (user3_lock_2, lock_amount_initial, 0),
            (user3_lock_3, lock_amount_initial, 0),
            (user3_lock_4, lock_amount_initial, 0),
            (user3_lock_5, lock_amount_initial, 0),
            (user3_lock_6, lock_amount_initial, 0),
        ],
    );

    // Verify the maximum number of tokens that can be slashed for voting on proposals 1 and 2.
    // Adding pending slashes does not affect the maximum number of tokens that can be slashed.
    verify_expected_slashable_token_num(
        &deps,
        &env,
        round_1,
        &[
            (tranche_1, proposal_1, 6 * lock_amount_after_slash_1),
            (tranche_2, proposal_2, 6 * lock_amount_after_slash_1),
        ],
    );
}

#[test]
fn slashing_removes_lockups_test() {
    let user1 = "addr0000";
    let user2 = "addr0001";

    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([
            (IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string()),
            (IBC_DENOM_2.to_string(), VALIDATOR_2_LST_DENOM_1.to_string()),
        ]),
    );
    let (mut deps, mut env) = (
        crate::testing_mocks::mock_dependencies(grpc_query),
        mock_env(),
    );

    let user1_addr = deps.api.addr_make(user1);
    let user2_addr = deps.api.addr_make(user2);

    // Instantiate contract with 1 tranche and allowed locking periods of 1, 2 and 3 rounds
    let mut instantiate_msg = get_default_instantiate_msg(&deps.api);
    instantiate_msg.round_length = ONE_MONTH_IN_NANO_SECONDS;
    instantiate_msg.tranches = vec![TrancheInfo {
        name: "Tranche 1".to_string(),
        metadata: "".to_string(),
    }];
    instantiate_msg.round_lock_power_schedule = vec![
        (1, Decimal::one()),
        (2, Decimal::from_str("1.25").unwrap()),
        (3, Decimal::from_str("1.5").unwrap()),
    ];
    instantiate_msg.whitelist_admins = vec![get_address_as_str(&deps.api, user1)];

    let instantiate_info = get_message_info(&deps.api, user1, &[]);
    instantiate(
        deps.as_mut(),
        env.clone(),
        instantiate_info,
        instantiate_msg,
    )
    .unwrap();

    // Start round 0
    env.block.time = env.block.time.plus_nanos(1000);
    env.block.height += 100;

    let round_1 = 0;
    let round_2 = 1;
    let tranche_1 = 1;
    let proposal_1 = 0;
    let proposal_2 = 1;

    let user1_lock_1 = 0;
    let user2_lock_1 = 1;

    let lsm_token_info_provider_addr = deps.api.addr_make(LSM_TOKEN_PROVIDER_ADDR);
    setup_lsm_token_info_provider_mock(
        &mut deps,
        lsm_token_info_provider_addr.clone(),
        vec![
            (
                round_1,
                vec![
                    (VALIDATOR_1.to_string(), Decimal::one()),
                    (VALIDATOR_2.to_string(), Decimal::one()),
                ],
            ),
            (
                round_2,
                vec![
                    (VALIDATOR_1.to_string(), Decimal::one()),
                    (VALIDATOR_2.to_string(), Decimal::one()),
                ],
            ),
        ],
        true,
    );

    // user1 and user2 create one lockup each
    let lock_amount_initial = 1000u128;
    let users_locking_tokens = [(user1, IBC_DENOM_1), (user2, IBC_DENOM_2)];
    for &user_locking_tokens in &users_locking_tokens {
        let funds = vec![Coin::new(lock_amount_initial, user_locking_tokens.1)];
        let info = get_message_info(&deps.api, user_locking_tokens.0, &funds);
        let res = execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::LockTokens {
                lock_duration: 3 * ONE_MONTH_IN_NANO_SECONDS,
                proof: None,
            },
        );
        assert!(res.is_ok());
    }

    // Submit proposal_1 in tranche_1
    let proposal_info = get_message_info(&deps.api, user1, &[]);
    let create_prop_res = execute(
        deps.as_mut(),
        env.clone(),
        proposal_info.clone(),
        ExecuteMsg::CreateProposal {
            round_id: None,
            tranche_id: tranche_1,
            title: "Proposal 1".to_string(),
            description: "".to_string(),
            deployment_duration: 1,
            minimum_atom_liquidity_request: Uint128::zero(),
        },
    );
    assert!(create_prop_res.is_ok());

    // user1 votes on proposal_1 with lock id 0
    vote_for_proposal(
        &mut deps,
        &env,
        user1,
        tranche_1,
        proposal_1,
        vec![user1_lock_1],
    );

    // user2 votes for proposal_1 with lock id 1
    vote_for_proposal(
        &mut deps,
        &env,
        user2,
        tranche_1,
        proposal_1,
        vec![user2_lock_1],
    );

    // Move to round 1
    env.block.time = env.block.time.plus_nanos(ONE_MONTH_IN_NANO_SECONDS);
    env.block.height += 100000;

    // Submit proposal_2 in tranche_1. This allows us to verify that the
    // current round votes are removed together with the removed lockups.
    let proposal_info = get_message_info(&deps.api, user1, &[]);
    let create_prop_res = execute(
        deps.as_mut(),
        env.clone(),
        proposal_info.clone(),
        ExecuteMsg::CreateProposal {
            round_id: None,
            tranche_id: tranche_1,
            title: "Proposal 2".to_string(),
            description: "".to_string(),
            deployment_duration: 1,
            minimum_atom_liquidity_request: Uint128::zero(),
        },
    );
    assert!(create_prop_res.is_ok());

    // user1 votes on proposal_2 with lock id 0
    vote_for_proposal(
        &mut deps,
        &env,
        user1,
        tranche_1,
        proposal_2,
        vec![user1_lock_1],
    );

    // user2 votes for proposal_2 with lock id 1
    vote_for_proposal(
        &mut deps,
        &env,
        user2,
        tranche_1,
        proposal_2,
        vec![user2_lock_1],
    );

    // Verify that the votes existed before slashing
    assert!(VOTE_MAP_V2
        .may_load(&deps.storage, ((round_2, tranche_1), user1_lock_1))
        .unwrap()
        .is_some());
    assert!(VOTE_MAP_V2
        .may_load(&deps.storage, ((round_2, tranche_1), user2_lock_1))
        .unwrap()
        .is_some());

    // Verify the maximum number of tokens that can be slashed for voting on proposal 1
    verify_expected_slashable_token_num(
        &deps,
        &env,
        round_1,
        &[(tranche_1, proposal_1, 2 * lock_amount_initial)],
    );

    // Slash proposal_1 in (round 0, tranche 1) by 49%. This should only atach pending slashes.
    let slash_info = get_message_info(&deps.api, user1, &[]);
    let slash_res = execute(
        deps.as_mut(),
        env.clone(),
        slash_info.clone(),
        ExecuteMsg::SlashProposalVoters {
            round_id: round_1,
            tranche_id: tranche_1,
            proposal_id: proposal_1,
            slash_percent: Decimal::from_str("0.49").unwrap(),
            start_from: 0,
            limit: 1000,
        },
    );
    assert!(slash_res.is_ok());

    // Then slash the same proposal for additional 60%. This is a corner case that verifies if the calculated
    // amount to slash is greater than the amount held by the lockup, then we should slash the entire lockup.
    let slash_res = execute(
        deps.as_mut(),
        env.clone(),
        slash_info.clone(),
        ExecuteMsg::SlashProposalVoters {
            round_id: round_1,
            tranche_id: tranche_1,
            proposal_id: proposal_1,
            slash_percent: Decimal::from_str("0.6").unwrap(),
            start_from: 0,
            limit: 1000,
        },
    );
    assert!(slash_res.is_ok());

    // Verify that the lock infos are removed from all relevant stores.
    for user_locks in &[(user1_addr, user1_lock_1), (user2_addr, user2_lock_1)] {
        assert!(LOCKS_MAP_V2
            .may_load(&deps.storage, user_locks.1)
            .unwrap()
            .is_none());
        assert!(LOCKS_PENDING_SLASHES
            .may_load(&deps.storage, user_locks.1)
            .unwrap()
            .is_none());
        assert!(VOTING_ALLOWED_ROUND
            .may_load(&deps.storage, (tranche_1, user_locks.1))
            .unwrap()
            .is_none());
        assert_eq!(
            USER_LOCKS
                .load(&deps.storage, user_locks.0.clone())
                .unwrap()
                .len(),
            0
        );
        assert!(VOTE_MAP_V2
            .may_load(&deps.storage, ((round_2, tranche_1), user_locks.1))
            .unwrap()
            .is_none());
    }

    // Verify the maximum number of tokens that can be slashed for voting on proposal 1
    // Since the lockups were removed during previous slash action, the maximum number
    // of tokens that can be slashed should be 0.
    verify_expected_slashable_token_num(&deps, &env, round_1, &[(tranche_1, proposal_1, 0)]);
}

#[test]
fn proposals_and_rounds_power_updates_on_slashing_test() {
    let user1 = "addr0000";
    let user2 = "addr0001";

    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([
            (IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string()),
            (IBC_DENOM_2.to_string(), VALIDATOR_2_LST_DENOM_1.to_string()),
        ]),
    );
    let (mut deps, mut env) = (
        crate::testing_mocks::mock_dependencies(grpc_query),
        mock_env(),
    );

    // Instantiate contract with 2 tranches and allowed locking periods of 1, 2 and 3 rounds
    let mut instantiate_msg = get_default_instantiate_msg(&deps.api);
    instantiate_msg.round_length = ONE_MONTH_IN_NANO_SECONDS;
    instantiate_msg.tranches = vec![
        TrancheInfo {
            name: "Tranche 1".to_string(),
            metadata: "".to_string(),
        },
        TrancheInfo {
            name: "Tranche 2".to_string(),
            metadata: "".to_string(),
        },
    ];
    instantiate_msg.round_lock_power_schedule = vec![
        (1, Decimal::one()),
        (2, Decimal::from_str("1.25").unwrap()),
        (3, Decimal::from_str("1.5").unwrap()),
    ];
    instantiate_msg.whitelist_admins = vec![get_address_as_str(&deps.api, user1)];

    let instantiate_info = get_message_info(&deps.api, user1, &[]);
    instantiate(
        deps.as_mut(),
        env.clone(),
        instantiate_info,
        instantiate_msg.clone(),
    )
    .unwrap();

    let lsm_token_info_provider_addr = deps.api.addr_make(LSM_TOKEN_PROVIDER_ADDR);
    setup_lsm_token_info_provider_mock(
        &mut deps,
        lsm_token_info_provider_addr.clone(),
        vec![
            (
                0,
                vec![
                    (VALIDATOR_1.to_string(), Decimal::one()),
                    (VALIDATOR_2.to_string(), Decimal::one()),
                ],
            ),
            (
                1,
                vec![
                    (VALIDATOR_1.to_string(), Decimal::one()),
                    (VALIDATOR_2.to_string(), Decimal::one()),
                ],
            ),
        ],
        true,
    );

    let round_1 = 0;
    let round_2 = 1;
    let round_3 = 2;
    let tranche_1 = 1;
    let tranche_2 = 2;

    // Round 0 proposals
    let proposal_1 = 0;
    // Round 1 proposals
    let proposal_2 = 1;
    let proposal_3 = 2;

    let user1_lock_1 = 0;
    let user2_lock_1 = 1;

    // Start round 0
    env.block.time = env.block.time.plus_nanos(1000);
    env.block.height += 100;

    // user1 and user2 create one lockup each
    let lock_amount_initial = 1000u128;
    let users_locking_tokens = [(user1, IBC_DENOM_1), (user2, IBC_DENOM_2)];
    for &user_locking_tokens in &users_locking_tokens {
        let funds = vec![Coin::new(lock_amount_initial, user_locking_tokens.1)];
        let info = get_message_info(&deps.api, user_locking_tokens.0, &funds);
        let res = execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::LockTokens {
                lock_duration: 3 * ONE_MONTH_IN_NANO_SECONDS,
                proof: None,
            },
        );
        assert!(res.is_ok());
    }

    // Submit single proposal in tranche_1
    let proposal_info = get_message_info(&deps.api, user1, &[]);
    let create_prop_res = execute(
        deps.as_mut(),
        env.clone(),
        proposal_info.clone(),
        ExecuteMsg::CreateProposal {
            round_id: None,
            tranche_id: tranche_1,
            title: "Proposal 1".to_string(),
            description: "".to_string(),
            deployment_duration: 1,
            minimum_atom_liquidity_request: Uint128::zero(),
        },
    );
    assert!(create_prop_res.is_ok());

    // user1 votes on proposal_1 in tranche_1
    vote_for_proposal(
        &mut deps,
        &env,
        user1,
        tranche_1,
        proposal_1,
        vec![user1_lock_1],
    );

    // user2 votes on proposal_1 in tranche_1
    vote_for_proposal(
        &mut deps,
        &env,
        user2,
        tranche_1,
        proposal_1,
        vec![user2_lock_1],
    );

    // Move to round 1
    env.block.time = env.block.time.plus_nanos(ONE_MONTH_IN_NANO_SECONDS);
    env.block.height += 100000;

    // Submit two proposals, one in each tranche
    for create_proposal_info in &[("Proposal 2", tranche_1), ("Proposal 3", tranche_2)] {
        let create_prop_res = execute(
            deps.as_mut(),
            env.clone(),
            proposal_info.clone(),
            ExecuteMsg::CreateProposal {
                round_id: None,
                tranche_id: create_proposal_info.1,
                title: create_proposal_info.0.to_string(),
                description: "".to_string(),
                deployment_duration: 1,
                minimum_atom_liquidity_request: Uint128::zero(),
            },
        );
        assert!(create_prop_res.is_ok());
    }

    // user1 and user2 vote on proposal_2 in tranche_1 and proposal_3 in tranche_2
    for user_voting in &[(user1, user1_lock_1), (user2, user2_lock_1)] {
        vote_for_proposal(
            &mut deps,
            &env,
            user_voting.0,
            tranche_1,
            proposal_2,
            vec![user_voting.1],
        );
        vote_for_proposal(
            &mut deps,
            &env,
            user_voting.0,
            tranche_2,
            proposal_3,
            vec![user_voting.1],
        );
    }

    let round_1_total_power = query_round_total_power(deps.as_ref(), round_1)
        .unwrap()
        .total_voting_power;
    let round_2_total_power = query_round_total_power(deps.as_ref(), round_2)
        .unwrap()
        .total_voting_power;
    let round_3_total_power = query_round_total_power(deps.as_ref(), round_3)
        .unwrap()
        .total_voting_power;

    assert_eq!(round_1_total_power.u128(), 3000);
    assert_eq!(round_2_total_power.u128(), 2500);
    assert_eq!(round_3_total_power.u128(), 2000);

    let proposal2_power = PROPOSAL_MAP
        .load(&deps.storage, (round_2, tranche_1, proposal_2))
        .unwrap()
        .power;
    let proposal3_power = PROPOSAL_MAP
        .load(&deps.storage, (round_2, tranche_2, proposal_3))
        .unwrap()
        .power;

    // Both lockups have voting power 1250, hence the proposal powers are 2500.
    assert_eq!(proposal2_power.u128(), 2500);
    assert_eq!(proposal3_power.u128(), 2500);

    // Slash proposal_1 from (round_1, tranche_1) with 57%.
    let slash_percent = Decimal::from_str("0.57").unwrap();
    let slash_info = get_message_info(&deps.api, user1, &[]);
    let slash_res = execute(
        deps.as_mut(),
        env.clone(),
        slash_info.clone(),
        ExecuteMsg::SlashProposalVoters {
            round_id: 0,
            tranche_id: tranche_1,
            proposal_id: proposal_1,
            slash_percent,
            start_from: 0,
            limit: 1000,
        },
    );
    assert!(slash_res.is_ok());

    let round_1_total_power = query_round_total_power(deps.as_ref(), round_1)
        .unwrap()
        .total_voting_power;
    let round_2_total_power = query_round_total_power(deps.as_ref(), round_2)
        .unwrap()
        .total_voting_power;
    let round_3_total_power = query_round_total_power(deps.as_ref(), round_3)
        .unwrap()
        .total_voting_power;

    // Slashing occured in round_2 so the round_1 total power should not change.
    assert_eq!(round_1_total_power.u128(), 3000);
    // In round_2 and round_3 total voting power should be reduced by 57%
    assert_eq!(round_2_total_power.u128(), 1074);
    assert_eq!(round_3_total_power.u128(), 860);

    let proposal2_power_after = PROPOSAL_MAP
        .load(&deps.storage, (round_2, tranche_1, proposal_2))
        .unwrap()
        .power;
    let proposal3_power_after = PROPOSAL_MAP
        .load(&deps.storage, (round_2, tranche_2, proposal_3))
        .unwrap()
        .power;

    // Due to the rounding down when caculating each lockup voting power, proposal powers
    // after slashing are 1074 instead of the expected 1075 (43% of 2500).
    assert_eq!(proposal2_power_after.u128(), 1074);
    assert_eq!(proposal3_power_after.u128(), 1074);
}

// Verify that lockups that originated from a split/merge sequence will not escape slashing
// in case of original lockup had voted on a proposal that gets slashed.
#[test]
fn slashing_after_lock_split_merge_test() {
    let user1 = "addr0000";

    let round_1 = 0;
    let round_2 = 1;
    let tranche_1 = 1;
    let proposal_1 = 0;

    // Initial lockup
    let user1_lock_1 = 0;
    let user1_lock_2 = 1;
    // New lockups after first split
    let user1_lock_3 = 2;
    let user1_lock_4 = 3;
    // New lockups after second split
    let user1_lock_5 = 4;
    let user1_lock_6 = 5;
    // New lockup after merge
    let user1_lock_7 = 6;

    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
    );
    let (mut deps, mut env) = (
        crate::testing_mocks::mock_dependencies(grpc_query),
        mock_env(),
    );

    // Instantiate contract with allowed locking periods of 1, 2 and 3 rounds
    let mut instantiate_msg = get_default_instantiate_msg(&deps.api);
    instantiate_msg.round_length = ONE_MONTH_IN_NANO_SECONDS;
    instantiate_msg.tranches = vec![TrancheInfo {
        name: "Tranche 1".to_string(),
        metadata: "".to_string(),
    }];
    instantiate_msg.round_lock_power_schedule = vec![
        (1, Decimal::one()),
        (2, Decimal::from_str("1.25").unwrap()),
        (3, Decimal::from_str("1.5").unwrap()),
    ];
    instantiate_msg.whitelist_admins = vec![get_address_as_str(&deps.api, user1)];

    let message_info = get_message_info(&deps.api, user1, &[]);
    instantiate(
        deps.as_mut(),
        env.clone(),
        message_info.clone(),
        instantiate_msg.clone(),
    )
    .unwrap();

    let lsm_token_info_provider_addr = deps.api.addr_make(LSM_TOKEN_PROVIDER_ADDR);
    setup_lsm_token_info_provider_mock(
        &mut deps,
        lsm_token_info_provider_addr.clone(),
        vec![
            (round_1, vec![(VALIDATOR_1.to_string(), Decimal::one())]),
            (round_2, vec![(VALIDATOR_1.to_string(), Decimal::one())]),
        ],
        true,
    );

    // Advance block time and height by one day
    env.block.time = env.block.time.plus_nanos(ONE_DAY_IN_NANO_SECONDS);
    env.block.height += 100;

    // Create proposal 1 in (round 0, tranche 1)
    let create_prop1 = execute(
        deps.as_mut(),
        env.clone(),
        message_info.clone(),
        ExecuteMsg::CreateProposal {
            round_id: None,
            tranche_id: tranche_1,
            title: "Proposal 1".to_string(),
            description: "P1".to_string(),
            deployment_duration: 1,
            minimum_atom_liquidity_request: Uint128::zero(),
        },
    );
    assert!(create_prop1.is_ok());

    // Have user create two lockups to lock 90.0000 and 10.000 tokens for 3 rounds
    for lock_amount in [90000u128, 10000u128] {
        let funds = vec![Coin::new(lock_amount, IBC_DENOM_1)];
        let info = get_message_info(&deps.api, user1, &funds);
        let res = execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::LockTokens {
                lock_duration: 3 * ONE_MONTH_IN_NANO_SECONDS,
                proof: None,
            },
        );
        assert!(res.is_ok());
    }

    // User votes on proposal 1 with user1_lock_1
    vote_for_proposal(
        &mut deps,
        &env,
        user1,
        tranche_1,
        proposal_1,
        vec![user1_lock_1],
    );

    env.block.time = env.block.time.plus_nanos(ONE_DAY_IN_NANO_SECONDS);
    env.block.height += 100;

    // While still in round 0, split user1_lock_1 into 600 and 300 tokens lockups
    // (user1_lock_3 and user1_lock_4 are created)
    let split_res = execute(
        deps.as_mut(),
        env.clone(),
        message_info.clone(),
        ExecuteMsg::SplitLock {
            lock_id: user1_lock_1,
            amount: Uint128::from(30000u128),
        },
    );
    assert!(split_res.is_ok());

    // Move to round 1
    env.block.time = env.block.time.plus_nanos(ONE_MONTH_IN_NANO_SECONDS);
    env.block.height += 100000;

    // Split user1_lock_3 into two lockups with both holding 300 tokens (user1_lock_5 and user1_lock_6 are created)
    let split_res2 = execute(
        deps.as_mut(),
        env.clone(),
        message_info.clone(),
        ExecuteMsg::SplitLock {
            lock_id: user1_lock_3,
            amount: Uint128::from(30000u128),
        },
    );
    assert!(split_res2.is_ok());

    // Merge lockups user1_lock_2, user1_lock_5, user1_lock_6 into one lockup and verify that
    // the new lockup (user1_lock_7) holds 70.000 tokens
    let merge_res = execute(
        deps.as_mut(),
        env.clone(),
        message_info.clone(),
        ExecuteMsg::MergeLocks {
            lock_ids: vec![user1_lock_2, user1_lock_5, user1_lock_6],
        },
    );
    assert!(merge_res.is_ok());

    let merged_lock_7 = LOCKS_MAP_V2
        .may_load(&deps.storage, user1_lock_7)
        .unwrap()
        .unwrap();
    assert_eq!(merged_lock_7.funds.amount, Uint128::from(70000u128));

    // Verify the maximum number of tokens that can be slashed for voting on proposal 1
    // The lockup that initially voted on this proposal (user1_lock_1) had 90.000 tokens,
    // and it was split and merged multiple times, but the maximum number of tokens that
    // can be slashed for voting on proposal 1 should remain 90.000 tokens.
    verify_expected_slashable_token_num(&deps, &env, round_1, &[(tranche_1, proposal_1, 90000)]);

    // Slash proposal 1 with 60%
    let slash_res = execute(
        deps.as_mut(),
        env.clone(),
        message_info.clone(),
        ExecuteMsg::SlashProposalVoters {
            round_id: round_1,
            tranche_id: tranche_1,
            proposal_id: proposal_1,
            slash_percent: Decimal::percent(60),
            start_from: 0,
            limit: 1000,
        },
    );
    assert!(slash_res.is_ok());

    // user1_lock_4: 30.000 tokens, all inherited from user1_lock_1
    // should be slashed by 60% (18.000 tokens) = 12.000 tokens left
    let lock_4 = LOCKS_MAP_V2
        .may_load(&deps.storage, user1_lock_4)
        .unwrap()
        .unwrap();
    assert_eq!(lock_4.funds.amount.u128(), 12000u128);

    // user1_lock_7: originally 70.000 tokens, inherited 60.000 tokens from user1_lock_1
    // should be slashed by 60% (36.000 tokens) = 34.000 tokens left.
    let lock_7 = LOCKS_MAP_V2
        .may_load(&deps.storage, user1_lock_7)
        .unwrap()
        .unwrap();
    assert_eq!(lock_7.funds.amount, Uint128::from(34000u128));
}

// Verify that the amount to slash is correctly calculated when the lockup is converted into one holding dtokens
#[test]
fn slash_after_dtoken_conversion_test() {
    let user1 = "addr0000";

    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
    );
    let (mut deps, mut env) = (
        crate::testing_mocks::mock_dependencies(grpc_query),
        mock_env(),
    );

    // Instantiate contract with allowed locking periods of 1, 2 and 3 rounds
    let mut instantiate_msg = get_default_instantiate_msg(&deps.api);
    instantiate_msg.round_length = ONE_MONTH_IN_NANO_SECONDS;
    instantiate_msg.tranches = vec![TrancheInfo {
        name: "Tranche 1".to_string(),
        metadata: "".to_string(),
    }];
    instantiate_msg.round_lock_power_schedule = vec![
        (1, Decimal::one()),
        (2, Decimal::from_str("1.25").unwrap()),
        (3, Decimal::from_str("1.5").unwrap()),
    ];
    instantiate_msg.whitelist_admins = vec![get_address_as_str(&deps.api, user1)];

    let message_info = get_message_info(&deps.api, user1, &[]);
    instantiate(
        deps.as_mut(),
        env.clone(),
        message_info.clone(),
        instantiate_msg.clone(),
    )
    .unwrap();

    let d_token_info_provider_addr = deps.api.addr_make("dtoken_info_provider");
    let d_atom_ratio = Decimal::from_str("1.17").unwrap();

    let derivative_providers = HashMap::from([get_d_atom_denom_info_mock_data(
        d_token_info_provider_addr.to_string(),
        (0..=1)
            .map(|round_id: u64| (round_id, d_atom_ratio))
            .collect(),
    )]);

    let lsm_token_info_provider_addr = deps.api.addr_make(LSM_TOKEN_PROVIDER_ADDR);
    let lsm_provider = Some((
        lsm_token_info_provider_addr.to_string(),
        HashMap::from_iter((0..=1).map(|round_id: u64| {
            (
                round_id,
                HashMap::from([get_validator_info_mock_data(
                    VALIDATOR_1.to_string(),
                    Decimal::one(),
                )]),
            )
        })),
    ));

    setup_multiple_token_info_provider_mocks(
        &mut deps,
        derivative_providers.clone(),
        lsm_provider.clone(),
        true,
    );

    // Start round 0
    env.block.time = env.block.time.plus_nanos(1000);
    env.block.height += 100;

    let tranche_1 = 1;
    let proposal_1 = 0;
    let user1_lock_1 = 0;

    let lock_amount_initial = 1000u128;

    let funds = vec![Coin::new(lock_amount_initial, IBC_DENOM_1)];
    let lcoking_info = get_message_info(&deps.api, user1, &funds);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        lcoking_info,
        ExecuteMsg::LockTokens {
            lock_duration: 3 * ONE_MONTH_IN_NANO_SECONDS,
            proof: None,
        },
    );
    assert!(res.is_ok());

    let create_prop_res = execute(
        deps.as_mut(),
        env.clone(),
        message_info.clone(),
        ExecuteMsg::CreateProposal {
            round_id: None,
            tranche_id: tranche_1,
            title: "Proposal 1".to_string(),
            description: "".to_string(),
            deployment_duration: 1,
            minimum_atom_liquidity_request: Uint128::zero(),
        },
    );
    assert!(create_prop_res.is_ok());

    // user1 votes on proposal_1
    vote_for_proposal(
        &mut deps,
        &env,
        user1,
        tranche_1,
        proposal_1,
        vec![user1_lock_1],
    );

    // Move to round 1
    env.block.time = env.block.time.plus_nanos(ONE_MONTH_IN_NANO_SECONDS);
    env.block.height += 100000;

    // Mock as if user converted their lockup to dTOKEN one.
    LOCKS_MAP_V2
        .update(
            &mut deps.storage,
            user1_lock_1,
            env.block.height,
            |lock: Option<LockEntryV2>| -> StdResult<LockEntryV2> {
                let mut lock = lock.unwrap();

                let datom_amount = Decimal::from_ratio(lock.funds.amount, Uint128::one())
                    .checked_div(d_atom_ratio)
                    .unwrap()
                    .to_uint_floor();

                lock.funds = Coin {
                    denom: D_ATOM_ON_NEUTRON.to_string(),
                    amount: datom_amount,
                };

                Ok(lock)
            },
        )
        .unwrap();

    // Verify the maximum number of tokens that can be slashed for voting on proposal 1
    verify_expected_slashable_token_num(
        &deps,
        &env,
        0,
        &[(tranche_1, proposal_1, 999)], // should be 1000, but rounding down in our math made it 999
    );

    // Slash proposal_1 from tranche_1 by 55%
    let slash_info = get_message_info(&deps.api, user1, &[]);
    let slash_res = execute(
        deps.as_mut(),
        env.clone(),
        slash_info.clone(),
        ExecuteMsg::SlashProposalVoters {
            round_id: 0,
            tranche_id: tranche_1,
            proposal_id: proposal_1,
            slash_percent: Decimal::from_str("0.55").unwrap(),
            start_from: 0,
            limit: 1000,
        },
    );
    assert!(slash_res.is_ok());
    let submsgs = slash_res.unwrap().messages;

    // initial_amount (1000) / d_token_ratio (1.17) * slash_percent (0.55) = 470
    let expected_slashed_amount = 470;

    match submsgs[0].clone().msg {
        cosmwasm_std::CosmosMsg::Bank(bank_msg) => match bank_msg {
            cosmwasm_std::BankMsg::Send { to_address, amount } => {
                assert_eq!(to_address, instantiate_msg.slash_tokens_receiver_addr);

                assert_eq!(amount.len(), 1);
                let slashed_token = amount[0].clone();

                if slashed_token.denom != D_ATOM_ON_NEUTRON {
                    panic!(
                        "slashed unexpected tokens with denom: {}",
                        slashed_token.denom
                    );
                }

                assert_eq!(slashed_token.amount.u128(), expected_slashed_amount)
            }
            _ => panic!("unexpected BankMsg type"),
        },
        _ => panic!("unexpected SubMsg type"),
    }

    // Verify that amounts held by slashed lockups are reduced and that the pending slashes are removed
    verify_locks_and_pending_slashes(&deps.storage, vec![(user1_lock_1, 384, 0)]);
}

fn vote_for_proposal(
    deps: &mut OwnedDeps<MockStorage, MockApi, MockQuerier, NeutronQuery>,
    env: &Env,
    user1: &str,
    tranche_id: u64,
    proposal_id: u64,
    lock_ids: Vec<u64>,
) {
    let vote_info1 = get_message_info(&deps.api, user1, &[]);
    let vote_res1 = execute(
        deps.as_mut(),
        env.clone(),
        vote_info1,
        ExecuteMsg::Vote {
            tranche_id,
            proposals_votes: vec![crate::msg::ProposalToLockups {
                proposal_id,
                lock_ids,
            }],
        },
    );
    assert!(vote_res1.is_ok());
}

// expected_results: Vec<(lock_id, expected_lock_amount, expected_pending_slash)>
fn verify_locks_and_pending_slashes(
    storage: &dyn Storage,
    expected_results: Vec<(u64, u128, u128)>,
) {
    for expected_result in expected_results {
        assert_eq!(
            LOCKS_PENDING_SLASHES
                .may_load(storage, expected_result.0)
                .unwrap()
                .unwrap_or_default()
                .u128(),
            expected_result.2
        );

        assert_eq!(
            LOCKS_MAP_V2
                .load(storage, expected_result.0)
                .unwrap()
                .funds
                .amount
                .u128(),
            expected_result.1
        );
    }
}

// expected_slashable_amounts: &[(tranche_id, proposal_id, expected_max_slashable_token_num)]
fn verify_expected_slashable_token_num(
    deps: &OwnedDeps<cosmwasm_std::MemoryStorage, MockApi, MockQuerier, NeutronQuery>,
    env: &Env,
    round_id: u64,
    expected_slashable_amounts: &[(u64, u64, u128)],
) {
    for expected_slashable_amount in expected_slashable_amounts {
        let max_slashable_token_num = query_slashable_token_num_for_voting_on_proposal(
            deps.as_ref(),
            env.clone(),
            round_id,
            expected_slashable_amount.0,
            expected_slashable_amount.1,
        )
        .unwrap();
        assert_eq!(max_slashable_token_num.u128(), expected_slashable_amount.2);
    }
}
