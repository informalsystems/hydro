use std::str::FromStr;

use crate::contract::exchange_rate_to_decimal;
use crate::contract::{execute, instantiate, query};
use crate::msg::{ExecuteMsg, InstantiateMsg};
use crate::query::{HTokenQueryMsg, HTokenRateResponse, QueryMsg};
use cosmwasm_std::{
    testing::{mock_dependencies, mock_env, MockApi},
    to_json_binary, Coin, ContractResult, Decimal, Decimal256, MessageInfo, QuerierResult, SubMsg,
    SystemError, SystemResult, Timestamp, Uint128, Uint256, WasmMsg, WasmQuery,
};
use interface::hydro::{
    CurrentRoundResponse as HydroCurrentRoundResponse, ExecuteMsg as HydroExecuteMsg,
    QueryMsg as HydroQueryMsg, TokenGroupRatioChange,
};
use interface::token_info_provider::DenomInfoResponse;

const H_TOKEN_DENOM: &str = "ibc/htoken";
const TOKEN_GROUP_ID: &str = "hTOKEN";
const HYDRO_CONTRACT: &str = "hydro";
const H_TOKEN_CONTRACT: &str = "h_token";

pub type WasmQueryFunc = dyn Fn(&WasmQuery) -> QuerierResult;

pub fn get_message_info(api: &MockApi, sender: &str, funds: &[Coin]) -> MessageInfo {
    MessageInfo {
        sender: api.addr_make(sender),
        funds: funds.to_vec(),
    }
}

pub fn external_contracts_mock(current_round: u64, current_ratio: Decimal) -> Box<WasmQueryFunc> {
    Box::new(move |req| match req {
        WasmQuery::Smart {
            contract_addr: _,
            msg,
        } => {
            if let Ok(HydroQueryMsg::CurrentRound {}) = cosmwasm_std::from_json(msg) {
                return SystemResult::Ok(ContractResult::Ok(
                    to_json_binary(&HydroCurrentRoundResponse {
                        round_id: current_round,
                        round_end: Timestamp::from_nanos(0),
                    })
                    .unwrap(),
                ));
            }
            if let Ok(HTokenQueryMsg::Rate {}) = cosmwasm_std::from_json(msg) {
                return SystemResult::Ok(ContractResult::Ok(
                    to_json_binary(&HTokenRateResponse {
                        exchange_rate: Decimal256::from(current_ratio),
                        holders_value: Uint256::zero(),
                        supply: Uint128::zero(),
                    })
                    .unwrap(),
                ));
            }

            SystemResult::Err(SystemError::Unknown {})
        }
        _ => SystemResult::Err(SystemError::Unknown {}),
    })
}

