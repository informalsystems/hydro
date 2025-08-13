use std::collections::HashMap;

use crate::{
    contract::{
        calculate_voter_claim_amount, execute, instantiate, query_historical_tribute_claims,
        query_outstanding_lockup_claimable_coins, query_outstanding_tribute_claims,
        query_proposal_tributes, query_round_tributes,
    },
    msg::{ExecuteMsg, InstantiateMsg},
    query::TributeClaim,
    state::{
        Config, Tribute, CONFIG, ID_TO_TRIBUTE_MAP, TRIBUTE_CLAIMED_LOCKS, TRIBUTE_CLAIMS,
        TRIBUTE_MAP,
    },
};
use cosmwasm_std::{
    coin, coins, from_json,
    testing::{mock_dependencies, mock_env, MockApi},
    to_json_binary, Addr, Binary, ContractResult, Decimal, MessageInfo, QuerierResult, Response,
    StdError, StdResult, SystemError, SystemResult, Timestamp, Uint128, WasmQuery,
};
use cosmwasm_std::{BankMsg, Coin, CosmosMsg};
use hydro::{
    msg::LiquidityDeployment,
    query::{
        ConstantsResponse, CurrentRoundResponse, LiquidityDeploymentResponse, ProposalResponse,
        QueryMsg as HydroQueryMsg, UserVotedLocksResponse, UserVotesResponse, VotedLockInfo,
    },
    state::{Constants, Proposal, VoteWithPower},
};

