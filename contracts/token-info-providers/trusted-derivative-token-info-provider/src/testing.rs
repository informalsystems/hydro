use std::str::FromStr;

use crate::contract::{execute, instantiate, query};
use crate::msg::{ExecuteMsg, InstantiateMsg};
use crate::query::{QueryMsg, WhitelistResponse};
use cosmwasm_std::{
    testing::{mock_dependencies, mock_env, MockApi},
    to_json_binary, Coin, ContractResult, Decimal, MessageInfo, QuerierResult, SubMsg, SystemError,
    SystemResult, Timestamp, WasmMsg, WasmQuery,
};
use interface::hydro::{
    CurrentRoundResponse as HydroCurrentRoundResponse, ExecuteMsg as HydroExecuteMsg,
    QueryMsg as HydroQueryMsg, TokenGroupRatioChange,
};
use interface::token_info_provider::DenomInfoResponse;

const DERIVATIVE_TOKEN_DENOM: &str =
    "ibc/B7864B03E54B0E4D283072AA25AF3C850A9B394AAAF0CFC4B10F9F1F5AAA00B";
const TOKEN_GROUP_ID: &str = "stATOM";
const HYDRO_CONTRACT: &str = "hydro";

type WasmQueryFunc = dyn Fn(&WasmQuery) -> QuerierResult;

fn get_message_info(api: &MockApi, sender: &str, funds: &[Coin]) -> MessageInfo {
    MessageInfo {
        sender: api.addr_make(sender),
        funds: funds.to_vec(),
    }
}

fn hydro_mock(current_round: u64) -> Box<WasmQueryFunc> {
    Box::new(move |req| match req {
        WasmQuery::Smart { msg, .. } => {
            if let Ok(HydroQueryMsg::CurrentRound {}) = cosmwasm_std::from_json(msg) {
                return SystemResult::Ok(ContractResult::Ok(
                    to_json_binary(&HydroCurrentRoundResponse {
                        round_id: current_round,
                        round_end: Timestamp::from_nanos(0),
                    })
                    .unwrap(),
                ));
            }
            SystemResult::Err(SystemError::Unknown {})
        }
        _ => SystemResult::Err(SystemError::Unknown {}),
    })
}

fn default_instantiate_msg(api: &MockApi, whitelisted: &[&str]) -> InstantiateMsg {
    InstantiateMsg {
        hydro_contract_address: None,
        derivative_token_denom: DERIVATIVE_TOKEN_DENOM.to_string(),
        token_group_id: TOKEN_GROUP_ID.to_string(),
        initial_whitelist: whitelisted
            .iter()
            .map(|s| api.addr_make(s).to_string())
            .collect(),
    }
}

#[test]
fn test_instantiate_sender_as_hydro() {
    let (env, mut deps) = (mock_env(), mock_dependencies());
    let info_hydro = get_message_info(&deps.api, HYDRO_CONTRACT, &[]);

    let msg = default_instantiate_msg(&deps.api, &["alice"]);
    let res = instantiate(deps.as_mut(), env, info_hydro.clone(), msg);
    assert!(res.is_ok());

    let res = query(deps.as_ref(), mock_env(), QueryMsg::Config {}).unwrap();
    let config_resp: crate::query::ConfigResponse = cosmwasm_std::from_json(&res).unwrap();
    assert_eq!(config_resp.config.hydro_contract_address, info_hydro.sender);
}

#[test]
fn test_instantiate_explicit_hydro_address() {
    let (env, mut deps) = (mock_env(), mock_dependencies());
    let hydro_addr = deps.api.addr_make(HYDRO_CONTRACT).to_string();
    let alice_addr = deps.api.addr_make("alice").to_string();

    let msg = InstantiateMsg {
        hydro_contract_address: Some(hydro_addr.clone()),
        derivative_token_denom: DERIVATIVE_TOKEN_DENOM.to_string(),
        token_group_id: TOKEN_GROUP_ID.to_string(),
        initial_whitelist: vec![alice_addr],
    };

    let info = get_message_info(&deps.api, "someone_else", &[]);
    let res = instantiate(deps.as_mut(), env, info, msg);
    assert!(res.is_ok());

    let res = query(deps.as_ref(), mock_env(), QueryMsg::Config {}).unwrap();
    let config_resp: crate::query::ConfigResponse = cosmwasm_std::from_json(&res).unwrap();
    assert_eq!(
        config_resp.config.hydro_contract_address.to_string(),
        hydro_addr
    );
}

