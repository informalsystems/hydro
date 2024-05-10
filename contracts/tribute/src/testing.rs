use crate::{
    contract::{execute, instantiate, query_proposal_tributes},
    msg::{ExecuteMsg, InstantiateMsg},
};
use atom_wars::query::QueryMsg as AtomWarsQueryMsg;
use atom_wars::state::{CovenantParams, Proposal, Vote};
use cosmwasm_std::{
    from_json,
    testing::{mock_dependencies, mock_env, mock_info},
    to_json_binary, Binary, ContractResult, QuerierResult, Response, SystemError, SystemResult,
    Uint128, WasmQuery,
};
use cosmwasm_std::{BankMsg, Coin, CosmosMsg};

pub fn get_instantiate_msg(atom_wars_contract: String) -> InstantiateMsg {
    InstantiateMsg {
        atom_wars_contract,
        top_n_props_count: 10,
    }
}

const DEFAULT_DENOM: &str = "uatom";
const ATOM_WARS_CONTRACT_ADDRESS: &str = "addr0000";
const USER_ADDRESS_1: &str = "addr0001";
const USER_ADDRESS_2: &str = "addr0002";

pub struct MockWasmQuerier {
    atom_wars_contract: String,
    current_round: u64,
    proposal: Option<Proposal>,
    user_vote: Option<(u64, u64, String, Vote)>,
    top_n_proposals: Vec<Proposal>,
}

impl MockWasmQuerier {
    fn new(
        atom_wars_contract: String,
        current_round: u64,
        proposal: Option<Proposal>,
        user_vote: Option<(u64, u64, String, Vote)>,
        top_n_proposals: Vec<Proposal>,
    ) -> Self {
        Self {
            atom_wars_contract,
            current_round,
            proposal,
            user_vote,
            top_n_proposals,
        }
    }

    fn handler(&self, query: &WasmQuery) -> QuerierResult {
        match query {
            WasmQuery::Smart { contract_addr, msg } => {
                if *contract_addr != self.atom_wars_contract {
                    return SystemResult::Err(SystemError::NoSuchContract {
                        addr: contract_addr.to_string(),
                    });
                }

                let response = match from_json(msg).unwrap() {
                    AtomWarsQueryMsg::CurrentRound {} => to_json_binary(&self.current_round),
                    AtomWarsQueryMsg::Proposal {
                        round_id,
                        tranche_id,
                        proposal_id,
                    } => {
                        let err = SystemResult::Err(SystemError::InvalidRequest {
                            error: "proposal couldn't be found".to_string(),
                            request: Binary(vec![]),
                        });

                        match &self.proposal {
                            Some(prop) => {
                                if prop.round_id == round_id
                                    && prop.tranche_id == tranche_id
                                    && prop.proposal_id == proposal_id
                                {
                                    to_json_binary(&prop)
                                } else {
                                    return err;
                                }
                            }
                            _ => return err,
                        }
                    }
                    AtomWarsQueryMsg::UserVote {
                        round_id,
                        tranche_id,
                        address,
                    } => {
                        let err = SystemResult::Err(SystemError::InvalidRequest {
                            error: "vote couldn't be found".to_string(),
                            request: Binary(vec![]),
                        });

                        match &self.user_vote {
                            Some(vote) => {
                                if vote.0 == round_id && vote.1 == tranche_id && vote.2 == address {
                                    to_json_binary(&vote.3)
                                } else {
                                    return err;
                                }
                            }
                            _ => return err,
                        }
                    }
                    AtomWarsQueryMsg::TopNProposals {
                        round_id: _,
                        tranche_id: _,
                        number_of_proposals: _,
                    } => to_json_binary(&self.top_n_proposals),
                    _ => panic!("unsupported query"),
                };

                SystemResult::Ok(ContractResult::Ok(response.unwrap()))
            }
            _ => SystemResult::Err(SystemError::UnsupportedRequest {
                kind: "unsupported query type".to_string(),
            }),
        }
    }
}