pub fn get_instantiate_msg(hydro_contract: String) -> InstantiateMsg {
    InstantiateMsg { hydro_contract }
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

fn get_nonzero_deployment_for_proposal(proposal: Proposal) -> LiquidityDeployment {
    LiquidityDeployment {
        round_id: proposal.round_id,
        tranche_id: proposal.tranche_id,
        proposal_id: proposal.proposal_id,
        destinations: vec![],
        deployed_funds: coins(100, "utest"),
        funds_before_deployment: vec![],
        total_rounds: 0,
        remaining_rounds: 0,
    }
}

fn get_zero_deployment_for_proposal(proposal: Proposal) -> LiquidityDeployment {
    LiquidityDeployment {
        round_id: proposal.round_id,
        tranche_id: proposal.tranche_id,
        proposal_id: proposal.proposal_id,
        destinations: vec![],
        deployed_funds: vec![],
        funds_before_deployment: vec![],
        total_rounds: 0,
        remaining_rounds: 0,
    }
}

const DEFAULT_DENOM: &str = "uatom";
const HYDRO_CONTRACT_ADDRESS: &str = "addr0000";
const USER_ADDRESS_1: &str = "addr0001";
const USER_ADDRESS_2: &str = "addr0002";
const MIN_PROP_PERCENT_FOR_CLAIMABLE_TRIBUTES: Uint128 = Uint128::new(5);

pub struct MockWasmQuerier {
    hydro_contract: String,
    current_round: u64,
    proposals: Vec<Proposal>,
    user_votes: Vec<UserVote>,
    liquidity_deployments: Vec<LiquidityDeployment>,
    hydro_constants: Option<Constants>,
}

impl MockWasmQuerier {
    pub fn new(
        hydro_contract: String,
        current_round: u64,
        proposals: Vec<Proposal>,
        user_votes: Vec<UserVote>,
        liquidity_deployments: Vec<LiquidityDeployment>,
        hydro_constants: Option<Constants>,
    ) -> Self {
        Self {
            hydro_contract,
            current_round,
            proposals,
            user_votes,
            liquidity_deployments,
            hydro_constants,
        }
    }

    pub fn handler(&self, query: &WasmQuery) -> QuerierResult {
        match query {
            WasmQuery::Smart { contract_addr, msg } => {
                if *contract_addr != self.hydro_contract {
                    return SystemResult::Err(SystemError::NoSuchContract {
                        addr: contract_addr.to_string(),
                    });
                }

                let response = match from_json(msg).unwrap() {
                    HydroQueryMsg::CurrentRound {} => to_json_binary(&CurrentRoundResponse {
                        round_id: self.current_round,
                        // use an arbitrary timestamp here
                        round_end: Timestamp::from_seconds(1),
                    }),
                    HydroQueryMsg::Proposal {
                        round_id,
                        tranche_id,
                        proposal_id,
                    } => Ok({
                        let res = self.find_matching_proposal(round_id, tranche_id, proposal_id);

                        match res {
                            Ok(res) => res,
                            Err(_) => {
                                return SystemResult::Err(SystemError::InvalidRequest {
                                    error: format!("proposal couldn't be found: round_id={round_id}, tranche_id={tranche_id}, proposal_id={proposal_id}"),
                                    request: Binary::new(vec![]),
                                })
                            }
                        }
                    }),
                    HydroQueryMsg::UserVotes {
                        round_id,
                        tranche_id,
                        address,
                    } => Ok({
                        let res =
                            self.find_matching_user_votes(round_id, tranche_id, address.as_str());

                        match res {
                            Ok(res) => res,
                            Err(_) => {
                                return SystemResult::Err(SystemError::InvalidRequest {
                                    error: "vote couldn't be found".to_string(),
                                    request: Binary::new(vec![]),
                                })
                            }
                        }
                    }),
                    HydroQueryMsg::LiquidityDeployment {
                        round_id,
                        tranche_id,
                        proposal_id,
                    } => Ok({
                        let res = self.find_matching_liquidity_deployment(
                            round_id,
                            tranche_id,
                            proposal_id,
                        );

                        match res {
                                Ok(res) => res,
                                Err(_) => {
                                    return SystemResult::Err(SystemError::InvalidRequest {
                                        error: format!("liquidity deployment couldn't be found: round_id={round_id}, tranche_id={tranche_id}, proposal_id={proposal_id}"),
                                        request: Binary::new(vec![]),
                                    })
                                }
                            }
                    }),
                    HydroQueryMsg::Constants {} => to_json_binary(&ConstantsResponse {
                        constants: self.hydro_constants.clone().unwrap(),
                    }),
                    HydroQueryMsg::UserVotedLocks {
                        user_address,
                        round_id,
                        tranche_id,
                        proposal_id,
                    } => Ok({
                        let res = self.find_matching_user_voted_locks(
                            round_id,
                            tranche_id,
                            user_address.as_str(),
                            proposal_id,
                        );

                        match res {
                            Ok(res) => res,
                            Err(_) => {
                                return SystemResult::Err(SystemError::InvalidRequest {
                                    error: "vote couldn't be found".to_string(),
                                    request: Binary::new(vec![]),
                                })
                            }
                        }
                    }),
                    HydroQueryMsg::LockVotesHistory {
                        lock_id,
                        start_from_round_id: _,
                        stop_at_round_id: _,
                        tranche_id: _,
                    } => Ok({
                        let res = self.find_lock_votes_history(lock_id);

                        match res {
                            Ok(res) => res,
                            Err(_) => {
                                return SystemResult::Err(SystemError::InvalidRequest {
                                    error: format!(
                                        "lock votes history couldn't be found for lock_id={lock_id}"
                                    ),
                                    request: Binary::new(vec![]),
                                })
                            }
                        }
                    }),

                    _ => panic!("unsupported query"),
                };

                SystemResult::Ok(ContractResult::Ok(response.unwrap()))
            }
            _ => SystemResult::Err(SystemError::UnsupportedRequest {
                kind: "unsupported query type".to_string(),
            }),
        }
    }

    fn find_matching_user_votes(
        &self,
        round_id: u64,
        tranche_id: u64,
        address: &str,
    ) -> StdResult<Binary> {
        let mut votes = vec![];
        for (vote_round_id, vote_tranche_id, vote_address, vote, _) in &self.user_votes {
            if *vote_round_id == round_id
                && *vote_tranche_id == tranche_id
                && vote_address == address
            {
                votes.push(vote.clone());
            }
        }

        if votes.is_empty() {
            return StdResult::Err(StdError::generic_err("vote couldn't be found"));
        }

        to_json_binary(&UserVotesResponse { votes })
    }

    fn find_matching_user_voted_locks(
        &self,
        round_id: u64,
        tranche_id: u64,
        user_address: &str,
        proposal_id: Option<u64>,
    ) -> StdResult<Binary> {
        let mut locks_by_prop_id: HashMap<u64, Vec<VotedLockInfo>> = HashMap::new();
        for (vote_round_id, vote_tranche_id, vote_address, vote, lock_id) in &self.user_votes {
            if *vote_round_id == round_id
                && *vote_tranche_id == tranche_id
                && vote_address == user_address
                && (proposal_id.is_none() || Some(vote.prop_id) == proposal_id)
            {
                let votes = locks_by_prop_id.entry(vote.prop_id).or_default();
                votes.push(VotedLockInfo {
                    lock_id: *lock_id,
                    vote_power: vote.power,
                });
            }
        }
        to_json_binary(&UserVotedLocksResponse {
            voted_locks: locks_by_prop_id.into_iter().collect(),
        })
    }

    fn find_matching_proposal(
        &self,
        round_id: u64,
        tranche_id: u64,
        proposal_id: u64,
    ) -> StdResult<Binary> {
        for prop in &self.proposals {
            if prop.round_id == round_id
                && prop.tranche_id == tranche_id
                && prop.proposal_id == proposal_id
            {
                let res: StdResult<Binary> = to_json_binary(&ProposalResponse {
                    proposal: prop.clone(),
                });
                return res;
            }
        }
        StdResult::Err(StdError::generic_err("proposal couldn't be found"))
    }

    fn find_matching_liquidity_deployment(
        &self,
        round_id: u64,
        tranche_id: u64,
        proposal_id: u64,
    ) -> StdResult<Binary> {
        for deployment in &self.liquidity_deployments {
            if deployment.round_id == round_id
                && deployment.tranche_id == tranche_id
                && deployment.proposal_id == proposal_id
            {
                return to_json_binary(&LiquidityDeploymentResponse {
                    liquidity_deployment: deployment.clone(),
                });
            }
        }

        Err(StdError::generic_err(
            "liquidity deployment couldn't be found",
        ))
    }

    fn find_lock_votes_history(&self, lock_id: u64) -> StdResult<Binary> {
        use hydro::query::{LockVotesHistoryEntry, LockVotesHistoryResponse};

        let mut vote_history = vec![];

        // Look through user_votes to find votes for this specific lock_id
        for (round_id, tranche_id, _address, vote, vote_lock_id) in &self.user_votes {
            if *vote_lock_id == lock_id {
                vote_history.push(LockVotesHistoryEntry {
                    round_id: *round_id,
                    tranche_id: *tranche_id,
                    proposal_id: vote.prop_id,
                    vote_power: vote.power,
                });
            }
        }

        to_json_binary(&LockVotesHistoryResponse { vote_history })
    }
}

struct AddTributeTestCase {
    description: String,
    tributes_to_add: Vec<Vec<Coin>>,
    // (current_round_id, proposal_to_tribute)
    mock_data: (u64, Vec<Proposal>),
    expected_success: bool,
    expected_error_msg: String,
}

struct ClaimTributeTestCase {
    description: String,
    tributes_to_add: Vec<AddTributeInfo>,
    tributes_to_claim: Vec<ClaimTributeInfo>,
    mock_data: ClaimTributeMockData,
}

// to make clippy happy :)
// (add_tribute_round_id, claim_tribute_round_id, proposals, user_votes, liquidity_deployments)
type ClaimTributeMockData = (
    u64,
    u64,
    Vec<Proposal>,
    Vec<UserVote>,
    Vec<LiquidityDeployment>,
);

struct AddTributeInfo {
    round_id: u64,
    tranche_id: u64,
    proposal_id: u64,
    token: Coin,
}

struct ClaimTributeInfo {
    round_id: u64,
    tranche_id: u64,
    tribute_id: u64,
    expected_success: bool,
    expected_error_msg: String,
    expected_tribute_claim: u128,
}

type UserVote = (u64, u64, String, VoteWithPower, u64); // (round_id, tranche_id, address, VoteWithPower, lock_id)

struct RefundTributeTestCase {
    description: String,
    // (round_id, tranche_id, proposal_id, tribute_id)
    tribute_info: (u64, u64, u64, u64),
    tribute_to_add: Vec<Coin>,
    // (add_tribute_round_id, refund_tribute_round_id, proposals, liquidity_deployments)
    mock_data: (u64, u64, Vec<Proposal>, Vec<LiquidityDeployment>),
    tribute_refunder: Option<String>,
    expected_tribute_refund: u128,
    expected_success: bool,
    expected_error_msg: String,
}

#[test]
fn add_tribute_test() {
    let mock_proposal = Proposal {
        round_id: 10,
        tranche_id: 0,
        proposal_id: 5,
        title: "proposal title 1".to_string(),
        description: "proposal description 1".to_string(),
        power: Uint128::new(10000),
        percentage: Uint128::zero(),
        minimum_atom_liquidity_request: Uint128::zero(),
        deployment_duration: 1,
    };

    let test_cases: Vec<AddTributeTestCase> = vec![
        AddTributeTestCase {
            description: "happy path".to_string(),
            tributes_to_add: vec![
                vec![Coin::new(1000u64, DEFAULT_DENOM)],
                vec![Coin::new(5000u64, DEFAULT_DENOM)],
            ],
            mock_data: (10, vec![mock_proposal.clone()]),
            expected_success: true,
            expected_error_msg: String::new(),
        },
        AddTributeTestCase {
            description: "try adding tribute for non-existing proposal".to_string(),
            tributes_to_add: vec![vec![Coin::new(1000u64, DEFAULT_DENOM)]],
            mock_data: (10, vec![]),
            expected_success: false,
            expected_error_msg: "proposal couldn't be found".to_string(),
        },
        AddTributeTestCase {
            description: "try adding tribute without providing any funds".to_string(),
            tributes_to_add: vec![vec![]],
            mock_data: (10, vec![mock_proposal.clone()]),
            expected_success: false,
            expected_error_msg: "Must send funds to add tribute".to_string(),
        },
        AddTributeTestCase {
            description: "try adding tribute by providing more than one token".to_string(),
            tributes_to_add: vec![vec![
                Coin::new(1000u64, DEFAULT_DENOM),
                Coin::new(1000u64, "stake"),
            ]],
            mock_data: (10, vec![mock_proposal.clone()]),
            expected_success: false,
            expected_error_msg: "Must send exactly one coin".to_string(),
        },
        AddTributeTestCase {
            description: "add tribute to previous round".to_string(),
            tributes_to_add: vec![vec![Coin::new(1000u64, DEFAULT_DENOM)]],
            // proposal is in round 10, but we are trying to add tribute during round 11
            mock_data: (11, vec![mock_proposal.clone()]),
            expected_success: true,
            expected_error_msg: String::new(),
        },
    ];

    for test in test_cases {
        println!("running test case: {}", test.description);

        let (mut deps, env) = (mock_dependencies(), mock_env());
        let info = get_message_info(&deps.api, USER_ADDRESS_1, &[]);

        let hydro_contract_address = get_address_as_str(&deps.api, HYDRO_CONTRACT_ADDRESS);
        let mock_querier = MockWasmQuerier::new(
            hydro_contract_address.clone(),
            test.mock_data.0,
            test.mock_data.1,
            vec![],
            vec![],
            None,
        );
        deps.querier.update_wasm(move |q| mock_querier.handler(q));

        let msg = get_instantiate_msg(hydro_contract_address);
        let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
        assert!(res.is_ok());

        let tribute_payer = USER_ADDRESS_1;
        let tribute_payer_addr = get_address_as_str(&deps.api, tribute_payer);

        for tribute in &test.tributes_to_add {
            let info = get_message_info(&deps.api, tribute_payer, tribute);
            let msg = ExecuteMsg::AddTribute {
                tranche_id: mock_proposal.tranche_id,
                round_id: mock_proposal.round_id,
                proposal_id: mock_proposal.proposal_id,
            };

            let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
            if test.expected_success {
                assert!(res.is_ok(), "failed with: {}", res.unwrap_err());
            } else {
                assert!(res
                    .unwrap_err()
                    .to_string()
                    .contains(&test.expected_error_msg))
            }
        }

        // If ExecuteMsg::AddTribute was supposed to fail, then there will be no tributes added
        if !test.expected_success {
            continue;
        }

        let res = query_proposal_tributes(
            deps.as_ref(),
            mock_proposal.round_id,
            mock_proposal.proposal_id,
            0,
            3000,
        )
        .unwrap()
        .tributes;
        assert_eq!(test.tributes_to_add.len(), res.len());

        for (i, tribute) in test.tributes_to_add.iter().enumerate() {
            assert_eq!(res[i].funds, tribute[0].clone());
            assert_eq!(res[i].depositor.to_string(), tribute_payer_addr.clone());
            assert!(!res[i].refunded);
            assert_eq!(res[i].creation_time, env.block.time);
        }
    }
}

#[test]
fn claim_tribute_test() {
    let mock_proposal1 = Proposal {
        round_id: 10,
        tranche_id: 0,
        proposal_id: 5,
        title: "proposal title 1".to_string(),
        description: "proposal description 1".to_string(),
        power: Uint128::new(10000),
        percentage: MIN_PROP_PERCENT_FOR_CLAIMABLE_TRIBUTES,
        minimum_atom_liquidity_request: Uint128::zero(),
        deployment_duration: 1,
    };
    let mock_proposal2 = Proposal {
        round_id: 10,
        tranche_id: 0,
        proposal_id: 6,
        title: "proposal title 2".to_string(),
        description: "proposal description 2".to_string(),
        power: Uint128::new(10000),
        percentage: MIN_PROP_PERCENT_FOR_CLAIMABLE_TRIBUTES,
        minimum_atom_liquidity_request: Uint128::zero(),
        deployment_duration: 1,
    };
    let mock_proposal3 = Proposal {
        round_id: 10,
        tranche_id: 0,
        proposal_id: 7,
        title: "proposal title 3".to_string(),
        description: "proposal description 3".to_string(),
        power: Uint128::new(10000),
        percentage: MIN_PROP_PERCENT_FOR_CLAIMABLE_TRIBUTES,
        minimum_atom_liquidity_request: Uint128::zero(),
        deployment_duration: 1,
    };

    let mock_proposals = vec![
        mock_proposal1.clone(),
        mock_proposal2.clone(),
        mock_proposal3.clone(),
    ];

    // a default liquidity deployments vector, to have a valid deployment
    // for each mock proposal
    let deployments_for_all_proposals = mock_proposals
        .iter()
        .map(|p| get_nonzero_deployment_for_proposal(p.clone()))
        .collect::<Vec<LiquidityDeployment>>();

    let zero_deployments_for_all_proposals = mock_proposals
        .iter()
        .map(|p| get_zero_deployment_for_proposal(p.clone()))
        .collect::<Vec<LiquidityDeployment>>();

    let deps = mock_dependencies();
    let test_cases: Vec<ClaimTributeTestCase> = vec![
        ClaimTributeTestCase {
            description: "happy path: claim tributes for multiple proposals that user voted on"
                .to_string(),
            tributes_to_add: vec![
                AddTributeInfo {
                    round_id: 10,
                    tranche_id: 0,
                    proposal_id: 5,
                    token: Coin::new(1000u64, DEFAULT_DENOM),
                },
                AddTributeInfo {
                    round_id: 10,
                    tranche_id: 0,
                    proposal_id: 6,
                    token: Coin::new(2000u64, DEFAULT_DENOM),
                },
            ],
            tributes_to_claim: vec![
                ClaimTributeInfo {
                    round_id: 10,
                    tranche_id: 0,
                    tribute_id: 0,
                    expected_success: true,
                    expected_tribute_claim: 7, // (70 / 10_000) * 1_000
                    expected_error_msg: String::new(),
                },
                ClaimTributeInfo {
                    round_id: 10,
                    tranche_id: 0,
                    tribute_id: 1,
                    expected_success: true,
                    expected_tribute_claim: 14, // (70 / 10_000) * 2_000
                    expected_error_msg: String::new(),
                },
            ],
            mock_data: (
                10,
                11,
                mock_proposals.clone(),
                vec![
                    (
                        10,
                        0,
                        get_address_as_str(&deps.api, USER_ADDRESS_2),
                        VoteWithPower {
                            prop_id: 5,
                            power: Decimal::from_ratio(Uint128::new(70), Uint128::one()),
                        },
                        0,
                    ),
                    (
                        10,
                        0,
                        get_address_as_str(&deps.api, USER_ADDRESS_2),
                        VoteWithPower {
                            prop_id: 6,
                            power: Decimal::from_ratio(Uint128::new(70), Uint128::one()),
                        },
                        1,
                    ),
                ],
                deployments_for_all_proposals.clone(),
            ),
        },
        ClaimTributeTestCase {
            description: "try claim tribute for proposal in current round".to_string(),
            tributes_to_add: vec![AddTributeInfo {
                round_id: 10,
                tranche_id: 0,
                proposal_id: 5,
                token: Coin::new(1000u64, DEFAULT_DENOM),
            }],
            tributes_to_claim: vec![ClaimTributeInfo {
                round_id: 10,
                tranche_id: 0,
                tribute_id: 0,
                expected_success: false,
                expected_tribute_claim: 0,
                expected_error_msg: "Round has not ended yet".to_string(),
            }],
            mock_data: (
                10,
                10,
                mock_proposals.clone(),
                vec![],
                deployments_for_all_proposals.clone(),
            ),
        },
        ClaimTributeTestCase {
            description: "try claim tribute if user didn't vote at all".to_string(),
            tributes_to_add: vec![AddTributeInfo {
                round_id: 10,
                tranche_id: 0,
                proposal_id: 5,
                token: Coin::new(1000u64, DEFAULT_DENOM),
            }],
            tributes_to_claim: vec![ClaimTributeInfo {
                round_id: 10,
                tranche_id: 0,
                tribute_id: 0,
                expected_success: false,
                expected_tribute_claim: 0,
                expected_error_msg:
                    "Nothing to claim - all locks have already claimed this tribute".to_string(),
            }],
            mock_data: (
                10,
                11,
                mock_proposals.clone(),
                vec![],
                deployments_for_all_proposals.clone(),
            ),
        },
        ClaimTributeTestCase {
            description: "try claim tribute for non existing tribute id".to_string(),
            tributes_to_add: vec![AddTributeInfo {
                round_id: 10,
                tranche_id: 0,
                proposal_id: 5,
                token: Coin::new(1000u64, DEFAULT_DENOM),
            }],
            tributes_to_claim: vec![ClaimTributeInfo {
                round_id: 10,
                tranche_id: 0,
                tribute_id: 1,
                expected_success: false,
                expected_tribute_claim: 0,
                expected_error_msg: "not found".to_string(),
            }],
            mock_data: (
                10,
                11,
                mock_proposals.clone(),
                vec![(
                    10,
                    0,
                    get_address_as_str(&deps.api, USER_ADDRESS_2),
                    VoteWithPower {
                        prop_id: 5,
                        power: Decimal::from_ratio(Uint128::new(70), Uint128::one()),
                    },
                    0,
                )],
                deployments_for_all_proposals.clone(),
            ),
        },
        ClaimTributeTestCase {
            description:
                "try to claim tribute that belongs to different proposal than the one user voted on"
                    .to_string(),
            tributes_to_add: vec![AddTributeInfo {
                round_id: 10,
                tranche_id: 0,
                proposal_id: 5,
                token: Coin::new(1000u64, DEFAULT_DENOM),
            }],
            tributes_to_claim: vec![ClaimTributeInfo {
                round_id: 10,
                tranche_id: 0,
                tribute_id: 0,
                expected_success: false,
                expected_tribute_claim: 0,
                expected_error_msg:
                    "Nothing to claim - all locks have already claimed this tribute".to_string(),
            }],
            mock_data: (
                10,
                11,
                mock_proposals.clone(),
                vec![(
                    10,
                    0,
                    get_address_as_str(&deps.api, USER_ADDRESS_2),
                    VoteWithPower {
                        prop_id: 6,
                        power: Decimal::from_ratio(Uint128::new(70), Uint128::one()),
                    },
                    0,
                )],
                deployments_for_all_proposals.clone(),
            ),
        },
        ClaimTributeTestCase {
            description: "try claim tribute for proposal with no deployment entered".to_string(),
            tributes_to_add: vec![AddTributeInfo {
                round_id: 10,
                tranche_id: 0,
                proposal_id: 5,
                token: Coin::new(1000u64, DEFAULT_DENOM),
            }],
            tributes_to_claim: vec![ClaimTributeInfo {
                round_id: 10,
                tranche_id: 0,
                tribute_id: 0,
                expected_success: false,
                expected_tribute_claim: 0,
                expected_error_msg: "Proposal did not have a liquidity deployment entered"
                    .to_string(),
            }],
            mock_data: (
                10,
                11,
                mock_proposals.clone(),
                vec![(
                    10,
                    0,
                    get_address_as_str(&deps.api, USER_ADDRESS_2),
                    VoteWithPower {
                        prop_id: 5,
                        power: Decimal::from_ratio(Uint128::new(70), Uint128::one()),
                    },
                    0,
                )],
                vec![],
            ),
        },
        ClaimTributeTestCase {
            description: "try claim tribute for proposal with zero deployment".to_string(),
            tributes_to_add: vec![AddTributeInfo {
                round_id: 10,
                tranche_id: 0,
                proposal_id: 5,
                token: Coin::new(1000u64, DEFAULT_DENOM),
            }],
            tributes_to_claim: vec![ClaimTributeInfo {
                round_id: 10,
                tranche_id: 0,
                tribute_id: 0,
                expected_success: false,
                expected_tribute_claim: 0,
                expected_error_msg: "Proposal did not receive a non-zero liquidity deployment"
                    .to_string(),
            }],
            mock_data: (
                10,
                11,
                mock_proposals.clone(),
                vec![(
                    10,
                    0,
                    get_address_as_str(&deps.api, USER_ADDRESS_2),
                    VoteWithPower {
                        prop_id: 5,
                        power: Decimal::from_ratio(Uint128::new(70), Uint128::one()),
                    },
                    0,
                )],
                zero_deployments_for_all_proposals,
            ),
        },
    ];

    for test in test_cases {
        println!("running test case: {}", test.description);

        let (mut deps, env) = (mock_dependencies(), mock_env());
        let info = get_message_info(&deps.api, USER_ADDRESS_1, &[]);

        let hydro_contract_address = get_address_as_str(&deps.api, HYDRO_CONTRACT_ADDRESS);
        let mock_querier = MockWasmQuerier::new(
            hydro_contract_address.clone(),
            test.mock_data.0,
            test.mock_data.2.clone(),
            vec![],
            vec![],
            None,
        );
        deps.querier.update_wasm(move |q| mock_querier.handler(q));

        let msg = get_instantiate_msg(hydro_contract_address.clone());
        let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
        assert!(res.is_ok());

        let tribute_payer = USER_ADDRESS_1;
        for tribute_to_add in test.tributes_to_add.iter() {
            let info = get_message_info(&deps.api, tribute_payer, &[tribute_to_add.token.clone()]);
            let msg = ExecuteMsg::AddTribute {
                tranche_id: tribute_to_add.tranche_id,
                round_id: tribute_to_add.round_id,
                proposal_id: tribute_to_add.proposal_id,
            };

            let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
            assert!(res.is_ok());
        }

        // Update the expected round so that the tribute can be claimed
        let mock_querier = MockWasmQuerier::new(
            hydro_contract_address.clone(),
            test.mock_data.1,
            test.mock_data.2.clone(),
            test.mock_data.3.clone(),
            test.mock_data.4.clone(),
            None,
        );
        deps.querier.update_wasm(move |q| mock_querier.handler(q));

        for tribute_to_claim in test.tributes_to_claim.iter() {
            let tribute_claimer = get_address_as_str(&deps.api, USER_ADDRESS_2);
            let info = get_message_info(&deps.api, USER_ADDRESS_1, &[]);
            let msg = ExecuteMsg::ClaimTribute {
                round_id: tribute_to_claim.round_id,
                tranche_id: tribute_to_claim.tranche_id,
                tribute_id: tribute_to_claim.tribute_id,
                voter_address: tribute_claimer.clone(),
            };
            let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone());

            if !tribute_to_claim.expected_success {
                let error_msg = res.unwrap_err().to_string();
                assert!(
                    error_msg.contains(&tribute_to_claim.expected_error_msg),
                    "expected: {}, got: {}",
                    tribute_to_claim.expected_error_msg,
                    error_msg
                );
                continue;
            }

            assert!(res.is_ok());
            let res = res.unwrap();
            assert_eq!(1, res.messages.len());

            verify_tokens_received(
                res,
                &tribute_claimer.clone(),
                &DEFAULT_DENOM.to_string(),
                tribute_to_claim.expected_tribute_claim,
            );

            // Verify that the same tribute can't be claimed twice for the same user
            let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone());
            assert!(res.unwrap_err().to_string().contains("Nothing to claim"));
        }
    }
}