#[test]
fn test_instantiate_empty_whitelist_fails() {
    let (env, mut deps) = (mock_env(), mock_dependencies());
    let info = get_message_info(&deps.api, HYDRO_CONTRACT, &[]);
    let msg = default_instantiate_msg(&deps.api, &[]);
    let res = instantiate(deps.as_mut(), env, info, msg);
    assert!(res.is_err());
}

#[test]
fn test_submit_token_ratio_and_denom_info() {
    let (env, mut deps) = (mock_env(), mock_dependencies());
    let info_hydro = get_message_info(&deps.api, HYDRO_CONTRACT, &[]);
    let info_alice = get_message_info(&deps.api, "alice", &[]);

    let msg = default_instantiate_msg(&deps.api, &["alice"]);
    instantiate(deps.as_mut(), env.clone(), info_hydro.clone(), msg).unwrap();

    // Before any update, round 0 ratio is zero
    let res = query(
        deps.as_ref(),
        env.clone(),
        QueryMsg::DenomInfo { round_id: 0 },
    )
    .unwrap();
    let denom_info: DenomInfoResponse = cosmwasm_std::from_json(&res).unwrap();
    assert_eq!(denom_info.denom, DERIVATIVE_TOKEN_DENOM);
    assert_eq!(denom_info.token_group_id, TOKEN_GROUP_ID);
    assert_eq!(denom_info.ratio, Decimal::zero());

    // Future round carries forward zero
    let res = query(
        deps.as_ref(),
        env.clone(),
        QueryMsg::DenomInfo { round_id: 3 },
    )
    .unwrap();
    let denom_info: DenomInfoResponse = cosmwasm_std::from_json(&res).unwrap();
    assert_eq!(denom_info.ratio, Decimal::zero());

    // Submit ratio for round 0
    let ratio_round_0 = Decimal::from_str("1.15").unwrap();
    deps.querier.update_wasm(hydro_mock(0));

    let res = execute(
        deps.as_mut(),
        env.clone(),
        info_alice.clone(),
        ExecuteMsg::SubmitTokenRatio {
            ratio: ratio_round_0,
        },
    );
    assert!(res.is_ok());

    // Verify UpdateTokenGroupsRatios was sent to Hydro
    let response = res.unwrap();
    assert_eq!(
        response.messages,
        vec![SubMsg::new(WasmMsg::Execute {
            contract_addr: info_hydro.sender.to_string(),
            msg: to_json_binary(&HydroExecuteMsg::UpdateTokenGroupsRatios {
                changes: vec![TokenGroupRatioChange {
                    token_group_id: TOKEN_GROUP_ID.to_owned(),
                    old_ratio: Decimal::zero(),
                    new_ratio: ratio_round_0,
                }],
            })
            .unwrap(),
            funds: vec![],
        })]
    );

    // Query round 0 and future rounds
    let res = query(
        deps.as_ref(),
        env.clone(),
        QueryMsg::DenomInfo { round_id: 0 },
    )
    .unwrap();
    let denom_info: DenomInfoResponse = cosmwasm_std::from_json(&res).unwrap();
    assert_eq!(denom_info.ratio, ratio_round_0);

    let res = query(
        deps.as_ref(),
        env.clone(),
        QueryMsg::DenomInfo { round_id: 5 },
    )
    .unwrap();
    let denom_info: DenomInfoResponse = cosmwasm_std::from_json(&res).unwrap();
    assert_eq!(denom_info.ratio, ratio_round_0);
}