struct AddTributeTestCase {
    description: String,
    // (tranche_id, proposal_id)
    proposal_info: (u64, u64),
    tributes_to_add: Vec<Vec<Coin>>,
    // (current_round_id, proposal_to_tribute)
    mock_data: (u64, Option<Proposal>),
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
// (add_tribute_round_id, claim_tribute_round_id, proposal, user_vote, top_n_proposals)
type ClaimTributeMockData = (
    u64,
    u64,
    Option<Proposal>,
    Option<(u64, u64, String, Vote)>,
    Vec<Proposal>,
);

struct RefundTributeTestCase {
    description: String,
    // (round_id, tranche_id, proposal_id, tribute_id)
    tribute_info: (u64, u64, u64, u64),
    tribute_to_add: Vec<Coin>,
    // (add_tribute_round_id, refund_tribute_round_id, proposal, top_n_proposals)
    mock_data: (u64, u64, Option<Proposal>, Vec<Proposal>),
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
        covenant_params: CovenantParams {
            pool_id: "pool 1".to_string(),
            outgoing_channel_id: "channel-1".to_string(),
            funding_destination_name: "".to_string(),
        },
        executed: false,
        power: Uint128::new(10000),
        percentage: Uint128::zero(),
    };

    let test_cases: Vec<AddTributeTestCase> = vec![
        AddTributeTestCase {
            description: "happy path".to_string(),
            proposal_info: (0, 5),
            tributes_to_add: vec![
                vec![Coin::new(1000, DEFAULT_DENOM)],
                vec![Coin::new(5000, DEFAULT_DENOM)],
            ],
            mock_data: (10, Some(mock_proposal.clone())),
            expected_success: true,
            expected_error_msg: String::new(),
        },
        AddTributeTestCase {
            description: "try adding tribute for non-existing proposal".to_string(),
            proposal_info: (0, 5),
            tributes_to_add: vec![vec![Coin::new(1000, DEFAULT_DENOM)]],
            mock_data: (10, None),
            expected_success: false,
            expected_error_msg: "proposal couldn't be found".to_string(),
        },
        AddTributeTestCase {
            description: "try adding tribute without providing any funds".to_string(),
            proposal_info: (0, 5),
            tributes_to_add: vec![vec![]],
            mock_data: (10, Some(mock_proposal.clone())),
            expected_success: false,
            expected_error_msg: "Must send funds to add tribute".to_string(),
        },
        AddTributeTestCase {
            description: "try adding tribute by providing more than one token".to_string(),
            proposal_info: (0, 5),
            tributes_to_add: vec![vec![
                Coin::new(1000, DEFAULT_DENOM),
                Coin::new(1000, "stake"),
            ]],
            mock_data: (10, Some(mock_proposal.clone())),
            expected_success: false,
            expected_error_msg: "Must send exactly one coin".to_string(),
        },
    ];

    for test in test_cases {
        println!("running test case: {}", test.description);

        let (mut deps, env, info) = (
            mock_dependencies(),
            mock_env(),
            mock_info(USER_ADDRESS_1, &[]),
        );

        let mock_querier = MockWasmQuerier::new(
            ATOM_WARS_CONTRACT_ADDRESS.to_string(),
            test.mock_data.0,
            test.mock_data.1,
            None,
            vec![],
        );
        deps.querier.update_wasm(move |q| mock_querier.handler(q));

        let msg = get_instantiate_msg(ATOM_WARS_CONTRACT_ADDRESS.to_string());
        let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
        assert!(res.is_ok());

        let tribute_payer = USER_ADDRESS_1;

        for tribute in &test.tributes_to_add {
            let info = mock_info(tribute_payer, tribute);
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
            test.proposal_info.0,
            test.proposal_info.1,
        );
        assert_eq!(test.tributes_to_add.len(), res.len());

        for (i, tribute) in test.tributes_to_add.iter().enumerate() {
            assert_eq!(res[i].funds, tribute[0].clone());
            assert_eq!(res[i].depositor.to_string(), tribute_payer.to_string());
            assert!(!res[i].refunded);
        }
    }
}

