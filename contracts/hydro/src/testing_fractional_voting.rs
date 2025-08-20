use std::{collections::HashMap, slice::Iter};

use cosmwasm_std::{
    testing::{mock_env, MockApi, MockStorage},
    Addr, Coin, Env, OwnedDeps, Uint128,
};
use neutron_sdk::bindings::query::NeutronQuery;

use crate::{
    contract::{execute, instantiate, query_proposal, query_user_votes},
    msg::{ExecuteMsg, ProposalToLockups},
    testing::{
        get_default_instantiate_msg, get_message_info, IBC_DENOM_1, IBC_DENOM_2, VALIDATOR_1,
        VALIDATOR_1_LST_DENOM_1, VALIDATOR_2, VALIDATOR_2_LST_DENOM_1,
    },
    testing_lsm_integration::set_validator_infos_for_round,
    testing_mocks::{denom_trace_grpc_query_mock, mock_dependencies, MockQuerier},
};

#[derive(Clone)]
struct TestLockup {
    lockup_id: u64,
    token: Coin,
}
struct TestProposalPower {
    proposal_id: u64,
    power: u128,
}
struct TestUserVote {
    proposal_id: u64,
    power: u128,
}
struct TestLockAndVote {
    lockups_to_create: Vec<TestLockup>,
    votes: Vec<ProposalToLockups>,
    expected_error: Option<String>,
    expected_proposal_powers: Vec<TestProposalPower>,
    expected_user_votes: Vec<TestUserVote>,
}
struct TestAddMoreLocks {
    more_lockups_to_create: Vec<TestLockup>,
    expected_proposal_powers: Vec<TestProposalPower>,
    expected_user_votes: Vec<TestUserVote>,
}
struct TestRefreshLocks {
    lockups_to_refresh: Vec<TestLockup>,
    expected_proposal_powers: Vec<TestProposalPower>,
    expected_user_votes: Vec<TestUserVote>,
}
struct FractionalVotingTestCase {
    description: &'static str,
    voter_address: &'static str,
    lock_and_vote: TestLockAndVote,
    add_more_locks: Option<TestAddMoreLocks>,
    refresh_locks: Option<TestRefreshLocks>,
}

impl FractionalVotingTestCase {
    fn create_lockups(
        &self,
        deps: &mut OwnedDeps<MockStorage, MockApi, MockQuerier, NeutronQuery>,
        env: &Env,
        lockups: Iter<TestLockup>,
        lock_epoch_length: u64,
    ) {
        for lockup in lockups {
            let info = get_message_info(
                &deps.api,
                self.voter_address,
                std::slice::from_ref(&lockup.token),
            );
            let msg = ExecuteMsg::LockTokens {
                lock_duration: lock_epoch_length,
                proof: None,
            };

            let res = execute(deps.as_mut(), env.clone(), info, msg);
            assert!(res.is_ok());
        }
    }

    fn verify_proposals_and_votes(
        &self,
        deps: &mut OwnedDeps<MockStorage, MockApi, MockQuerier, NeutronQuery>,
        round_id: u64,
        tranche_id: u64,
        voter: &Addr,
        expected_proposal_powers: Iter<TestProposalPower>,
        expected_user_votes: Iter<TestUserVote>,
    ) {
        for expected_proposal in expected_proposal_powers {
            let res = query_proposal(
                deps.as_ref(),
                round_id,
                tranche_id,
                expected_proposal.proposal_id,
            );
            assert_eq!(res.unwrap().proposal.power.u128(), expected_proposal.power);
        }

        let user_votes = query_user_votes(deps.as_ref(), round_id, tranche_id, voter.to_string())
            .unwrap()
            .votes;
        for expected_user_vote in expected_user_votes {
            let mut vote_found = false;
            for vote in user_votes.clone() {
                if vote.prop_id == expected_user_vote.proposal_id {
                    vote_found = true;
                    assert_eq!(vote.power.to_uint_ceil().u128(), expected_user_vote.power);
                }
            }

            assert!(vote_found, "expected user vote not found");
        }
    }
}