#[test]
fn test_submit_token_ratio_carry_forward_across_rounds() {
    let (env, mut deps) = (mock_env(), mock_dependencies());
    let info_hydro = get_message_info(&deps.api, HYDRO_CONTRACT, &[]);
    let info_alice = get_message_info(&deps.api, "alice", &[]);

    let msg = default_instantiate_msg(&deps.api, &["alice"]);
    instantiate(deps.as_mut(), env.clone(), info_hydro, msg).unwrap();

    // Submit ratio for round 0
    let ratio_round_0 = Decimal::from_str("1.15").unwrap();
    deps.querier.update_wasm(hydro_mock(0));
    execute(
        deps.as_mut(),
        env.clone(),
        info_alice.clone(),
        ExecuteMsg::SubmitTokenRatio {
            ratio: ratio_round_0,
        },
    )
    .unwrap();

    // Jump to round 5 and submit a new ratio
    let ratio_round_5 = Decimal::from_str("1.30").unwrap();
    deps.querier.update_wasm(hydro_mock(5));
    execute(
        deps.as_mut(),
        env.clone(),
        info_alice.clone(),
        ExecuteMsg::SubmitTokenRatio {
            ratio: ratio_round_5,
        },
    )
    .unwrap();

    // Rounds 1–4 should carry forward round 0's ratio
    for round_id in 1..=4 {
        let res = query(deps.as_ref(), env.clone(), QueryMsg::DenomInfo { round_id }).unwrap();
        let denom_info: DenomInfoResponse = cosmwasm_std::from_json(&res).unwrap();
        assert_eq!(denom_info.ratio, ratio_round_0, "round {round_id}");
    }

    let res = query(
        deps.as_ref(),
        env.clone(),
        QueryMsg::DenomInfo { round_id: 5 },
    )
    .unwrap();
    let denom_info: DenomInfoResponse = cosmwasm_std::from_json(&res).unwrap();
    assert_eq!(denom_info.ratio, ratio_round_5);

    // Submitting the same ratio again produces no Hydro message
    deps.querier.update_wasm(hydro_mock(5));
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info_alice.clone(),
        ExecuteMsg::SubmitTokenRatio {
            ratio: ratio_round_5,
        },
    )
    .unwrap();
    assert!(res.messages.is_empty());
}

#[test]
fn test_submit_token_ratio_unauthorized() {
    let (env, mut deps) = (mock_env(), mock_dependencies());
    let info_hydro = get_message_info(&deps.api, HYDRO_CONTRACT, &[]);
    let info_bob = get_message_info(&deps.api, "bob", &[]);

    let msg = default_instantiate_msg(&deps.api, &["alice"]);
    instantiate(deps.as_mut(), env.clone(), info_hydro, msg).unwrap();

    deps.querier.update_wasm(hydro_mock(0));

    let res = execute(
        deps.as_mut(),
        env,
        info_bob,
        ExecuteMsg::SubmitTokenRatio {
            ratio: Decimal::from_str("1.2").unwrap(),
        },
    );
    assert!(matches!(
        res.unwrap_err(),
        crate::error::ContractError::Unauthorized {}
    ));
}

#[test]
fn test_whitelist_management() {
    let (env, mut deps) = (mock_env(), mock_dependencies());
    let info_hydro = get_message_info(&deps.api, HYDRO_CONTRACT, &[]);
    let info_alice = get_message_info(&deps.api, "alice", &[]);
    let info_bob = get_message_info(&deps.api, "bob", &[]);

    let msg = default_instantiate_msg(&deps.api, &["alice"]);
    instantiate(deps.as_mut(), env.clone(), info_hydro, msg).unwrap();

    // Non-whitelisted bob cannot add addresses
    let charlie_addr = deps.api.addr_make("charlie").to_string();
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info_bob.clone(),
        ExecuteMsg::AddToWhitelist {
            address: charlie_addr,
        },
    );
    assert!(matches!(
        res.unwrap_err(),
        crate::error::ContractError::Unauthorized {}
    ));

    // Alice adds bob
    let bob_addr = deps.api.addr_make("bob").to_string();
    execute(
        deps.as_mut(),
        env.clone(),
        info_alice.clone(),
        ExecuteMsg::AddToWhitelist { address: bob_addr },
    )
    .unwrap();

    // Whitelist query should return both
    let res = query(deps.as_ref(), env.clone(), QueryMsg::Whitelist {}).unwrap();
    let wl: WhitelistResponse = cosmwasm_std::from_json(&res).unwrap();
    assert_eq!(wl.addresses.len(), 2);

    // Alice removes herself; bob remains
    let alice_addr = deps.api.addr_make("alice").to_string();
    execute(
        deps.as_mut(),
        env.clone(),
        info_alice.clone(),
        ExecuteMsg::RemoveFromWhitelist {
            address: alice_addr.clone(),
        },
    )
    .unwrap();

    // Now only bob is in the whitelist; bob cannot remove himself (would empty the list)
    let bob_addr = deps.api.addr_make("bob").to_string();
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info_bob.clone(),
        ExecuteMsg::RemoveFromWhitelist {
            address: bob_addr.clone(),
        },
    );
    assert!(matches!(
        res.unwrap_err(),
        crate::error::ContractError::WhitelistEmpty {}
    ));

    // Removed alice cannot remove bob
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info_alice.clone(),
        ExecuteMsg::RemoveFromWhitelist { address: bob_addr },
    );
    assert!(matches!(
        res.unwrap_err(),
        crate::error::ContractError::Unauthorized {}
    ));
}