#[test]
fn refund_tribute_test() {
    let mock_proposal = Proposal {
        round_id: 10,
        tranche_id: 0,
        proposal_id: 5,
        title: "proposal title 1".to_string(),
        description: "proposal description 1".to_string(),
        power: Uint128::new(10000),
        percentage: Uint128::zero(),
        minimum_atom_liquidity_request: Uint128::zero(),
        deployment_duration: 1,
    };

    let mock_proposals = vec![mock_proposal.clone()];

    let liquidity_deployments_refundable =
        vec![get_zero_deployment_for_proposal(mock_proposal.clone())];

    let liquidity_deployments_non_refundable =
        vec![get_nonzero_deployment_for_proposal(mock_proposal.clone())];

    let test_cases: Vec<RefundTributeTestCase> = vec![
        RefundTributeTestCase {
            description: "happy path: refund tribute for deployment with zero-deployment"
                .to_string(),
            tribute_info: (10, 0, 5, 0),
            tribute_to_add: vec![Coin::new(1000u64, DEFAULT_DENOM)],
            mock_data: (
                10,
                11,
                mock_proposals.clone(),
                liquidity_deployments_refundable.clone(),
            ),
            tribute_refunder: None,
            expected_tribute_refund: 1000,
            expected_success: true,
            expected_error_msg: String::new(),
        },
        RefundTributeTestCase {
            description: "try to get refund for the current round".to_string(),
            tribute_info: (10, 0, 5, 0),
            tribute_to_add: vec![Coin::new(1000u64, DEFAULT_DENOM)],
            mock_data: (
                10,
                10,
                mock_proposals.clone(),
                liquidity_deployments_refundable.clone(),
            ),
            tribute_refunder: None,
            expected_tribute_refund: 0,
            expected_success: false,
            expected_error_msg: "Round has not ended yet".to_string(),
        },
        RefundTributeTestCase {
            description: "try to get refund for non existing tribute".to_string(),
            tribute_info: (10, 0, 5, 1),
            tribute_to_add: vec![Coin::new(1000u64, DEFAULT_DENOM)],
            mock_data: (
                10,
                11,
                mock_proposals.clone(),
                liquidity_deployments_refundable.clone(),
            ),
            tribute_refunder: None,
            expected_tribute_refund: 0,
            expected_success: false,
            expected_error_msg: "not found".to_string(),
        },
        RefundTributeTestCase {
            description: "try to get refund for tribute with no deployment entered".to_string(),
            tribute_info: (10, 0, 5, 0),
            tribute_to_add: vec![Coin::new(1000u64, DEFAULT_DENOM)],
            mock_data: (10, 11, mock_proposals.clone(), vec![]),
            tribute_refunder: None,
            expected_tribute_refund: 1000,
            expected_success: false,
            expected_error_msg:
                "Can't refund tribute for proposal that didn't have a liquidity deployment entered"
                    .to_string(),
        },
        RefundTributeTestCase {
            description: "try to get refund for tribute with non-zero fund deployment".to_string(),
            tribute_info: (10, 0, 5, 0),
            tribute_to_add: vec![Coin::new(1000u64, DEFAULT_DENOM)],
            mock_data: (
                10,
                11,
                mock_proposals.clone(),
                liquidity_deployments_non_refundable.clone(),
            ),
            tribute_refunder: None,
            expected_tribute_refund: 1000,
            expected_success: false,
            expected_error_msg:
                "Can't refund tribute for proposal that received a non-zero liquidity deployment"
                    .to_string(),
        },
        RefundTributeTestCase {
            description: "try to get refund if not the depositor".to_string(),
            tribute_info: (10, 0, 5, 0),
            tribute_to_add: vec![Coin::new(1000u64, DEFAULT_DENOM)],
            mock_data: (
                10,
                11,
                mock_proposals.clone(),
                liquidity_deployments_refundable.clone(),
            ),
            tribute_refunder: Some(USER_ADDRESS_2.to_string()),
            expected_tribute_refund: 0,
            expected_success: false,
            expected_error_msg: "Sender is not the depositor of the tribute".to_string(),
        },
    ];

    for test in test_cases {
        println!("running test case: {}", test.description);

        let (mut deps, env) = (mock_dependencies(), mock_env());
        let info = get_message_info(&deps.api, USER_ADDRESS_1, &[]);

        let hydro_contract_address = get_address_as_str(&deps.api, HYDRO_CONTRACT_ADDRESS);
        let mock_querier = MockWasmQuerier::new(
            hydro_contract_address.clone(),
            test.mock_data.0,
            test.mock_data.2.clone(),
            vec![],
            vec![],
            None,
        );
        deps.querier.update_wasm(move |q| mock_querier.handler(q));

        let msg = get_instantiate_msg(hydro_contract_address.clone());
        let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
        assert!(res.is_ok());

        let tribute_payer = USER_ADDRESS_1;
        let info = get_message_info(&deps.api, tribute_payer, &test.tribute_to_add);
        let msg = ExecuteMsg::AddTribute {
            tranche_id: test.tribute_info.1,
            round_id: test.tribute_info.0,
            proposal_id: test.tribute_info.2,
        };

        let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
        assert!(res.is_ok());

        // Update the expected round so that the tribute can be refunded
        let mock_querier = MockWasmQuerier::new(
            hydro_contract_address.clone(),
            test.mock_data.1,
            test.mock_data.2.clone(),
            vec![],
            test.mock_data.3.clone(),
            None,
        );
        deps.querier.update_wasm(move |q| mock_querier.handler(q));

        // If specified, try to use a different tribute refunder
        let tribute_refunder = match test.tribute_refunder {
            Some(refunder) => refunder,
            None => tribute_payer.to_string(),
        };

        let info = get_message_info(&deps.api, &tribute_refunder, &[]);
        let msg = ExecuteMsg::RefundTribute {
            round_id: test.tribute_info.0,
            tranche_id: test.tribute_info.1,
            proposal_id: test.tribute_info.2,
            tribute_id: test.tribute_info.3,
        };
        let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone());

        if !test.expected_success {
            let error_msg = res.unwrap_err().to_string();
            assert!(
                error_msg.contains(&test.expected_error_msg),
                "expected error message: {}, got: {}",
                test.expected_error_msg,
                error_msg
            );
            continue;
        }

        assert!(res.is_ok());
        let res = res.unwrap();
        assert_eq!(1, res.messages.len());

        verify_tokens_received(
            res,
            &info.sender.to_string(),
            &test.tribute_to_add[0].denom,
            test.expected_tribute_refund,
        );

        // Verify that the user can't refund the same tribute twice
        let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone());
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("Sender has already refunded the tribute"))
    }
}

