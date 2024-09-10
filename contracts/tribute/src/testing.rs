use crate::{
    contract::{
        calculate_voter_claim_amount, execute, get_community_pool_tribute_share,
        get_voters_tribute_share, instantiate, query_historical_tribute_claims,
        query_outstanding_tribute_claims, query_proposal_tributes, query_round_tributes,
    },
    error::ContractError,
    msg::{CommunityPoolTaxConfig, ExecuteMsg, InstantiateMsg},
    query::TributeClaim,
    state::{Config, Tribute, CONFIG, ID_TO_TRIBUTE_MAP, TRIBUTE_CLAIMS, TRIBUTE_MAP},
};
use cosmwasm_std::{coin, BankMsg, Coin, CosmosMsg};
use cosmwasm_std::{
    from_json,
    testing::{mock_dependencies, mock_env, MockApi, MockQuerier},
    to_json_binary, Addr, Binary, ContractResult, Decimal, Env, IbcMsg, MemoryStorage, MessageInfo,
    OwnedDeps, QuerierResult, Response, StdError, StdResult, SubMsg, SystemError, SystemResult,
    Timestamp, Uint128, WasmQuery,
};
use hydro::{
    query::{
        CurrentRoundResponse, ProposalResponse, QueryMsg as HydroQueryMsg, TopNProposalsResponse,
        UserVoteResponse,
    },
    state::{Proposal, VoteWithPower},
};
use proptest::prelude::*;