#[test]
fn claim_tribute_test() {
    let mock_proposal = Proposal {
        round_id: 10,
        tranche_id: 0,
        proposal_id: 5,
        covenant_params: CovenantParams {
            pool_id: "pool 1".to_string(),
            outgoing_channel_id: "channel-1".to_string(),
            funding_destination_name: "".to_string(),
        },
        executed: false,
        power: Uint128::new(10000),
        percentage: Uint128::zero(),
    };

    let mock_top_n_proposals = vec![
        Proposal {
            round_id: 10,
            tranche_id: 0,
            proposal_id: 5,
            covenant_params: CovenantParams {
                pool_id: "pool 1".to_string(),
                outgoing_channel_id: "channel-1".to_string(),
                funding_destination_name: "".to_string(),
            },
            executed: false,
            power: Uint128::new(10000),
            percentage: Uint128::zero(),
        },
        Proposal {
            round_id: 10,
            tranche_id: 0,
            proposal_id: 6,
            covenant_params: CovenantParams {
                pool_id: "pool 2".to_string(),
                outgoing_channel_id: "channel-2".to_string(),
                funding_destination_name: "".to_string(),
            },
            executed: false,
            power: Uint128::new(10000),
            percentage: Uint128::zero(),
        },
    ];

    let test_cases: Vec<ClaimTributeTestCase> = vec![
        ClaimTributeTestCase {
            description: "happy path".to_string(),
            tribute_info: (10, 0, 5, 0),
            tribute_to_add: vec![Coin::new(1000, DEFAULT_DENOM)],
            mock_data: (
                10,
                11,
                Some(mock_proposal.clone()),
                Some((
                    10,
                    0,
                    USER_ADDRESS_2.to_string(),
                    Vote {
                        prop_id: 5,
                        power: Uint128::new(70),
                    },
                )),
                mock_top_n_proposals.clone(),
            ),
            expected_tribute_claim: 7, // (70 / 10_000) * 1_000
            expected_success: true,
            expected_error_msg: String::new(),
        },
        ClaimTributeTestCase {
            description: "try claim tribute for proposal in current round".to_string(),
            tribute_info: (10, 0, 5, 0),
            tribute_to_add: vec![Coin::new(1000, DEFAULT_DENOM)],
            mock_data: (10, 10, Some(mock_proposal.clone()), None, vec![]),
            expected_tribute_claim: 0,
            expected_success: false,
            expected_error_msg: "Round has not ended yet".to_string(),
        },
        ClaimTributeTestCase {
            description: "try claim tribute if user didn't vote at all".to_string(),
            tribute_info: (10, 0, 5, 0),
            tribute_to_add: vec![Coin::new(1000, DEFAULT_DENOM)],
            mock_data: (10, 11, Some(mock_proposal.clone()), None, vec![]),
            expected_tribute_claim: 0,
            expected_success: false,
            expected_error_msg: "vote couldn't be found".to_string(),
        },
        ClaimTributeTestCase {
            description: "try claim tribute if user didn't vote for top N proposal".to_string(),
            tribute_info: (10, 0, 5, 0),
            tribute_to_add: vec![Coin::new(1000, DEFAULT_DENOM)],
            mock_data: (
                10,
                11,
                Some(mock_proposal.clone()),
                Some((
                    10,
                    0,
                    USER_ADDRESS_2.to_string(),
                    Vote {
                        prop_id: 7,
                        power: Uint128::new(70),
                    },
                )),
                mock_top_n_proposals.clone(),
            ),
            expected_tribute_claim: 0,
            expected_success: false,
            expected_error_msg: "User voted for proposal outside of top N proposals".to_string(),
        },
        ClaimTributeTestCase {
            description: "try claim tribute for non existing tribute id".to_string(),
            tribute_info: (10, 0, 5, 1),
            tribute_to_add: vec![Coin::new(1000, DEFAULT_DENOM)],
            mock_data: (
                10,
                11,
                Some(mock_proposal.clone()),
                Some((
                    10,
                    0,
                    USER_ADDRESS_2.to_string(),
                    Vote {
                        prop_id: 5,
                        power: Uint128::new(70),
                    },
                )),
                mock_top_n_proposals.clone(),
            ),
            expected_tribute_claim: 0,
            expected_success: false,
            expected_error_msg: "Tribute not found".to_string(),
        },
    ];

    for test in test_cases {
        println!("running test case: {}", test.description);

        let (mut deps, env, info) = (
            mock_dependencies(),
            mock_env(),
            mock_info(USER_ADDRESS_1, &[]),
        );

        let mock_querier = MockWasmQuerier::new(
            ATOM_WARS_CONTRACT_ADDRESS.to_string(),
            test.mock_data.0,
            test.mock_data.2.clone(),
            None,
            vec![],
        );
        deps.querier.update_wasm(move |q| mock_querier.handler(q));

        let msg = get_instantiate_msg(ATOM_WARS_CONTRACT_ADDRESS.to_string());
        let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
        assert!(res.is_ok());

        let tribute_payer = USER_ADDRESS_1;
        let info = mock_info(tribute_payer, &test.tribute_to_add);
        let msg = ExecuteMsg::AddTribute {
            tranche_id: test.tribute_info.1,
            proposal_id: test.tribute_info.2,
        };

        let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
        assert!(res.is_ok());

        // Update the expected round so that the tribute can be claimed
        let mock_querier = MockWasmQuerier::new(
            ATOM_WARS_CONTRACT_ADDRESS.to_string(),
            test.mock_data.1,
            test.mock_data.2.clone(),
            test.mock_data.3.clone(),
            test.mock_data.4.clone(),
        );
        deps.querier.update_wasm(move |q| mock_querier.handler(q));

        let tribute_claimer = USER_ADDRESS_2;
        let info = mock_info(tribute_claimer, &[]);
        let msg = ExecuteMsg::ClaimTribute {
            round_id: test.tribute_info.0,
            tranche_id: test.tribute_info.1,
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
            &tribute_claimer.to_string(),
            &test.tribute_to_add[0].denom,
            test.expected_tribute_claim,
        );

        // Verify that the user can't claim the same tribute twice
        let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone());
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("Sender has already claimed the tribute"))
    }
}