#[test]
fn fractional_voting_test() {
    let round_id_1 = 0;
    let tranche_id_1 = 1;

    let proposal_id_1 = 0;
    let proposal_id_2 = 1;
    let non_existing_proposal_id = 1000;

    let lockup_1_amount: u128 = 1000;
    let lockup_2_amount: u128 = 2000;
    let lockup_3_amount: u128 = 3000;
    let lockup_4_amount: u128 = 4000;
    let lockup_5_amount: u128 = 5000;

    // First lockup that gets created in test belongs to a different user,
    // therefore lockup IDs start from 1.
    let lockup_1 = TestLockup {
        lockup_id: 1,
        token: Coin::new(lockup_1_amount, IBC_DENOM_1.to_string()),
    };
    let lockup_2 = TestLockup {
        lockup_id: 2,
        token: Coin::new(lockup_2_amount, IBC_DENOM_2.to_string()),
    };
    let lockup_3 = TestLockup {
        lockup_id: 3,
        token: Coin::new(lockup_3_amount, IBC_DENOM_2.to_string()),
    };
    let lockup_4 = TestLockup {
        lockup_id: 4,
        token: Coin::new(lockup_4_amount, IBC_DENOM_1.to_string()),
    };
    let lockup_5 = TestLockup {
        lockup_id: 5,
        token: Coin::new(lockup_5_amount, IBC_DENOM_2.to_string()),
    };

    let test_cases = vec![
        FractionalVotingTestCase {
            description: "Use multiple lockups to vote for single proposal. Then lock more tokens and verify that the voting power increased. Then refresh some locks and verify if it affected the voting powers.",
            voter_address: "addr0000",
            lock_and_vote: TestLockAndVote {
                lockups_to_create: vec![
                    lockup_1.clone(),
                    lockup_2.clone(),
                    lockup_3.clone(),
                    lockup_4.clone(),
                ],
                votes: vec![ProposalToLockups {
                    proposal_id: proposal_id_1,
                    lock_ids: vec![lockup_1.lockup_id, lockup_2.lockup_id],
                }],
                expected_error: None,
                expected_proposal_powers: vec![TestProposalPower {
                    proposal_id: proposal_id_1,
                    power: lockup_1_amount + lockup_2_amount,
                }],
                expected_user_votes: vec![TestUserVote {
                    proposal_id: proposal_id_1,
                    power: lockup_1_amount + lockup_2_amount,
                }],
            },
            add_more_locks: Some(TestAddMoreLocks {
                more_lockups_to_create: vec![lockup_5.clone()],
                expected_proposal_powers: vec![TestProposalPower {
                    proposal_id: proposal_id_1,
                    power: lockup_1_amount + lockup_2_amount + lockup_5_amount,
                }],
                expected_user_votes: vec![TestUserVote {
                    proposal_id: proposal_id_1,
                    power: lockup_1_amount + lockup_2_amount + lockup_5_amount,
                }],
            }),
            refresh_locks: Some(TestRefreshLocks {
                lockups_to_refresh: vec![
                    // this one was used to vote => should increase proposal power
                    lockup_1.clone(),
                    // this one was NOT used to vote => should NOT increase proposal power
                    lockup_3.clone(),
                ],
                expected_proposal_powers: vec![TestProposalPower {
                    proposal_id: proposal_id_1,
                    // locks will be extended to 6 epochs, hence the 2x multiplier
                    power: 2 * lockup_1_amount + lockup_2_amount + lockup_5_amount,
                }],
                expected_user_votes: vec![TestUserVote {
                    proposal_id: proposal_id_1,
                    // locks will be extended to 6 epochs, hence the 2x multiplier
                    power: 2 * lockup_1_amount + lockup_2_amount + lockup_5_amount,
                }],
            }),
        },
        FractionalVotingTestCase {
            description: "Use multiple lockups to vote for two different proposals. Then lock more tokens and verify that the voting power on those proposals wasn't affected.",
            voter_address: "addr0000",
            lock_and_vote: TestLockAndVote {
                lockups_to_create: vec![
                    lockup_1.clone(),
                    lockup_2.clone(),
                    lockup_3.clone(),
                    lockup_4.clone(),
                ],
                votes: vec![
                    ProposalToLockups {
                        proposal_id: proposal_id_1,
                        lock_ids: vec![lockup_1.lockup_id, lockup_2.lockup_id],
                    },
                    ProposalToLockups {
                        proposal_id: proposal_id_2,
                        lock_ids: vec![lockup_3.lockup_id, lockup_4.lockup_id],
                    },
                ],
                expected_error: None,
                expected_proposal_powers: vec![
                    TestProposalPower {
                        proposal_id: proposal_id_1,
                        power: lockup_1_amount + lockup_2_amount,
                    },
                    TestProposalPower {
                        proposal_id: proposal_id_2,
                        power: lockup_3_amount + lockup_4_amount,
                    },
                ],
                expected_user_votes: vec![
                    TestUserVote {
                        proposal_id: proposal_id_1,
                        power: lockup_1_amount + lockup_2_amount,
                    },
                    TestUserVote {
                        proposal_id: proposal_id_2,
                        power: lockup_3_amount + lockup_4_amount,
                    },
                ],
            },
            add_more_locks: Some(TestAddMoreLocks {
                more_lockups_to_create: vec![lockup_5.clone()],
                expected_proposal_powers: vec![
                    TestProposalPower {
                        proposal_id: proposal_id_1,
                        power: lockup_1_amount + lockup_2_amount,
                    },
                    TestProposalPower {
                        proposal_id: proposal_id_2,
                        power: lockup_3_amount + lockup_4_amount,
                    },
                ],
                expected_user_votes: vec![
                    TestUserVote {
                        proposal_id: proposal_id_1,
                        power: lockup_1_amount + lockup_2_amount,
                    },
                    TestUserVote {
                        proposal_id: proposal_id_2,
                        power: lockup_3_amount + lockup_4_amount,
                    },
                ],
            }),
            refresh_locks: None,
        },
        FractionalVotingTestCase {
            description: "try to vote without providing any proposal and lock IDs",
            voter_address: "addr0000",
            lock_and_vote: TestLockAndVote {
                lockups_to_create: vec![],
                votes: vec![],
                expected_error: Some("Must provide at least one proposal and lockup to vote".to_string()),
                expected_proposal_powers: vec![],
                expected_user_votes: vec![],
            },
            add_more_locks: None,
            refresh_locks: None,
        },
        FractionalVotingTestCase {
            description: "try to vote by providing proposal ID but no lock IDs",
            voter_address: "addr0000",
            lock_and_vote: TestLockAndVote {
                lockups_to_create: vec![],
                votes: vec![ProposalToLockups {
                    proposal_id: proposal_id_1,
                    lock_ids: vec![],
                }],
                expected_error: Some(format!("No lock IDs provided to vote for proposal ID {proposal_id_1}").to_string()),
                expected_proposal_powers: vec![],
                expected_user_votes: vec![],
            },
            add_more_locks: None,
            refresh_locks: None,
        },
        FractionalVotingTestCase {
            description:
                "repeat proposal ID in Vote message",
            voter_address: "addr0000",
            lock_and_vote: TestLockAndVote {
                lockups_to_create: vec![lockup_1.clone(), lockup_2.clone()],
                votes: vec![
                    ProposalToLockups {
                        proposal_id: proposal_id_1,
                        lock_ids: vec![lockup_1.lockup_id, lockup_2.lockup_id],
                    },
                    ProposalToLockups {
                        proposal_id: proposal_id_1,
                        lock_ids: vec![lockup_3.lockup_id, lockup_4.lockup_id],
                    },
                ],
                expected_error: Some(format!(
                    "Duplicate proposal ID {proposal_id_1} provided"
                ).to_string()),
                expected_proposal_powers: vec![],
                expected_user_votes: vec![],
            },
            add_more_locks: None,
            refresh_locks: None,
        },
        FractionalVotingTestCase {
            description:
                "repeat lock IDs in Vote message",
            voter_address: "addr0000",
            lock_and_vote: TestLockAndVote {
                lockups_to_create: vec![
                    lockup_1.clone(),
                    lockup_2.clone(),
                    lockup_3.clone(),
                    lockup_4.clone(),
                ],
                votes: vec![
                    ProposalToLockups {
                        proposal_id: proposal_id_1,
                        lock_ids: vec![lockup_1.lockup_id, lockup_2.lockup_id],
                    },
                    ProposalToLockups {
                        proposal_id: proposal_id_2,
                        lock_ids: vec![
                            lockup_1.lockup_id,
                            lockup_2.lockup_id,
                            lockup_3.lockup_id,
                            lockup_3.lockup_id,
                            lockup_4.lockup_id,
                            lockup_4.lockup_id,
                        ],
                    },
                ],
                expected_error: Some(format!(
                    "Duplicate lock ID {} provided",
                    lockup_1.lockup_id
                ).to_string()),
                expected_proposal_powers: vec![],
                expected_user_votes: vec![],
            },
            add_more_locks: None,
            refresh_locks: None,
        },
        FractionalVotingTestCase {
            description: "try to vote for non existing proposal",
            voter_address: "addr0000",
            lock_and_vote: TestLockAndVote {
                lockups_to_create: vec![lockup_1.clone()],
                votes: vec![ProposalToLockups {
                    proposal_id: non_existing_proposal_id,
                    lock_ids: vec![lockup_1.lockup_id],
                }],
                expected_error: Some("not found".to_string()),
                expected_proposal_powers: vec![],
                expected_user_votes: vec![],
            },
            add_more_locks: None,
            refresh_locks: None,
        },
        FractionalVotingTestCase {
            description: "try to vote with non existing lock ID",
            voter_address: "addr0000",
            lock_and_vote: TestLockAndVote {
                lockups_to_create: vec![lockup_1.clone()],
                votes: vec![ProposalToLockups {
                    proposal_id: proposal_id_1,
                    lock_ids: vec![1000],
                }],
                expected_error: Some("not found".to_string()),
                expected_proposal_powers: vec![],
                expected_user_votes: vec![],
            },
            add_more_locks: None,
            refresh_locks: None,
        },
        FractionalVotingTestCase {
            description: "try to vote with other user's lock ID",
            voter_address: "addr0000",
            lock_and_vote: TestLockAndVote {
                lockups_to_create: vec![
                    lockup_1.clone(),
                    lockup_2.clone(),
                    lockup_3.clone(),
                    lockup_4.clone(),
                ],
                votes: vec![ProposalToLockups {
                    proposal_id: proposal_id_1,
                    lock_ids: vec![5],
                }],
                expected_error: Some("not found".to_string()),
                expected_proposal_powers: vec![],
                expected_user_votes: vec![],
            },
            add_more_locks: None,
            refresh_locks: None,
        },
    ];

    for test in test_cases {
        println!("running test case: {}", test.description);

        let grpc_query = denom_trace_grpc_query_mock(
            "transfer/channel-0".to_string(),
            HashMap::from([
                (IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string()),
                (IBC_DENOM_2.to_string(), VALIDATOR_2_LST_DENOM_1.to_string()),
            ]),
        );
        let (mut deps, env) = (mock_dependencies(grpc_query), mock_env());
        let info1 = get_message_info(&deps.api, test.voter_address, &[]);
        let instantiate_msg = get_default_instantiate_msg(&deps.api);
        let lock_epoch_length = instantiate_msg.lock_epoch_length;

        let res = instantiate(
            deps.as_mut(),
            env.clone(),
            info1.clone(),
            instantiate_msg.clone(),
        );
        assert!(res.is_ok());

        let result = set_validator_infos_for_round(
            &mut deps.storage,
            0,
            vec![VALIDATOR_1.to_string(), VALIDATOR_2.to_string()],
        );
        assert!(result.is_ok());

        let proposal_msgs = vec![
            ExecuteMsg::CreateProposal {
                round_id: None,
                tranche_id: tranche_id_1,
                title: "proposal title 1".to_string(),
                description: "proposal description 1".to_string(),
                minimum_atom_liquidity_request: Uint128::zero(),
                deployment_duration: 1,
            },
            ExecuteMsg::CreateProposal {
                round_id: None,
                tranche_id: tranche_id_1,
                title: "proposal title 2".to_string(),
                description: "proposal description 2".to_string(),
                minimum_atom_liquidity_request: Uint128::zero(),
                deployment_duration: 1,
            },
        ];

        for proposal_msg in proposal_msgs {
            let res = execute(deps.as_mut(), env.clone(), info1.clone(), proposal_msg);
            assert!(res.is_ok());
        }

        // create a lockup for different user so that it can be verified that the first user can't vote with that lockup
        let other_user_info = get_message_info(
            &deps.api,
            "addr0001",
            &[Coin::new(5000u128, IBC_DENOM_1.to_string())],
        );
        let msg = ExecuteMsg::LockTokens {
            lock_duration: lock_epoch_length,
            proof: None,
        };
        let res = execute(deps.as_mut(), env.clone(), other_user_info, msg);
        assert!(res.is_ok());

        test.create_lockups(
            &mut deps,
            &env,
            test.lock_and_vote.lockups_to_create.iter(),
            lock_epoch_length,
        );

        let vote_msg = ExecuteMsg::Vote {
            tranche_id: tranche_id_1,
            proposals_votes: test.lock_and_vote.votes.clone(),
        };
        let res = execute(deps.as_mut(), env.clone(), info1.clone(), vote_msg);

        if let Some(expected_error) = test.lock_and_vote.expected_error {
            assert!(res
                .unwrap_err()
                .to_string()
                .contains(expected_error.as_str()));
            continue;
        }

        assert!(res.is_ok());
        test.verify_proposals_and_votes(
            &mut deps,
            round_id_1,
            tranche_id_1,
            &info1.sender,
            test.lock_and_vote.expected_proposal_powers.iter(),
            test.lock_and_vote.expected_user_votes.iter(),
        );

        if let Some(add_more_locks) = test.add_more_locks.as_ref() {
            test.create_lockups(
                &mut deps,
                &env,
                add_more_locks.more_lockups_to_create.iter(),
                lock_epoch_length,
            );

            test.verify_proposals_and_votes(
                &mut deps,
                round_id_1,
                tranche_id_1,
                &info1.sender,
                add_more_locks.expected_proposal_powers.iter(),
                add_more_locks.expected_user_votes.iter(),
            );
        };

        if let Some(refresh_locks) = test.refresh_locks.as_ref() {
            let info = get_message_info(&deps.api, test.voter_address, &[]);
            let msg = ExecuteMsg::RefreshLockDuration {
                lock_ids: refresh_locks
                    .lockups_to_refresh
                    .iter()
                    .map(|lockup| lockup.lockup_id)
                    .collect(),
                lock_duration: 6 * lock_epoch_length,
            };

            let res = execute(deps.as_mut(), env.clone(), info, msg);
            assert!(res.is_ok());

            test.verify_proposals_and_votes(
                &mut deps,
                round_id_1,
                tranche_id_1,
                &info1.sender,
                refresh_locks.expected_proposal_powers.iter(),
                refresh_locks.expected_user_votes.iter(),
            );
        }
    }
}