fn verify_tokens_received(
    res: Response,
    expected_receiver: &String,
    expected_denom: &String,
    expected_amount: u128,
) {
    match &res.messages[0].msg {
        CosmosMsg::Bank(bank_msg) => match bank_msg {
            BankMsg::Send { to_address, amount } => {
                assert_eq!(*expected_receiver, *to_address);
                assert_eq!(1, amount.len());
                assert_eq!(*expected_denom, amount[0].denom);
                assert_eq!(expected_amount, amount[0].amount.u128());
            }
            _ => panic!("expected BankMsg::Send message"),
        },
        _ => panic!("expected CosmosMsg::Bank msg"),
    };
}

struct HistoricalTributeClaimsTestCase {
    description: String,
    user_address: Addr,
    start_from: u32,
    limit: u32,
    expected_claims: Vec<TributeClaim>,
    expected_error: Option<StdError>,
}

#[test]
fn test_query_historical_tribute_claims() {
    let deps = mock_dependencies();

    let test_cases = vec![
        HistoricalTributeClaimsTestCase {
            description: "User with claimed tributes".to_string(),
            user_address: deps.api.addr_make("user1"),
            start_from: 0,
            limit: 10,
            expected_claims: vec![
                TributeClaim {
                    round_id: 1,
                    tranche_id: 1,
                    proposal_id: 1,
                    tribute_id: 0,
                    amount: Coin::new(Uint128::new(100), "token"),
                },
                TributeClaim {
                    round_id: 1,
                    tranche_id: 1,
                    proposal_id: 2,
                    tribute_id: 1,
                    amount: Coin::new(Uint128::new(200), "token"),
                },
            ],
            expected_error: None,
        },
        HistoricalTributeClaimsTestCase {
            description: "User with no claimed tributes".to_string(),
            user_address: deps.api.addr_make("user2"),
            start_from: 0,
            limit: 10,
            expected_claims: vec![],
            expected_error: None,
        },
    ];

    for test_case in test_cases {
        println!("Running test case: {}", test_case.description);

        let (mut deps, _env) = (mock_dependencies(), mock_env());

        // Mock the database
        let tributes = vec![
            Tribute {
                tribute_id: 0,
                round_id: 1,
                tranche_id: 1,
                proposal_id: 1,
                depositor: Addr::unchecked("user1"),
                funds: Coin::new(Uint128::new(100), "token"),
                refunded: false,
                creation_round: 1,
                creation_time: cosmwasm_std::Timestamp::from_seconds(1),
            },
            Tribute {
                tribute_id: 1,
                round_id: 1,
                tranche_id: 1,
                proposal_id: 2,
                depositor: Addr::unchecked("user1"),
                funds: Coin::new(Uint128::new(200), "token"),
                refunded: false,
                creation_round: 1,
                creation_time: cosmwasm_std::Timestamp::from_seconds(1),
            },
        ];

        for (i, tribute) in tributes.iter().enumerate() {
            ID_TO_TRIBUTE_MAP
                .save(&mut deps.storage, i as u64, tribute)
                .unwrap();
            TRIBUTE_CLAIMS
                .save(
                    &mut deps.storage,
                    (deps.api.addr_make("user1"), i as u64),
                    &tribute.funds.clone(),
                )
                .unwrap();
        }

        // Query historical tribute claims
        let result = query_historical_tribute_claims(
            &deps.as_ref(),
            test_case.user_address.to_string(),
            test_case.start_from,
            test_case.limit,
        );

        match result {
            Ok(claims) => {
                assert_eq!(claims.claims, test_case.expected_claims);
            }
            Err(err) => {
                assert_eq!(Some(err), test_case.expected_error);
            }
        }
    }
}