#[test]
fn refund_tribute_test() {
    let mock_proposal = Proposal {
        round_id: 10,
        tranche_id: 0,
        proposal_id: 5,
        covenant_params: CovenantParams {
            pool_id: "pool 1".to_string(),
            outgoing_channel_id: "channel-1".to_string(),
            funding_destination_name: "".to_string(),
        },
        executed: false,
        power: Uint128::new(10000),
        percentage: Uint128::zero(),
    };

    let mock_top_n_proposals = vec![Proposal {
        round_id: 10,
        tranche_id: 0,
        proposal_id: 6,
        covenant_params: CovenantParams {
            pool_id: "pool 2".to_string(),
            outgoing_channel_id: "channel-2".to_string(),
            funding_destination_name: "".to_string(),
        },
        executed: false,
        power: Uint128::new(10000),
        percentage: Uint128::zero(),
    }];

    let test_cases: Vec<RefundTributeTestCase> = vec![
        RefundTributeTestCase {
            description: "happy path".to_string(),
            tribute_info: (10, 0, 5, 0),
            tribute_to_add: vec![Coin::new(1000, DEFAULT_DENOM)],
            mock_data: (
                10,
                11,
                Some(mock_proposal.clone()),
                mock_top_n_proposals.clone(),
            ),
            tribute_refunder: None,
            expected_tribute_refund: 1000,
            expected_success: true,
            expected_error_msg: String::new(),
        },
        RefundTributeTestCase {
            description: "try to get refund for the current round".to_string(),
            tribute_info: (10, 0, 5, 0),
            tribute_to_add: vec![Coin::new(1000, DEFAULT_DENOM)],
            mock_data: (
                10,
                10,
                Some(mock_proposal.clone()),
                mock_top_n_proposals.clone(),
            ),
            tribute_refunder: None,
            expected_tribute_refund: 0,
            expected_success: false,
            expected_error_msg: "Round has not ended yet".to_string(),
        },
        RefundTributeTestCase {
            description: "try to get refund for the top N proposal".to_string(),
            tribute_info: (10, 0, 5, 0),
            tribute_to_add: vec![Coin::new(1000, DEFAULT_DENOM)],
            mock_data: (
                10,
                11,
                Some(mock_proposal.clone()),
                vec![mock_proposal.clone()],
            ),
            tribute_refunder: None,
            expected_tribute_refund: 0,
            expected_success: false,
            expected_error_msg: "Can't refund top N proposal".to_string(),
        },
        RefundTributeTestCase {
            description: "try to get refund for non existing tribute".to_string(),
            tribute_info: (10, 0, 5, 1),
            tribute_to_add: vec![Coin::new(1000, DEFAULT_DENOM)],
            mock_data: (
                10,
                11,
                Some(mock_proposal.clone()),
                mock_top_n_proposals.clone(),
            ),
            tribute_refunder: None,
            expected_tribute_refund: 0,
            expected_success: false,
            expected_error_msg: "Tribute not found".to_string(),
        },
        RefundTributeTestCase {
            description: "try to get refund if not the depositor".to_string(),
            tribute_info: (10, 0, 5, 0),
            tribute_to_add: vec![Coin::new(1000, DEFAULT_DENOM)],
            mock_data: (
                10,
                11,
                Some(mock_proposal.clone()),
                mock_top_n_proposals.clone(),
            ),
            tribute_refunder: Some(USER_ADDRESS_2.to_string()),
            expected_tribute_refund: 0,
            expected_success: false,
            expected_error_msg: "Sender is not the depositor of the tribute".to_string(),
        },
    ];

    for test in test_cases {
        println!("running test case: {}", test.description);

        let (mut deps, env, info) = (
            mock_dependencies(),
            mock_env(),
            mock_info(USER_ADDRESS_1, &[]),
        );

        let mock_querier = MockWasmQuerier::new(
            ATOM_WARS_CONTRACT_ADDRESS.to_string(),
            test.mock_data.0,
            test.mock_data.2.clone(),
            None,
            vec![],
        );
        deps.querier.update_wasm(move |q| mock_querier.handler(q));

        let msg = get_instantiate_msg(ATOM_WARS_CONTRACT_ADDRESS.to_string());
        let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
        assert!(res.is_ok());

        let tribute_payer = USER_ADDRESS_1;
        let info = mock_info(tribute_payer, &test.tribute_to_add);
        let msg = ExecuteMsg::AddTribute {
            tranche_id: test.tribute_info.1,
            proposal_id: test.tribute_info.2,
        };

        let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
        assert!(res.is_ok());

        // Update the expected round so that the tribute can be refunded
        let mock_querier = MockWasmQuerier::new(
            ATOM_WARS_CONTRACT_ADDRESS.to_string(),
            test.mock_data.1,
            test.mock_data.2.clone(),
            None,
            test.mock_data.3.clone(),
        );
        deps.querier.update_wasm(move |q| mock_querier.handler(q));

        // If specified, try to use a different tribute refunder
        let tribute_refunder = match test.tribute_refunder {
            Some(refunder) => refunder,
            None => tribute_payer.to_string(),
        };

        let info = mock_info(&tribute_refunder, &[]);
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
            &tribute_refunder,
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