pub fn get_instantiate_msg(hydro_contract: String) -> InstantiateMsg {
    InstantiateMsg {
        hydro_contract,
        top_n_props_count: 10,
        community_pool_config: CommunityPoolTaxConfig {
            tax_percent: Decimal::zero(),
            channel_id: "channel_id".to_string(),
            bucket_address: "community_pool_address".to_string(),
        },
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
                                    error: "proposal couldn't be found".to_string(),
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
    // (tranche_id, proposal_id)
    proposal_info: (u64, u64),
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
            proposal_info: (0, 5),
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
            proposal_info: (0, 5),
            tributes_to_add: vec![vec![Coin::new(1000u64, DEFAULT_DENOM)]],
            mock_data: (10, vec![]),
            expected_success: false,
            expected_error_msg: "proposal couldn't be found".to_string(),
        },
        AddTributeTestCase {
            description: "try adding tribute without providing any funds".to_string(),
            proposal_info: (0, 5),
            tributes_to_add: vec![vec![]],
            mock_data: (10, vec![mock_proposal.clone()]),
            expected_success: false,
            expected_error_msg: "Must send funds to add tribute".to_string(),
        },
        AddTributeTestCase {
            description: "try adding tribute by providing more than one token".to_string(),
            proposal_info: (0, 5),
            tributes_to_add: vec![vec![
                Coin::new(1000u64, DEFAULT_DENOM),
                Coin::new(1000u64, "stake"),
            ]],
            mock_data: (10, vec![mock_proposal.clone()]),
            expected_success: false,
            expected_error_msg: "Must send exactly one coin".to_string(),
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

        let msg = get_instantiate_msg(hydro_contract_address);
        let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
        assert!(res.is_ok());

        let tribute_payer = USER_ADDRESS_1;
        let tribute_payer_addr = get_address_as_str(&deps.api, tribute_payer);

        for tribute in &test.tributes_to_add {
            let info = get_message_info(&deps.api, tribute_payer, tribute);
            let msg = ExecuteMsg::AddTribute {
                tranche_id: test.proposal_info.0,
                proposal_id: test.proposal_info.1,
            };

            let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
            if test.expected_success {
                assert!(res.is_ok());
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
            test.mock_data.0,
            test.proposal_info.1,
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
        }
    }
}

#[test]
fn claim_tribute_test() {
    let mock_proposals = vec![Proposal {
        round_id: 10,
        tranche_id: 0,
        proposal_id: 5,
        title: "proposal title 1".to_string(),
        description: "proposal description 1".to_string(),
        power: Uint128::new(10000),
        percentage: Uint128::zero(),
    }];

    let mock_top_n_proposals = vec![
        Proposal {
            round_id: 10,
            tranche_id: 0,
            proposal_id: 5,
            title: "proposal title 1".to_string(),
            description: "proposal description 1".to_string(),
            power: Uint128::new(10000),
            percentage: Uint128::zero(),
        },
        Proposal {
            round_id: 10,
            tranche_id: 0,
            proposal_id: 6,
            title: "proposal title 2".to_string(),
            description: "proposal description 2".to_string(),
            power: Uint128::new(10000),
            percentage: Uint128::zero(),
        },
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

        let msg = get_instantiate_msg(hydro_contract_address.clone());
        let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
        assert!(res.is_ok());

        let tribute_payer = USER_ADDRESS_1;
        let info = get_message_info(&deps.api, tribute_payer, &test.tribute_to_add);
        let msg = ExecuteMsg::AddTribute {
            tranche_id: test.tribute_info.1,
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

    let mock_top_n_proposals = vec![Proposal {
        round_id: 10,
        tranche_id: 0,
        proposal_id: 6,
        title: "proposal title 2".to_string(),
        description: "proposal description 2".to_string(),
        power: Uint128::new(10000),
        percentage: Uint128::zero(),
    }];

    let test_cases: Vec<RefundTributeTestCase> = vec![
        RefundTributeTestCase {
            description: "happy path".to_string(),
            tribute_info: (10, 0, 5, 0),
            tribute_to_add: vec![Coin::new(1000u64, DEFAULT_DENOM)],
            mock_data: (10, 11, mock_proposals.clone(), mock_top_n_proposals.clone()),
            tribute_refunder: None,
            expected_tribute_refund: 1000,
            expected_success: true,
            expected_error_msg: String::new(),
        },
        RefundTributeTestCase {
            description: "try to get refund for the current round".to_string(),
            tribute_info: (10, 0, 5, 0),
            tribute_to_add: vec![Coin::new(1000u64, DEFAULT_DENOM)],
            mock_data: (10, 10, mock_proposals.clone(), mock_top_n_proposals.clone()),
            tribute_refunder: None,
            expected_tribute_refund: 0,
            expected_success: false,
            expected_error_msg: "Round has not ended yet".to_string(),
        },
        RefundTributeTestCase {
            description: "try to get refund for the top N proposal".to_string(),
            tribute_info: (10, 0, 5, 0),
            tribute_to_add: vec![Coin::new(1000u64, DEFAULT_DENOM)],
            mock_data: (10, 11, mock_proposals.clone(), mock_proposals.clone()),
            tribute_refunder: None,
            expected_tribute_refund: 0,
            expected_success: false,
            expected_error_msg: "Can't refund top N proposal".to_string(),
        },
        RefundTributeTestCase {
            description: "try to get refund for non existing tribute".to_string(),
            tribute_info: (10, 0, 5, 1),
            tribute_to_add: vec![Coin::new(1000u64, DEFAULT_DENOM)],
            mock_data: (10, 11, mock_proposals.clone(), mock_top_n_proposals.clone()),
            tribute_refunder: None,
            expected_tribute_refund: 0,
            expected_success: false,
            expected_error_msg: "not found".to_string(),
        },
        RefundTributeTestCase {
            description: "try to get refund if not the depositor".to_string(),
            tribute_info: (10, 0, 5, 0),
            tribute_to_add: vec![Coin::new(1000u64, DEFAULT_DENOM)],
            mock_data: (10, 11, mock_proposals.clone(), mock_top_n_proposals.clone()),
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

        let msg = get_instantiate_msg(hydro_contract_address.clone());
        let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
        assert!(res.is_ok());

        let tribute_payer = USER_ADDRESS_1;
        let info = get_message_info(&deps.api, tribute_payer, &test.tribute_to_add);
        let msg = ExecuteMsg::AddTribute {
            tranche_id: test.tribute_info.1,
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

fn verify_ibc_tokens_received(
    res: SubMsg,
    expected_receiver: &String,
    expected_channel_id: &String,
    expected_denom: &String,
    expected_amount: u128,
) {
    match &res.msg {
        CosmosMsg::Ibc(IbcMsg::Transfer {
            channel_id,
            to_address,
            amount,
            timeout: _,
            memo: _,
        }) => {
            assert_eq!(*expected_channel_id, *channel_id);
            assert_eq!(*expected_receiver, *to_address);
            assert_eq!(*expected_denom, amount.denom);
            assert_eq!(expected_amount, amount.amount.u128());
        }
        _ => panic!("expected CosmosMsg::Bank msg"),
    };
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000000))] // set the number of test cases to run
    #[test]
    fn test_tribute_shares(total_amount in 0u128..=1_000_000_000u128, tax_percent in 0u64..=100u64) {
        let funds = Coin {
            denom: "token".to_string(),
            amount: Uint128::new(total_amount),
        };

        let config = Config {
            community_pool_config: CommunityPoolTaxConfig {
                tax_percent: Decimal::percent(tax_percent),
                channel_id: "channel_id".to_string(),
                bucket_address: "community_pool_address".to_string(),
            },
            top_n_props_count: 10,
            hydro_contract: Addr::unchecked("hydro_contract".to_string()),
        };

        let community_pool_share = get_community_pool_tribute_share(&config, funds.clone()).unwrap();
        let voters_share = get_voters_tribute_share(&config, funds.clone()).unwrap();

        assert_eq!(community_pool_share + voters_share, funds.amount);

        // if the tax percent is 100, the voter share should be 0
        if tax_percent == 100 {
            assert!(voters_share.is_zero());
            // community pool amount should be equal to the total tribute
            assert_eq!(community_pool_share, funds.amount);
        }

        // if the tax percent is 0, the community pool share should be 0
        if tax_percent == 0 {
            assert!(community_pool_share.is_zero());
            // voters share should be equal to the total tribute
            assert_eq!(voters_share, funds.amount);
        }
    }
}

// proptest! {
//     // The test will create 3 proposals, add tribute to all of them.
//     // * Two proposals are in the same round&tranche
//     // * Third prop is in a different tranche
//     // The latter proposal exists to check that the claim function only claims tribute for the specified tranche.
//     //
//     // The test will try to claim the community pool tribute before the round has ended, which should fail.
//     // It then updates the round, and tries to claim the tribute for the community pool again, verifying that an IBC message with the right amount of tokens is sent.
//     // Then, it will try to claim the community pool tribute again (which should not send any tribute, because it was already claimed).
//     // Lastly, it claims the tribute for a voter, and verifies that the portion of the tribute the voter receives is correctly taking into account the community pool tax.
//     #![proptest_config(ProptestConfig::with_cases(1000))] // set the number of test cases to run
#[test]
fn claim_community_pool_tribute_test() {
    //tribute_amount1 in 0u64..=1_000_000_000u64, tribute_amount2 in 0u64..=1_000_000_000u64, community_pool_tax_percent in 0u64..=100u64) {
    let tribute_amount1 = 1000;
    let tribute_amount2 = 5000;
    let community_pool_tax_percent = 10;

    let expected_community_pool_tax1 = tribute_amount1 * community_pool_tax_percent / 100;
    let expected_community_pool_tax2 = tribute_amount2 * community_pool_tax_percent / 100;

    let mock_top_n_proposals = vec![
        Proposal {
            round_id: 0,
            tranche_id: 0,
            proposal_id: 0,
            title: "proposal title 1".to_string(),
            description: "proposal description 1".to_string(),
            power: Uint128::new(10000),
            percentage: Uint128::zero(),
        },
        Proposal {
            round_id: 0,
            tranche_id: 0,
            proposal_id: 1,
            title: "proposal title 2".to_string(),
            description: "proposal description 2".to_string(),
            power: Uint128::new(10000),
            percentage: Uint128::zero(),
        },
        // proposal in a different tranche
        Proposal {
            round_id: 0,
            tranche_id: 1,
            proposal_id: 2,
            title: "proposal title 3".to_string(),
            description: "proposal description 3".to_string(),
            power: Uint128::new(10000),
            percentage: Uint128::zero(),
        },
    ];
    // we will query for the top props of tranche 0, so don't return prop with id 2
    let top_n_proposals = mock_top_n_proposals[0..=1].to_vec();

    let (mut deps, env) = (mock_dependencies(), mock_env());
    let info = get_message_info(&deps.api, USER_ADDRESS_1, &[]);

    let hydro_contract_address = get_address_as_str(&deps.api, HYDRO_CONTRACT_ADDRESS);
    let mock_querier = MockWasmQuerier::new(
        hydro_contract_address.clone(),
        0,
        mock_top_n_proposals.clone(),
        vec![],
        top_n_proposals.clone(),
    );
    deps.querier.update_wasm(move |q| mock_querier.handler(q));

    let mut msg = get_instantiate_msg(hydro_contract_address.clone());
    // set the tax percent to 10%
    msg.community_pool_config.tax_percent = Decimal::percent(community_pool_tax_percent);
    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());

    // add a tribute to proposal 0
    let res = add_tribute_helper(
        &mut deps,
        env.clone(),
        USER_ADDRESS_1,
        tribute_amount1,
        "uatom".to_string(),
        0,
        0,
    );
    assert!(res.is_ok(), "failed to add tribute: {}", res.unwrap_err());

    // add a tribute to proposal 1
    let res = add_tribute_helper(
        &mut deps,
        env.clone(),
        USER_ADDRESS_1,
        tribute_amount2,
        "untrn".to_string(),
        0,
        1,
    );
    assert!(res.is_ok(), "failed to add tribute: {}", res.unwrap_err());

    // add tributes to the other proposals, too (but the exact amount is not important)
    let res = add_tribute_helper(
        &mut deps,
        env.clone(),
        USER_ADDRESS_1,
        1000,
        "uthree".to_string(),
        1,
        2,
    );
    assert!(res.is_ok(), "failed to add tribute: {}", res.unwrap_err());

    // try to claim tribute for the community pool; but the round has not ended yet
    let info = get_message_info(&deps.api, USER_ADDRESS_1, &[]);
    let msg = ExecuteMsg::ClaimCommunityPoolTribute {
        round_id: 0,
        tranche_id: 0,
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_err());
    assert!(res
        .err()
        .unwrap()
        .to_string()
        .contains("Round has not ended yet"));

    // update the round so that the tribute can be claimed, and to simulate that a user has voted on the prop
    let user_vote = (
        0, // round_id
        0, // tranche_id
        get_address_as_str(&deps.api, USER_ADDRESS_1),
        VoteWithPower {
            prop_id: 0,
            power: Decimal::from_ratio(mock_top_n_proposals[0].power, Uint128::new(2)), // user has 50% of the voting power
        },
    );

    let mock_querier = MockWasmQuerier::new(
        hydro_contract_address.clone(),
        1,
        mock_top_n_proposals.clone(),
        vec![user_vote],
        top_n_proposals,
    );
    deps.querier.update_wasm(move |q| mock_querier.handler(q));

    // try to claim again; this time it should succeed
    let info = get_message_info(&deps.api, USER_ADDRESS_1, &[]);
    let msg = ExecuteMsg::ClaimCommunityPoolTribute {
        round_id: 0,
        tranche_id: 0,
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok(), "failed to claim tribute: {}", res.unwrap_err());

    let res = res.unwrap();
    assert_eq!(2, res.messages.len());
    // verify that an ibc message was sent to claim the tokens for the community pool
    verify_ibc_tokens_received(
        res.clone().messages[0].clone(),
        &"community_pool_address".to_string(),
        &"channel_id".to_string(),
        &"uatom".to_string(),
        expected_community_pool_tax1.into(),
    );
    verify_ibc_tokens_received(
        res.clone().messages[1].clone(),
        &"community_pool_address".to_string(),
        &"channel_id".to_string(),
        &"untrn".to_string(),
        expected_community_pool_tax2.into(),
    );
    verify_claimed_tributes_count(res, 2);

    // try to claim tribute again - it should succeed, but no extra tokens should be sent, because the tribute was already claimed
    let info = get_message_info(&deps.api, USER_ADDRESS_1, &[]);
    let msg = ExecuteMsg::ClaimCommunityPoolTribute {
        round_id: 0,
        tranche_id: 0,
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok(), "failed to claim tribute: {}", res.unwrap_err());

    let res = res.unwrap();
    // no message in the response, in particular no IBC message, so no tokens are sent
    assert_eq!(0, res.messages.len());
    verify_claimed_tributes_count(res, 0);

    // user claims tribute for proposal 1
    let info = get_message_info(&deps.api, USER_ADDRESS_1, &[]);
    let msg = ExecuteMsg::ClaimTribute {
        round_id: 0,
        tranche_id: 0,
        tribute_id: 0,
        voter_address: get_address_as_str(&deps.api, USER_ADDRESS_1),
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok(), "failed to claim tribute: {}", res.unwrap_err());

    let res = res.unwrap();
    assert_eq!(1, res.messages.len());

    verify_tokens_received(
        res,
        &get_address_as_str(&deps.api, USER_ADDRESS_1),
        &DEFAULT_DENOM.to_string(),
        ((tribute_amount1 - expected_community_pool_tax1) / 2).into(), // user has 50% of the voting power
    );
}
// }

fn add_tribute_helper(
    deps: &mut OwnedDeps<MemoryStorage, MockApi, MockQuerier>,
    env: Env,
    tribute_payer: &str,
    tribute_amount: u64,
    denom: String,
    tranche_id: u64,
    proposal_id: u64,
) -> Result<Response, ContractError> {
    let info = get_message_info(
        &deps.api,
        tribute_payer,
        &[Coin::new(tribute_amount, denom)],
    );
    let msg = ExecuteMsg::AddTribute {
        tranche_id,
        proposal_id,
    };
    execute(deps.as_mut(), env, info, msg)
}

// Verifies that in the response, the claimed_tributes_count attribute
// has the expected value
fn verify_claimed_tributes_count(res: Response, expected_num_of_tributes: u128) {
    // assert that the claimed tribute count in the response is 1
    let claimed_tribute_count = res
        .attributes
        .iter()
        .find(|attr| attr.key == "claimed_tributes_count")
        .unwrap()
        .clone()
        .value;
    assert_eq!(
        expected_num_of_tributes,
        claimed_tribute_count.parse::<u128>().unwrap()
    );
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
            },
            Tribute {
                tribute_id: 1,
                round_id: 1,
                tranche_id: 1,
                proposal_id: 2,
                depositor: Addr::unchecked("user1"),
                funds: Coin::new(Uint128::new(200), "token"),
                refunded: false,
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
        },
        Tribute {
            tribute_id: 2,
            round_id: 1,
            tranche_id: 1,
            proposal_id: 2,
            depositor: Addr::unchecked("user2"),
            funds: Coin::new(Uint128::new(200), "token"),
            refunded: false,
        },
        Tribute {
            tribute_id: 3,
            round_id: 1,
            tranche_id: 2, // different tranche
            proposal_id: 3,
            depositor: Addr::unchecked("user3"),
            funds: Coin::new(Uint128::new(300), "token"),
            refunded: false,
        },
        Tribute {
            tribute_id: 4,
            round_id: 1,
            tranche_id: 3, // also different tranche
            proposal_id: 4,
            depositor: Addr::unchecked("user4"),
            funds: Coin::new(Uint128::new(400), "token"),
            refunded: false,
        },
        Tribute {
            tribute_id: 5,
            round_id: 2, // different round
            tranche_id: 1,
            proposal_id: 5,
            depositor: Addr::unchecked("user5"),
            funds: Coin::new(Uint128::new(500), "token"),
            refunded: false,
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

fn get_config_with_community_pool_tax(tax_percent: u64) -> Config {
    Config {
        hydro_contract: Addr::unchecked("hydro_contract".to_string()),
        top_n_props_count: 1,
        community_pool_config: CommunityPoolTaxConfig {
            tax_percent: Decimal::percent(tax_percent),
            channel_id: "".to_string(),
            bucket_address: "".to_string(),
        },
    }
}

fn decimal(value: u128) -> Decimal {
    Decimal::from_ratio(Uint128::new(value), Uint128::one())
}

#[test]
fn test_calculate_voter_claim_amount() {
    struct TestCase {
        description: &'static str,
        config: Config,
        tribute_funds: Coin,
        user_voting_power: Decimal,
        total_proposal_power: Uint128,
        expected_result: Result<Coin, ContractError>,
    }

    let test_cases = vec![
        TestCase {
            description: "Happy path with 0% community pool tax",
            config: get_config_with_community_pool_tax(0),
            tribute_funds: coin(1000, "token"),
            user_voting_power: decimal(100),
            total_proposal_power: Uint128::new(1000),
            expected_result: Ok(coin(100, "token")),
        },
        TestCase {
            description: "Happy path with 10% community pool tax",
            config: get_config_with_community_pool_tax(10),
            tribute_funds: coin(1000, "token"),
            user_voting_power: decimal(100),
            total_proposal_power: Uint128::new(1000),
            expected_result: Ok(coin(90, "token")),
        },
        TestCase {
            description: "Edge case with 0 voting power",
            config: get_config_with_community_pool_tax(10),
            tribute_funds: coin(1000, "token"),
            user_voting_power: Decimal::zero(),
            total_proposal_power: Uint128::new(1000),
            expected_result: Ok(coin(0, "token")),
        },
        TestCase {
            description: "Edge case with 0 total proposal power",
            config: get_config_with_community_pool_tax(10),
            tribute_funds: coin(1000, "token"),
            user_voting_power: decimal(100),
            total_proposal_power: Uint128::zero(),
            expected_result: Err(ContractError::Std(StdError::generic_err(
                "Failed to compute users voting power percentage",
            ))),
        },
        TestCase {
            description: "Edge case with 100% community pool tax",
            config: get_config_with_community_pool_tax(100),
            tribute_funds: coin(1000, "token"),
            user_voting_power: decimal(10),
            total_proposal_power: Uint128::new(1000),
            expected_result: Ok(coin(0, "token")),
        },
    ];

    for test_case in test_cases {
        println!("Running test case: {}", test_case.description);

        let result = calculate_voter_claim_amount(
            test_case.config,
            test_case.tribute_funds.clone(),
            test_case.user_voting_power,
            test_case.total_proposal_power,
        );

        match (result, test_case.expected_result) {
            (Ok(result_coin), Ok(expected_coin)) => assert_eq!(result_coin, expected_coin),
            (Err(result_err), Err(expected_err)) => {
                assert_eq!(result_err.to_string(), expected_err.to_string())
            }
            _ => panic!(
                "Test case '{}' failed: result and expected result do not match",
                test_case.description
            ),
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
                // proposal has 2000 total power, user has 500 power, 10% community pool tax, so get 90 tokens
                amount: Coin::new(Uint128::new(90), "token"),
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
            },
            Tribute {
                tribute_id: 2,
                round_id: 1,
                tranche_id: 1,
                proposal_id: 1,
                depositor: Addr::unchecked("user1"),
                funds: Coin::new(Uint128::new(200), "token"),
                refunded: false,
            },
            Tribute {
                tribute_id: 3,
                round_id: 2,
                tranche_id: 1,
                proposal_id: 3,
                depositor: Addr::unchecked("user1"),
                funds: Coin::new(Uint128::new(300), "token"),
                refunded: false,
            },
            Tribute {
                tribute_id: 4,
                round_id: 1,
                tranche_id: 2,
                proposal_id: 4,
                depositor: Addr::unchecked("user1"),
                funds: Coin::new(Uint128::new(400), "token"),
                refunded: false,
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
            power: decimal(500),
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
        let mut config = get_config_with_community_pool_tax(10);
        config.hydro_contract = Addr::unchecked("hydro_contract_address".to_string());
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