struct RoundTributesTestCase {
    description: String,
    round_id: u64,
    start_from: u32,
    limit: u32,
    expected_tributes: Vec<Tribute>,
    expected_error: Option<StdError>,
}

#[test]
fn test_query_round_tributes() {
    // Mock the database
    let tributes = vec![
        Tribute {
            tribute_id: 1,
            round_id: 1,
            tranche_id: 1,
            proposal_id: 1,
            depositor: Addr::unchecked("user1"),
            funds: Coin::new(Uint128::new(100), "token"),
            refunded: false,
            creation_round: 1,
            creation_time: cosmwasm_std::Timestamp::from_seconds(1),
        },
        Tribute {
            tribute_id: 2,
            round_id: 1,
            tranche_id: 1,
            proposal_id: 2,
            depositor: Addr::unchecked("user2"),
            funds: Coin::new(Uint128::new(200), "token"),
            refunded: false,
            creation_round: 1,
            creation_time: cosmwasm_std::Timestamp::from_seconds(1),
        },
        Tribute {
            tribute_id: 3,
            round_id: 1,
            tranche_id: 2, // different tranche
            proposal_id: 3,
            depositor: Addr::unchecked("user3"),
            funds: Coin::new(Uint128::new(300), "token"),
            refunded: false,
            creation_round: 1,
            creation_time: cosmwasm_std::Timestamp::from_seconds(1),
        },
        Tribute {
            tribute_id: 4,
            round_id: 1,
            tranche_id: 3, // also different tranche
            proposal_id: 4,
            depositor: Addr::unchecked("user4"),
            funds: Coin::new(Uint128::new(400), "token"),
            refunded: false,
            creation_round: 1,
            creation_time: cosmwasm_std::Timestamp::from_seconds(1),
        },
        Tribute {
            tribute_id: 5,
            round_id: 2, // different round
            tranche_id: 1,
            proposal_id: 5,
            depositor: Addr::unchecked("user5"),
            funds: Coin::new(Uint128::new(500), "token"),
            refunded: false,
            creation_round: 1,
            creation_time: cosmwasm_std::Timestamp::from_seconds(1),
        },
    ];

    let test_cases = vec![
        RoundTributesTestCase {
            description: "Query first 2 tributes".to_string(),
            round_id: 1,
            start_from: 0,
            limit: 2,
            expected_tributes: vec![tributes[0].clone(), tributes[1].clone()],
            expected_error: None,
        },
        RoundTributesTestCase {
            description: "Query other tributes".to_string(),
            round_id: 1,
            start_from: 2,
            limit: 3,
            expected_tributes: vec![tributes[2].clone(), tributes[3].clone()],
            expected_error: None,
        },
        RoundTributesTestCase {
            description: "Query with start_from beyond range".to_string(),
            round_id: 1,
            start_from: 10,
            limit: 2,
            expected_tributes: vec![],
            expected_error: None,
        },
        RoundTributesTestCase {
            description: "Query different round tributes".to_string(),
            round_id: 2,
            start_from: 0,
            limit: 2,
            expected_tributes: vec![tributes[4].clone()],
            expected_error: None,
        },
    ];

    for test_case in test_cases {
        println!("Running test case: {}", test_case.description);

        let (mut deps, _env) = (mock_dependencies(), mock_env());

        for tribute in tributes.iter() {
            ID_TO_TRIBUTE_MAP
                .save(&mut deps.storage, tribute.tribute_id, tribute)
                .unwrap();
            TRIBUTE_MAP
                .save(
                    &mut deps.storage,
                    (tribute.round_id, tribute.proposal_id, tribute.tribute_id),
                    &(tribute.tribute_id),
                )
                .unwrap();
        }

        // Query round tributes
        let result = query_round_tributes(
            &deps.as_ref(),
            test_case.round_id,
            test_case.start_from,
            test_case.limit,
        );

        match result {
            Ok(tributes) => {
                assert_eq!(tributes.tributes, test_case.expected_tributes);
            }
            Err(err) => {
                assert_eq!(Some(err), test_case.expected_error);
            }
        }
    }
}

