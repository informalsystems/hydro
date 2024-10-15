use crate::{
    contract::{
        execute, instantiate, query_historical_tribute_claims, query_outstanding_tribute_claims,
        query_proposal_tributes, query_round_tributes, CONTRACT_NAME,
    },
    migrate::migrate,
    msg::{ExecuteMsg, InstantiateMsg, MigrateMsg},
    query::TributeClaim,
    state::{Config, ConfigV1, Tribute, CONFIG, ID_TO_TRIBUTE_MAP, TRIBUTE_CLAIMS, TRIBUTE_MAP},
};
use cosmwasm_std::{
    from_json,
    testing::{mock_dependencies, mock_env, MockApi},
    to_json_binary, Addr, Binary, ContractResult, Decimal, MessageInfo, QuerierResult, Response,
    StdError, StdResult, SystemError, SystemResult, Timestamp, Uint128, WasmQuery,
};
use cosmwasm_std::{BankMsg, Coin, CosmosMsg};
use cw2::set_contract_version;
use cw_storage_plus::Item;
use hydro::{
    query::{
        CurrentRoundResponse, ProposalResponse, QueryMsg as HydroQueryMsg, TopNProposalsResponse,
        UserVoteResponse,
    },
    state::{Proposal, VoteWithPower},
};

pub fn get_instantiate_msg(
    hydro_contract: String,
    min_prop_percent_for_claimable_tributes: Uint128,
) -> InstantiateMsg {
    InstantiateMsg {
        hydro_contract,
        top_n_props_count: 10,
        min_prop_percent_for_claimable_tributes,
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
    top_n_proposals: Vec<Proposal>,
}

impl MockWasmQuerier {
    fn new(
        hydro_contract: String,
        current_round: u64,
        proposals: Vec<Proposal>,
        user_votes: Vec<UserVote>,
        top_n_proposals: Vec<Proposal>,
    ) -> Self {
        Self {
            hydro_contract,
            current_round,
            proposals,
            user_votes,
            top_n_proposals,
        }
    }

    fn handler(&self, query: &WasmQuery) -> QuerierResult {
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
                                    error: format!("proposal couldn't be found: round_id={}, tranche_id={}, proposal_id={}", round_id, tranche_id, proposal_id),
                                    request: Binary::new(vec![]),
                                })
                            }
                        }
                    }),
                    HydroQueryMsg::UserVote {
                        round_id,
                        tranche_id,
                        address,
                    } => Ok({
                        let res =
                            self.find_matching_user_vote(round_id, tranche_id, address.as_str());

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
                    HydroQueryMsg::TopNProposals {
                        round_id: _,
                        tranche_id: _,
                        number_of_proposals: _,
                    } => to_json_binary(&TopNProposalsResponse {
                        proposals: self.top_n_proposals.clone(),
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

    fn find_matching_user_vote(
        &self,
        round_id: u64,
        tranche_id: u64,
        address: &str,
    ) -> StdResult<Binary> {
        for (vote_round_id, vote_tranche_id, vote_address, vote) in &self.user_votes {
            if *vote_round_id == round_id
                && *vote_tranche_id == tranche_id
                && vote_address == address
            {
                let res: StdResult<Binary> =
                    to_json_binary(&UserVoteResponse { vote: vote.clone() });
                return res;
            }
        }
        StdResult::Err(StdError::generic_err("vote couldn't be found"))
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
    // (round_id, tranche_id, proposal_id, tribute_id)
    tribute_info: (u64, u64, u64, u64),
    tribute_to_add: Vec<Coin>,
    mock_data: ClaimTributeMockData,
    expected_tribute_claim: u128,
    expected_success: bool,
    expected_error_msg: String,
}

// to make clippy happy :)
// (add_tribute_round_id, claim_tribute_round_id, proposals, user_vote, top_n_proposals)
type ClaimTributeMockData = (u64, u64, Vec<Proposal>, Vec<UserVote>, Vec<Proposal>);

type UserVote = (u64, u64, String, VoteWithPower); // (round_id, tranche_id, address, VoteWithPower)

struct RefundTributeTestCase {
    description: String,
    // (round_id, tranche_id, proposal_id, tribute_id)
    tribute_info: (u64, u64, u64, u64),
    tribute_to_add: Vec<Coin>,
    // (add_tribute_round_id, refund_tribute_round_id, proposals, top_n_proposals)
    mock_data: (u64, u64, Vec<Proposal>, Vec<Proposal>),
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
        );
        deps.querier.update_wasm(move |q| mock_querier.handler(q));

        let msg = get_instantiate_msg(
            hydro_contract_address,
            MIN_PROP_PERCENT_FOR_CLAIMABLE_TRIBUTES,
        );
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
    };
    let mock_proposal2 = Proposal {
        round_id: 10,
        tranche_id: 0,
        proposal_id: 6,
        title: "proposal title 2".to_string(),
        description: "proposal description 2".to_string(),
        power: Uint128::new(10000),
        percentage: MIN_PROP_PERCENT_FOR_CLAIMABLE_TRIBUTES,
    };

    let mock_proposals = vec![mock_proposal1.clone(), mock_proposal2.clone()];

    let mock_top_n_proposals = vec![mock_proposal1.clone(), mock_proposal2.clone()];

    let mock_top_n_voting_threshold_not_reached = vec![
        Proposal {
            percentage: MIN_PROP_PERCENT_FOR_CLAIMABLE_TRIBUTES - Uint128::one(),
            ..mock_proposal1.clone()
        },
        mock_proposal2.clone(),
    ];

    let deps = mock_dependencies();
    let test_cases: Vec<ClaimTributeTestCase> = vec![
        ClaimTributeTestCase {
            description: "happy path".to_string(),
            tribute_info: (10, 0, 5, 0),
            tribute_to_add: vec![Coin::new(1000u64, DEFAULT_DENOM)],
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
                )],
                mock_top_n_proposals.clone(),
            ),
            expected_tribute_claim: 7, // (70 / 10_000) * 1_000
            expected_success: true,
            expected_error_msg: String::new(),
        },
        ClaimTributeTestCase {
            description: "try claim tribute for proposal in current round".to_string(),
            tribute_info: (10, 0, 5, 0),
            tribute_to_add: vec![Coin::new(1000u64, DEFAULT_DENOM)],
            mock_data: (10, 10, mock_proposals.clone(), vec![], vec![]),
            expected_tribute_claim: 0,
            expected_success: false,
            expected_error_msg: "Round has not ended yet".to_string(),
        },
        ClaimTributeTestCase {
            description: "try claim tribute if user didn't vote at all".to_string(),
            tribute_info: (10, 0, 5, 0),
            tribute_to_add: vec![Coin::new(1000u64, DEFAULT_DENOM)],
            mock_data: (10, 11, mock_proposals.clone(), vec![], vec![]),
            expected_tribute_claim: 0,
            expected_success: false,
            expected_error_msg: "vote couldn't be found".to_string(),
        },
        ClaimTributeTestCase {
            description: "try claim tribute if user didn't vote for top N proposal".to_string(),
            tribute_info: (10, 0, 5, 0),
            tribute_to_add: vec![Coin::new(1000u64, DEFAULT_DENOM)],
            mock_data: (
                10,
                11,
                mock_proposals.clone(),
                vec![(
                    10,
                    0,
                    get_address_as_str(&deps.api, USER_ADDRESS_2),
                    VoteWithPower {
                        prop_id: 7,
                        power: Decimal::from_ratio(Uint128::new(70), Uint128::one()),
                    },
                )],
                mock_top_n_proposals.clone(),
            ),
            expected_tribute_claim: 0,
            expected_success: false,
            expected_error_msg: "User voted for proposal outside of top N proposals".to_string(),
        },
        ClaimTributeTestCase {
            description: "try claim tribute if user voted for top N proposal that didn't reach the voting percentage threshold".to_string(),
            tribute_info: (10, 0, 5, 0),
            tribute_to_add: vec![Coin::new(1000u64, DEFAULT_DENOM)],
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
                )],
                mock_top_n_voting_threshold_not_reached.clone(),
            ),
            expected_tribute_claim: 0,
            expected_success: false,
            expected_error_msg: format!(
                "Tribute not claimable: Proposal received less voting percentage than threshold: {} required, but is {}", MIN_PROP_PERCENT_FOR_CLAIMABLE_TRIBUTES, MIN_PROP_PERCENT_FOR_CLAIMABLE_TRIBUTES - Uint128::one()).to_string(),
        },
        ClaimTributeTestCase {
            description: "try claim tribute for non existing tribute id".to_string(),
            tribute_info: (10, 0, 5, 1),
            tribute_to_add: vec![Coin::new(1000u64, DEFAULT_DENOM)],
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
                )],
                mock_top_n_proposals.clone(),
            ),
            expected_tribute_claim: 0,
            expected_success: false,
            expected_error_msg: "not found".to_string(),
        },
        ClaimTributeTestCase {
            description: "try to claim tribute that belongs to different top N proposal than the one user voted on".to_string(),
            tribute_info: (10, 0, 5, 0),
            tribute_to_add: vec![Coin::new(1000u64, DEFAULT_DENOM)],
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
                )],
                mock_top_n_proposals.clone(),
            ),
            expected_tribute_claim: 0,
            expected_success: false,
            expected_error_msg: "The tribute with the given ID does not belong to the proposal that the user voted on".to_string(),
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
        );
        deps.querier.update_wasm(move |q| mock_querier.handler(q));

        let msg = get_instantiate_msg(
            hydro_contract_address.clone(),
            MIN_PROP_PERCENT_FOR_CLAIMABLE_TRIBUTES,
        );
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

        // Update the expected round so that the tribute can be claimed
        let mock_querier = MockWasmQuerier::new(
            hydro_contract_address.clone(),
            test.mock_data.1,
            test.mock_data.2.clone(),
            test.mock_data.3.clone(),
            test.mock_data.4.clone(),
        );
        deps.querier.update_wasm(move |q| mock_querier.handler(q));

        let tribute_claimer = get_address_as_str(&deps.api, USER_ADDRESS_2);
        let info = get_message_info(&deps.api, USER_ADDRESS_1, &[]);
        let msg = ExecuteMsg::ClaimTribute {
            round_id: test.tribute_info.0,
            tranche_id: test.tribute_info.1,
            tribute_id: test.tribute_info.3,
            voter_address: tribute_claimer.clone(),
        };
        let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone());

        if !test.expected_success {
            assert!(res
                .unwrap_err()
                .to_string()
                .contains(&test.expected_error_msg));
            continue;
        }

        assert!(res.is_ok());
        let res = res.unwrap();
        assert_eq!(1, res.messages.len());

        verify_tokens_received(
            res,
            &tribute_claimer.clone(),
            &test.tribute_to_add[0].denom,
            test.expected_tribute_claim,
        );

        // Verify that the same tribute can't be claimed twice for the same user
        let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone());
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("User has already claimed the tribute"))
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
    };

    let mock_proposals = vec![mock_proposal.clone()];

    let mock_top_n_different_proposals = vec![Proposal {
        round_id: 10,
        tranche_id: 0,
        proposal_id: 6,
        title: "proposal title 2".to_string(),
        description: "proposal description 2".to_string(),
        power: Uint128::new(10000),
        percentage: MIN_PROP_PERCENT_FOR_CLAIMABLE_TRIBUTES,
    }];

    let mock_top_n_voting_threshold_reached = vec![Proposal {
        percentage: MIN_PROP_PERCENT_FOR_CLAIMABLE_TRIBUTES,
        ..mock_proposal.clone()
    }];

    let mock_top_n_voting_threshold_not_reached = vec![Proposal {
        percentage: MIN_PROP_PERCENT_FOR_CLAIMABLE_TRIBUTES - Uint128::one(),
        ..mock_proposal.clone()
    }];

    let test_cases: Vec<RefundTributeTestCase> = vec![
        RefundTributeTestCase {
            description: "refund tribute for the non top N proposal".to_string(),
            tribute_info: (10, 0, 5, 0),
            tribute_to_add: vec![Coin::new(1000u64, DEFAULT_DENOM)],
            mock_data: (10, 11, mock_proposals.clone(), mock_top_n_different_proposals.clone()),
            tribute_refunder: None,
            expected_tribute_refund: 1000,
            expected_success: true,
            expected_error_msg: String::new(),
        },
        RefundTributeTestCase {
            description: "refund tribute for the top N proposal with less voting percentage than the required threshold".to_string(),
            tribute_info: (10, 0, 5, 0),
            tribute_to_add: vec![Coin::new(1000u64, DEFAULT_DENOM)],
            mock_data: (10, 11, mock_proposals.clone(), mock_top_n_voting_threshold_not_reached),
            tribute_refunder: None,
            expected_tribute_refund: 1000,
            expected_success: true,
            expected_error_msg: String::new(),
        },
        RefundTributeTestCase {
            description: "try to get refund for the current round".to_string(),
            tribute_info: (10, 0, 5, 0),
            tribute_to_add: vec![Coin::new(1000u64, DEFAULT_DENOM)],
            mock_data: (10, 10, mock_proposals.clone(), mock_top_n_different_proposals.clone()),
            tribute_refunder: None,
            expected_tribute_refund: 0,
            expected_success: false,
            expected_error_msg: "Round has not ended yet".to_string(),
        },
        RefundTributeTestCase {
            description: "try to get refund for the top N proposal with at least minimum voting percentage".to_string(),
            tribute_info: (10, 0, 5, 0),
            tribute_to_add: vec![Coin::new(1000u64, DEFAULT_DENOM)],
            mock_data: (10, 11, mock_proposals.clone(), mock_top_n_voting_threshold_reached.clone()),
            tribute_refunder: None,
            expected_tribute_refund: 0,
            expected_success: false,
            expected_error_msg: "Can't refund top N proposal that received at least the threshold of the total voting power".to_string(),
        },
        RefundTributeTestCase {
            description: "try to get refund for non existing tribute".to_string(),
            tribute_info: (10, 0, 5, 1),
            tribute_to_add: vec![Coin::new(1000u64, DEFAULT_DENOM)],
            mock_data: (10, 11, mock_proposals.clone(), mock_top_n_different_proposals.clone()),
            tribute_refunder: None,
            expected_tribute_refund: 0,
            expected_success: false,
            expected_error_msg: "not found".to_string(),
        },
        RefundTributeTestCase {
            description: "try to get refund if not the depositor".to_string(),
            tribute_info: (10, 0, 5, 0),
            tribute_to_add: vec![Coin::new(1000u64, DEFAULT_DENOM)],
            mock_data: (10, 11, mock_proposals.clone(), mock_top_n_different_proposals.clone()),
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
        );
        deps.querier.update_wasm(move |q| mock_querier.handler(q));

        let msg = get_instantiate_msg(
            hydro_contract_address.clone(),
            MIN_PROP_PERCENT_FOR_CLAIMABLE_TRIBUTES,
        );
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
            assert!(res
                .unwrap_err()
                .to_string()
                .contains(&test.expected_error_msg));
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
                assert_eq!(claims, test_case.expected_claims);
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
    start_from: u32,
    limit: u32,
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
            start_from: 0,
            limit: 10,
            expected_claims: vec![TributeClaim {
                round_id: 1,
                tranche_id: 1,
                proposal_id: 1,
                tribute_id: 2,
                // proposal has 2000 total power, user has 500 power, so get 100 tokens
                amount: Coin::new(Uint128::new(100), "token"),
            }],
            expected_error: None,
        },
        OutstandingTributeClaimsTestCase {
            description: "User with no outstanding tributes".to_string(),
            user_address: deps.api.addr_make("user2"),
            round_id: 1,
            tranche_id: 1,
            start_from: 0,
            limit: 10,
            expected_claims: vec![],
            expected_error: None,
        },
        OutstandingTributeClaimsTestCase {
            description: "Query with start_from beyond range".to_string(),
            user_address: deps.api.addr_make("user1"),
            round_id: 1,
            tranche_id: 1,
            start_from: 10,
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
                funds: Coin::new(Uint128::new(200), "token"),
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
        // user 1 claimed tribute 1
        TRIBUTE_CLAIMS
            .save(
                &mut deps.storage,
                (deps.api.addr_make("user1"), 1),
                &Coin::new(Uint128::new(100), "token"),
            )
            .unwrap();

        // user 2 claimed both tributes
        TRIBUTE_CLAIMS
            .save(
                &mut deps.storage,
                (deps.api.addr_make("user2"), 1),
                &Coin::new(Uint128::new(100), "token"),
            )
            .unwrap();
        TRIBUTE_CLAIMS
            .save(
                &mut deps.storage,
                (deps.api.addr_make("user2"), 2),
                &Coin::new(Uint128::new(200), "token"),
            )
            .unwrap();

        // Mock proposals and user votes
        let mock_proposals = vec![
            Proposal {
                round_id: 1,
                tranche_id: 1,
                proposal_id: 1,
                title: "Proposal 1".to_string(),
                description: "Description 1".to_string(),
                power: Uint128::new(1000),
                percentage: Uint128::zero(),
            },
            Proposal {
                round_id: 1,
                tranche_id: 1,
                proposal_id: 2,
                title: "Proposal 2".to_string(),
                description: "Description 2".to_string(),
                power: Uint128::new(2000),
                percentage: Uint128::zero(),
            },
        ];

        let user_vote = VoteWithPower {
            prop_id: 1,
            power: Decimal::from_ratio(Uint128::new(500), Uint128::one()),
        };

        // print this
        println!("addr: {}", get_address_as_str(&deps.api, "user1"));

        let mock_querier = MockWasmQuerier::new(
            "hydro_contract_address".to_string(),
            1,
            mock_proposals.clone(),
            vec![
                (
                    // user 1 voted on prop 1
                    1,
                    1,
                    get_address_as_str(&deps.api, "user1"),
                    user_vote.clone(),
                ),
                (
                    // user 2 voted on prop 1, too
                    1,
                    1,
                    get_address_as_str(&deps.api, "user2"),
                    user_vote.clone(),
                ),
            ],
            mock_proposals.clone(),
        );

        deps.querier.update_wasm(move |q| mock_querier.handler(q));

        // Mock config
        let config = Config {
            hydro_contract: Addr::unchecked("hydro_contract_address".to_string()),
            top_n_props_count: 2,
            min_prop_percent_for_claimable_tributes: Uint128::new(5),
        };
        CONFIG.save(&mut deps.storage, &config).unwrap();

        // Query outstanding tribute claims
        let result = query_outstanding_tribute_claims(
            &deps.as_ref(),
            test_case.user_address.clone().to_string(),
            test_case.round_id,
            test_case.tranche_id,
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

#[test]
fn test_migrate() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let hydro_contract_address = deps.api.addr_make(HYDRO_CONTRACT_ADDRESS);

    let old_config = ConfigV1 {
        hydro_contract: hydro_contract_address,
        top_n_props_count: 77,
    };

    // Save old version of the config into the store
    const OLD_CONFIG: Item<ConfigV1> = Item::new("config");
    let result = OLD_CONFIG.save(&mut deps.storage, &old_config);
    assert!(result.is_ok());

    // Set the V1 contract version
    let result = set_contract_version(&mut deps.storage, CONTRACT_NAME, "1.0.0");
    assert!(result.is_ok());

    // Try to migrate to the new config by setting the percentage above 100%
    let msg = MigrateMsg {
        min_prop_percent_for_claimable_tributes: Uint128::new(101),
    };
    let result = migrate(deps.as_mut(), env.clone(), msg.clone());
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .to_lowercase()
        .contains("minimum proposal percentage for claimable tributes must be between 0 and 100."));

    // Try to migrate to a new (valid) config
    let msg = MigrateMsg {
        min_prop_percent_for_claimable_tributes: Uint128::new(5),
    };
    let result = migrate(deps.as_mut(), env.clone(), msg.clone());
    assert!(result.is_ok());

    // Assert that the migration was successful
    let new_config = CONFIG.load(&deps.storage).unwrap();
    assert_eq!(old_config.hydro_contract, new_config.hydro_contract);
    assert_eq!(old_config.top_n_props_count, new_config.top_n_props_count);
    assert_eq!(
        msg.min_prop_percent_for_claimable_tributes,
        new_config.min_prop_percent_for_claimable_tributes
    );

    // Try to migrate already migrated contract and verify this errors out
    let result = migrate(deps.as_mut(), env.clone(), msg.clone());
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .to_lowercase()
        .contains("contract is already migrated to the newest version."))
}