#[test]
fn denom_info_query_and_update_test() {
    let user = "addr0000";
    let (env, mut deps) = (mock_env(), mock_dependencies());

    let instantiate_msg = InstantiateMsg {
        h_token_denom: H_TOKEN_DENOM.to_string(),
        token_group_id: TOKEN_GROUP_ID.to_string(),
        h_token_contract: deps.api.addr_make(H_TOKEN_CONTRACT).to_string(),
    };

    let info_hydro = get_message_info(&deps.api, HYDRO_CONTRACT, &[]);
    let info_user = get_message_info(&deps.api, user, &[]);

    let res = instantiate(
        deps.as_mut(),
        mock_env(),
        info_hydro.clone(),
        instantiate_msg,
    );
    assert!(res.is_ok());

    let round_0_token_ratio = Decimal::from_str("1.15").unwrap();
    deps.querier
        .update_wasm(external_contracts_mock(0, round_0_token_ratio));

    // Query DenomInfo for current and future rounds - should return 0
    let query_msg = QueryMsg::DenomInfo { round_id: 0 };
    let res = query(deps.as_ref(), env.clone(), query_msg.clone()).unwrap();
    let denom_info: DenomInfoResponse = cosmwasm_std::from_json(&res).unwrap();

    assert_eq!(denom_info.denom, H_TOKEN_DENOM);
    assert_eq!(denom_info.token_group_id, TOKEN_GROUP_ID);
    assert_eq!(denom_info.ratio, Decimal::zero());

    let query_msg_future = QueryMsg::DenomInfo { round_id: 1 };
    let res = query(deps.as_ref(), env.clone(), query_msg_future.clone()).unwrap();
    let denom_info: DenomInfoResponse = cosmwasm_std::from_json(&res).unwrap();
    assert_eq!(denom_info.ratio, Decimal::zero());

    // Execute UpdateTokenRatio to set the h token ratio for the current (0) round
    let exec_msg = ExecuteMsg::UpdateTokenRatio {};
    let res = execute(deps.as_mut(), env.clone(), info_user.clone(), exec_msg);
    assert!(res.is_ok());

    // Verify that the response contains a SubMsg to update the token group ratio on the main Hydro contract
    let response = res.unwrap();
    assert_eq!(
        response.messages,
        vec![SubMsg::new(WasmMsg::Execute {
            contract_addr: info_hydro.sender.to_string(),
            msg: to_json_binary(&HydroExecuteMsg::UpdateTokenGroupsRatios {
                changes: vec![TokenGroupRatioChange {
                    token_group_id: TOKEN_GROUP_ID.to_owned(),
                    old_ratio: Decimal::zero(),
                    new_ratio: round_0_token_ratio,
                }],
            })
            .unwrap(),
            funds: vec![]
        })]
    );

    // Query DenomInfo for current round - should return updated ratio
    let res = query(deps.as_ref(), env.clone(), query_msg.clone()).unwrap();
    let denom_info: DenomInfoResponse = cosmwasm_std::from_json(&res).unwrap();
    assert_eq!(denom_info.ratio, round_0_token_ratio);

    // Query DenomInfo for future round - should return ratio from round 0
    let res = query(deps.as_ref(), env.clone(), query_msg_future).unwrap();
    let denom_info: DenomInfoResponse = cosmwasm_std::from_json(&res).unwrap();
    assert_eq!(denom_info.ratio, round_0_token_ratio);

    // Move into round with ID 5
    let round_5_token_ratio_1 = Decimal::from_str("1.3").unwrap();
    deps.querier
        .update_wasm(external_contracts_mock(5, round_5_token_ratio_1));

    // Execute UpdateTokenRatio to set the h token ratio for the current (5) round
    let exec_msg = ExecuteMsg::UpdateTokenRatio {};
    let res = execute(deps.as_mut(), env.clone(), info_user.clone(), exec_msg);
    assert!(res.is_ok());

    // Verify that the ratio for all past rounds is copied from round 0
    for round_id in 0..=4 {
        let query_msg = QueryMsg::DenomInfo { round_id };
        let res = query(deps.as_ref(), env.clone(), query_msg).unwrap();
        let denom_info: DenomInfoResponse = cosmwasm_std::from_json(&res).unwrap();

        assert_eq!(denom_info.ratio, round_0_token_ratio);
    }

    let query_msg = QueryMsg::DenomInfo { round_id: 5 };
    let res = query(deps.as_ref(), env.clone(), query_msg).unwrap();
    let denom_info: DenomInfoResponse = cosmwasm_std::from_json(&res).unwrap();

    assert_eq!(denom_info.ratio, round_5_token_ratio_1);

    // Execute UpdateTokenRatio to set new value for the same round (5)
    let round_5_token_ratio_2 = Decimal::from_str("1.31").unwrap();
    deps.querier
        .update_wasm(external_contracts_mock(5, round_5_token_ratio_2));

    let exec_msg = ExecuteMsg::UpdateTokenRatio {};
    let res = execute(deps.as_mut(), env.clone(), info_user.clone(), exec_msg);
    assert!(res.is_ok());

    let response = res.unwrap();
    assert_eq!(
        response.messages,
        vec![SubMsg::new(WasmMsg::Execute {
            contract_addr: info_hydro.sender.to_string(),
            msg: to_json_binary(&HydroExecuteMsg::UpdateTokenGroupsRatios {
                changes: vec![TokenGroupRatioChange {
                    token_group_id: TOKEN_GROUP_ID.to_owned(),
                    old_ratio: round_5_token_ratio_1,
                    new_ratio: round_5_token_ratio_2,
                }]
            })
            .unwrap(),
            funds: vec![]
        })]
    );

    let query_msg = QueryMsg::DenomInfo { round_id: 5 };
    let res = query(deps.as_ref(), env.clone(), query_msg).unwrap();
    let denom_info: DenomInfoResponse = cosmwasm_std::from_json(&res).unwrap();

    assert_eq!(denom_info.ratio, round_5_token_ratio_2);
}

#[test]
fn exchange_rate_to_decimal_test() {
    let rate = Decimal256::from_str("1.234567890123456789").unwrap();
    assert_eq!(
        exchange_rate_to_decimal(rate).unwrap(),
        Decimal::from_str("1.234567890123456789").unwrap()
    );

    let too_large = Decimal256::from(Decimal::MAX) + Decimal256::one();
    let err = exchange_rate_to_decimal(too_large).unwrap_err();
    assert!(err.to_string().contains("does not fit into Decimal"));
}

#[test]
fn update_token_ratio_fails_when_exchange_rate_overflows_decimal() {
    let (env, mut deps) = (mock_env(), mock_dependencies());

    let instantiate_msg = InstantiateMsg {
        h_token_denom: H_TOKEN_DENOM.to_string(),
        token_group_id: TOKEN_GROUP_ID.to_string(),
        h_token_contract: deps.api.addr_make(H_TOKEN_CONTRACT).to_string(),
    };
    let info_hydro = get_message_info(&deps.api, HYDRO_CONTRACT, &[]);
    instantiate(deps.as_mut(), env.clone(), info_hydro, instantiate_msg).unwrap();

    let too_large = Decimal256::from(Decimal::MAX) + Decimal256::one();
    deps.querier.update_wasm(move |req| match req {
        WasmQuery::Smart { msg, .. } => {
            if let Ok(HydroQueryMsg::CurrentRound {}) = cosmwasm_std::from_json(msg) {
                return SystemResult::Ok(ContractResult::Ok(
                    to_json_binary(&HydroCurrentRoundResponse {
                        round_id: 0,
                        round_end: Timestamp::from_nanos(0),
                    })
                    .unwrap(),
                ));
            }
            SystemResult::Ok(ContractResult::Ok(
                to_json_binary(&HTokenRateResponse {
                    exchange_rate: too_large,
                    holders_value: Uint256::zero(),
                    supply: Uint128::zero(),
                })
                .unwrap(),
            ))
        }
        _ => SystemResult::Err(SystemError::Unknown {}),
    });

    let info_user = get_message_info(&deps.api, "addr0000", &[]);
    let err = execute(
        deps.as_mut(),
        env,
        info_user,
        ExecuteMsg::UpdateTokenRatio {},
    )
    .unwrap_err();
    assert!(err.to_string().contains("does not fit into Decimal"));
}