struct OutstandingTributeClaimsTestCase {
    description: String,
    user_address: Addr,
    round_id: u64,
    tranche_id: u64,
    expected_claims: Vec<TributeClaim>,
    expected_error: Option<StdError>,
}

#[test]
fn test_query_outstanding_tribute_claims() {
    // create deps to use the api
    let deps = mock_dependencies();
    let test_cases = vec![
        OutstandingTributeClaimsTestCase {
            description: "Tribute 2 is outstanding".to_string(),
            user_address: deps.api.addr_make("user1"),
            round_id: 1,
            tranche_id: 1,
            expected_claims: vec![TributeClaim {
                round_id: 1,
                tranche_id: 1,
                proposal_id: 1,
                tribute_id: 2,
                // proposal has 1500 total power, user has 2*500=1000 power, so get 200 tokens
                amount: Coin::new(Uint128::new(200), "token"),
            }],
            expected_error: None,
        },
        OutstandingTributeClaimsTestCase {
            description: "User with no outstanding tributes".to_string(),
            user_address: deps.api.addr_make("user2"),
            round_id: 1,
            tranche_id: 1,
            expected_claims: vec![],
            expected_error: None,
        },
    ];

    for test_case in test_cases {
        println!("Running test case: {}", test_case.description);

        let (mut deps, _env) = (mock_dependencies(), mock_env());

        // Mock the database
        let tributes = vec![
            Tribute {
                // this tribute will be marked as already claimed by user1
                tribute_id: 1,
                round_id: 1,
                tranche_id: 1,
                proposal_id: 1,
                depositor: Addr::unchecked("user1"),
                funds: Coin::new(Uint128::new(100), "token"),
                refunded: false,
                creation_round: 1,
                creation_time: cosmwasm_std::Timestamp::from_seconds(1),
            },
            Tribute {
                tribute_id: 2,
                round_id: 1,
                tranche_id: 1,
                proposal_id: 1,
                depositor: Addr::unchecked("user1"),
                funds: Coin::new(Uint128::new(300), "token"),
                refunded: false,
                creation_round: 1,
                creation_time: cosmwasm_std::Timestamp::from_seconds(1),
            },
            Tribute {
                tribute_id: 3,
                round_id: 2,
                tranche_id: 1,
                proposal_id: 3,
                depositor: Addr::unchecked("user1"),
                funds: Coin::new(Uint128::new(300), "token"),
                refunded: false,
                creation_round: 1,
                creation_time: cosmwasm_std::Timestamp::from_seconds(1),
            },
            Tribute {
                tribute_id: 4,
                round_id: 1,
                tranche_id: 2,
                proposal_id: 4,
                depositor: Addr::unchecked("user1"),
                funds: Coin::new(Uint128::new(400), "token"),
                refunded: false,
                creation_round: 1,
                creation_time: cosmwasm_std::Timestamp::from_seconds(1),
            },
        ];

        for tribute in tributes.iter() {
            ID_TO_TRIBUTE_MAP
                .save(&mut deps.storage, tribute.tribute_id, tribute)
                .unwrap();

            TRIBUTE_MAP
                .save(
                    &mut deps.storage,
                    (tribute.round_id, tribute.proposal_id, tribute.tribute_id),
                    &tribute.tribute_id,
                )
                .unwrap();
        }

        // Mock claimed tributes - exact amounts do not matter
        // user 1 claimed tribute 1 with lock_ids 0 and 2
        TRIBUTE_CLAIMS
            .save(
                &mut deps.storage,
                (deps.api.addr_make("user1"), 1),
                &Coin::new(Uint128::new(100), "token"),
            )
            .unwrap();
        TRIBUTE_CLAIMED_LOCKS
            .save(&mut deps.storage, (1, 0), &true)
            .unwrap();
        TRIBUTE_CLAIMED_LOCKS
            .save(&mut deps.storage, (1, 2), &true)
            .unwrap();

        // user 2 claimed both tributes with lock_id 1
        TRIBUTE_CLAIMS
            .save(
                &mut deps.storage,
                (deps.api.addr_make("user2"), 1),
                &Coin::new(Uint128::new(100), "token"),
            )
            .unwrap();
        TRIBUTE_CLAIMED_LOCKS
            .save(&mut deps.storage, (1, 1), &true)
            .unwrap();
        TRIBUTE_CLAIMS
            .save(
                &mut deps.storage,
                (deps.api.addr_make("user2"), 2),
                &Coin::new(Uint128::new(200), "token"),
            )
            .unwrap();
        TRIBUTE_CLAIMED_LOCKS
            .save(&mut deps.storage, (2, 1), &true)
            .unwrap();

        // Mock proposals and user votes
        let mock_proposals = vec![
            Proposal {
                round_id: 1,
                tranche_id: 1,
                proposal_id: 1,
                title: "Proposal 1".to_string(),
                description: "Description 1".to_string(),
                power: Uint128::new(1500),
                percentage: Uint128::new(7),
                minimum_atom_liquidity_request: Uint128::zero(),
                deployment_duration: 1,
            },
            Proposal {
                round_id: 1,
                tranche_id: 1,
                proposal_id: 2,
                title: "Proposal 2".to_string(),
                description: "Description 2".to_string(),
                power: Uint128::new(2000),
                percentage: Uint128::new(7),
                minimum_atom_liquidity_request: Uint128::zero(),
                deployment_duration: 1,
            },
        ];

        // mock liquidity deployments to make the tributes outstanding
        let liquidity_deployments = mock_proposals
            .iter()
            .map(|proposal| get_nonzero_deployment_for_proposal(proposal.clone()))
            .collect();

        let user_vote = VoteWithPower {
            prop_id: 1,
            power: Decimal::from_ratio(Uint128::new(500), Uint128::one()),
        };

        let mock_querier = MockWasmQuerier::new(
            "hydro_contract_address".to_string(),
            1,
            mock_proposals.clone(),
            vec![
                (
                    // user 1 voted on prop 1 with lock_id 0
                    1,
                    1,
                    get_address_as_str(&deps.api, "user1"),
                    user_vote.clone(),
                    0,
                ),
                (
                    // user 1 voted on prop 1 with lock_id 2
                    1,
                    1,
                    get_address_as_str(&deps.api, "user1"),
                    user_vote.clone(),
                    2,
                ),
                (
                    // user 2 voted on prop 1 with lock_id 1
                    1,
                    1,
                    get_address_as_str(&deps.api, "user2"),
                    user_vote.clone(),
                    1,
                ),
            ],
            liquidity_deployments,
            None,
        );

        deps.querier.update_wasm(move |q| mock_querier.handler(q));

        // Mock config
        let config = Config {
            hydro_contract: Addr::unchecked("hydro_contract_address".to_string()),
        };
        CONFIG.save(&mut deps.storage, &config).unwrap();

        // Query outstanding tribute claims
        let result = query_outstanding_tribute_claims(
            &deps.as_ref(),
            test_case.user_address.clone().to_string(),
            test_case.round_id,
            test_case.tranche_id,
        );

        match result {
            Ok(claims) => {
                assert_eq!(claims.claims, test_case.expected_claims);
            }
            Err(err) => {
                assert_eq!(Some(err), test_case.expected_error);
            }
        }
    }
}

#[test]
fn test_query_outstanding_lockup_claimable_coins() {
    let (mut deps, _env) = (mock_dependencies(), mock_env());

    // Setup basic config
    let hydro_contract_address = get_address_as_str(&deps.api, HYDRO_CONTRACT_ADDRESS);
    let config = Config {
        hydro_contract: Addr::unchecked(hydro_contract_address.clone()),
    };
    CONFIG.save(&mut deps.storage, &config).unwrap();

    // Create test data
    let lock_id = 123;
    let tribute_id = 10;
    let round_id = 5;
    let tranche_id = 1;
    let proposal_id = 8;
    let test_denom = "token";

    // Create test tributes
    let tribute1 = Tribute {
        tribute_id,
        round_id,
        tranche_id,
        proposal_id,
        depositor: Addr::unchecked("depositor"),
        funds: Coin {
            denom: test_denom.to_string(),
            amount: Uint128::from(100000u128),
        },
        refunded: false,
        creation_time: Timestamp::from_seconds(0),
        creation_round: round_id,
    };

    // Store tribute in maps
    TRIBUTE_MAP
        .save(
            &mut deps.storage,
            (tribute1.round_id, tribute1.proposal_id, tribute1.tribute_id),
            &tribute1.tribute_id,
        )
        .unwrap();
    ID_TO_TRIBUTE_MAP
        .save(&mut deps.storage, tribute1.tribute_id, &tribute1)
        .unwrap();

    // Create mock data with proposals, user votes, and liquidity deployments
    let mock_proposal = Proposal {
        round_id,
        tranche_id,
        proposal_id,
        title: "Test Proposal".to_string(),
        description: "Test Description".to_string(),
        power: Uint128::from(1000u128), // Total power = 1000
        percentage: Uint128::from(100u128),
        minimum_atom_liquidity_request: Uint128::from(100u128),
        deployment_duration: 1,
    };

    let user_vote = (
        round_id,
        tranche_id,
        "user".to_string(),
        VoteWithPower {
            prop_id: proposal_id,
            power: Decimal::from_ratio(500u128, 1u128), // 500 units of voting power
        },
        lock_id,
    );

    let liquidity_deployment = LiquidityDeployment {
        round_id,
        tranche_id,
        proposal_id,
        destinations: vec!["dest1".to_string()],
        deployed_funds: vec![Coin {
            denom: test_denom.to_string(),
            amount: Uint128::from(100u128),
        }],
        funds_before_deployment: vec![],
        total_rounds: 1,
        remaining_rounds: 0,
    };

    // Setup mock querier
    let mock_querier = MockWasmQuerier::new(
        hydro_contract_address,
        round_id, // current_round
        vec![mock_proposal],
        vec![user_vote],
        vec![liquidity_deployment],
        None, // hydro_constants
    );
    deps.querier.update_wasm(move |q| mock_querier.handler(q));

    // Test: Query outstanding lockup claimable coins
    let result = query_outstanding_lockup_claimable_coins(&deps.as_ref(), lock_id).unwrap();

    // Should return aggregated coins
    assert_eq!(result.coins.len(), 1);
    let coin = &result.coins[0];
    assert_eq!(coin.denom, test_denom);

    // Expected calculation: 100,000 * (500 / 1000) = 50,000 tokens
    assert_eq!(coin.amount, Uint128::from(50000u128));

    // Test: Query for non-existent lock should return empty
    let result = query_outstanding_lockup_claimable_coins(&deps.as_ref(), 999).unwrap();
    assert_eq!(result.coins.len(), 0);

    // Test: Mark tribute as claimed and verify it's excluded
    TRIBUTE_CLAIMED_LOCKS
        .save(&mut deps.storage, (tribute1.tribute_id, lock_id), &true)
        .unwrap();

    let result = query_outstanding_lockup_claimable_coins(&deps.as_ref(), lock_id).unwrap();
    assert_eq!(result.coins.len(), 0); // Should be empty since tribute was claimed
}

// Verifies that a user cannot claim additional tribute after splitting or merging the lock that was used for voting.
// During split/merge process, 0-power votes are inserted for the new locks, which means that all rewards remain
// with the original lock(s) that have been splitted/merged.
#[test]
fn claim_tribute_after_lock_split_or_merge_test() {
    let mut current_round = 10;

    let mock_proposal = Proposal {
        round_id: current_round,
        tranche_id: 0,
        proposal_id: 5,
        title: "proposal title 1".to_string(),
        description: "proposal description 1".to_string(),
        power: Uint128::new(10000),
        percentage: MIN_PROP_PERCENT_FOR_CLAIMABLE_TRIBUTES,
        minimum_atom_liquidity_request: Uint128::zero(),
        deployment_duration: 1,
    };

    let mock_proposals = vec![mock_proposal.clone()];

    let deployments = mock_proposals
        .iter()
        .map(|p| get_nonzero_deployment_for_proposal(p.clone()))
        .collect::<Vec<LiquidityDeployment>>();

    let (mut deps, env) = (mock_dependencies(), mock_env());
    let info = get_message_info(&deps.api, USER_ADDRESS_1, &[]);

    let lock_id1 = 0;
    let lock_id2 = 1;
    let tribute_id = 0;

    let hydro_contract_address = get_address_as_str(&deps.api, HYDRO_CONTRACT_ADDRESS);
    let mock_querier = MockWasmQuerier::new(
        hydro_contract_address.clone(),
        current_round,
        mock_proposals.clone(),
        vec![],
        vec![],
        None,
    );
    deps.querier.update_wasm(move |q| mock_querier.handler(q));

    let msg = get_instantiate_msg(hydro_contract_address.clone());
    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());

    let tribute_payer = USER_ADDRESS_1;
    let tribute = Coin::new(1000u64, DEFAULT_DENOM);
    let info = get_message_info(&deps.api, tribute_payer, &[tribute.clone()]);
    let msg = ExecuteMsg::AddTribute {
        tranche_id: mock_proposal.tranche_id,
        round_id: current_round,
        proposal_id: mock_proposal.proposal_id,
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok());

    // Update the expected round, proposals deployment info and user votes so that the tribute can be claimed.
    // Claim the tribute before splitting the lock that was used for voting.
    current_round += 1;

    let tribute_claimer = get_address_as_str(&deps.api, USER_ADDRESS_2);
    let user_votes = vec![(
        mock_proposal.round_id,
        mock_proposal.tranche_id,
        tribute_claimer.clone(),
        VoteWithPower {
            prop_id: mock_proposal.proposal_id,
            power: Decimal::from_ratio(mock_proposal.power, Uint128::one()),
        },
        lock_id1,
    )];
    let mock_querier = MockWasmQuerier::new(
        hydro_contract_address.clone(),
        current_round,
        mock_proposals.clone(),
        user_votes,
        deployments.clone(),
        None,
    );
    deps.querier.update_wasm(move |q| mock_querier.handler(q));

    let info = get_message_info(&deps.api, USER_ADDRESS_1, &[]);
    let msg = ExecuteMsg::ClaimTribute {
        round_id: mock_proposal.round_id,
        tranche_id: mock_proposal.tranche_id,
        tribute_id,
        voter_address: tribute_claimer.clone(),
    };

    // Just verify that the result is ok. More detailed checks are done in other tests.
    // Here we focus only on split/merge testing.
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());

    // Then mock use case if user splits the lock that was used for voting and tries to claim additional tribute.
    let user_votes = vec![
        (
            mock_proposal.round_id,
            mock_proposal.tranche_id,
            tribute_claimer.clone(),
            VoteWithPower {
                prop_id: mock_proposal.proposal_id,
                power: Decimal::from_ratio(mock_proposal.power, Uint128::one()),
            },
            lock_id1,
        ),
        (
            mock_proposal.round_id,
            mock_proposal.tranche_id,
            tribute_claimer.clone(),
            VoteWithPower {
                prop_id: mock_proposal.proposal_id,
                power: Decimal::zero(),
            },
            lock_id2,
        ),
    ];
    let mock_querier = MockWasmQuerier::new(
        hydro_contract_address.clone(),
        current_round,
        mock_proposals.clone(),
        user_votes,
        deployments.clone(),
        None,
    );
    deps.querier.update_wasm(move |q| mock_querier.handler(q));

    let info = get_message_info(&deps.api, USER_ADDRESS_1, &[]);
    let msg = ExecuteMsg::ClaimTribute {
        round_id: mock_proposal.round_id,
        tranche_id: mock_proposal.tranche_id,
        tribute_id,
        voter_address: tribute_claimer.clone(),
    };

    // Verify that an error is received, which means user can't claim additional tributes for the lock
    // that was created by either splitting or merging the lock that has already claimed the tribute.
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res
        .unwrap_err()
        .to_string()
        .contains("Nothing to claim - all locks have already claimed this tribute"));
}

#[test]
fn test_calculate_voter_claim_amount() {
    // Test case 1: Simple case with exact division
    let result = calculate_voter_claim_amount(
        coin(1000, "uatom"),
        Decimal::from_ratio(Uint128::new(50), Uint128::new(1)), // 50 voting power
        Uint128::new(100),                                      // Total power
    )
    .unwrap();
    assert_eq!(result.amount, Uint128::new(500));
    assert_eq!(result.denom, "uatom");

    // Test case 2: Real world scenario with larger numbers
    let result = calculate_voter_claim_amount(
        coin(1_000_000_000, "uatom"), // 1000 ATOM
        Decimal::from_ratio(Uint128::new(1_000_000), Uint128::new(1)), // 1_000_000 voting power
        Uint128::new(10_000_000),
    )
    .unwrap();
    assert_eq!(result.amount, Uint128::new(100_000_000)); // Should get 100 ATOM

    // Test case 3: Very small fraction of vote
    let result = calculate_voter_claim_amount(
        coin(1000, "uatom"),
        Decimal::from_ratio(Uint128::new(1), Uint128::new(1)), // 1 voting power
        Uint128::new(1000),
    )
    .unwrap();
    assert_eq!(result.amount, Uint128::new(1)); // Should get 1 unit (rounded down)

    // Test case 4: Zero voting power
    let result =
        calculate_voter_claim_amount(coin(1000, "uatom"), Decimal::zero(), Uint128::new(100))
            .unwrap();
    assert_eq!(result.amount, Uint128::zero());

    // Test case 5: Large but reasonable numbers
    let result = calculate_voter_claim_amount(
        coin(1_000_000_000_000, "uatom"),                      // 1M ATOM
        Decimal::from_ratio(Uint128::new(25), Uint128::one()), // 25% of voting power
        Uint128::new(100),                                     // Total power
    )
    .unwrap();
    assert_eq!(result.amount, Uint128::new(250_000_000_000)); // 250k ATOM
}

#[test]
fn test_calculate_voter_claim_amount_edge_cases() {
    // Test case 1: Large tribute with small share
    let result = calculate_voter_claim_amount(
        coin(1_000_000_000_000, "uatom"),                      // 1M ATOM
        Decimal::from_ratio(Uint128::new(1), Uint128::new(1)), // 0.0001% power
        Uint128::new(1_000_000),
    )
    .unwrap();
    assert_eq!(result.amount, Uint128::new(1_000_000)); // 1 ATOM

    // Test case 2: Max practical voting power ratio
    let result = calculate_voter_claim_amount(
        coin(13850000000000000000000, "uatom"),
        Decimal::from_ratio(Uint128::new(10000000), Uint128::new(1)), // 100% of voting power
        Uint128::new(10000000),
    )
    .unwrap();
    assert_eq!(result.amount, Uint128::new(13850000000000000000000));

    // Test case 3: Large numbers within decimal precision
    let result = calculate_voter_claim_amount(
        coin(1_000_000_000_000, "uatom"), // 1M ATOM
        Decimal::from_ratio(Uint128::new(1_000_000), Uint128::new(1)), // 100% power
        Uint128::new(1_000_000),
    )
    .unwrap();
    assert_eq!(result.amount, Uint128::new(1_000_000_000_000)); // Should get full amount
}

#[test]
#[should_panic(expected = "Failed to compute tribute amount")]
fn test_calculate_voter_claim_amount_zero_total_power() {
    calculate_voter_claim_amount(
        coin(1000, "uatom"),
        Decimal::percent(50),
        Uint128::zero(), // This should cause a division by zero error
    )
    .unwrap();
}

#[test]
fn test_calculate_voter_claim_amount_precision() {
    // Test rounding behavior with small amounts
    let result = calculate_voter_claim_amount(
        coin(10, "uatom"),
        Decimal::from_ratio(Uint128::new(10), Uint128::new(3)), // 1/3 of voting power
        Uint128::new(10),
    )
    .unwrap();
    assert_eq!(result.amount, Uint128::new(3)); // Should round down to 3

    // Test precision with typical staking amounts
    let result = calculate_voter_claim_amount(
        coin(1_000_000, "uatom"),                               // 1 ATOM
        Decimal::from_ratio(Uint128::new(10), Uint128::new(3)), // 1/3 of voting power
        Uint128::new(10),
    )
    .unwrap();
    assert_eq!(result.amount, Uint128::new(333_333)); // Should get roughly 1/3
}
