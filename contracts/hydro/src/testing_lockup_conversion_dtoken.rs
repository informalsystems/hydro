use std::{collections::HashMap, str::FromStr};

use cosmwasm_std::{
    testing::{mock_env, MockApi, MockStorage},
    to_json_binary, Addr, Coin, Decimal, Event, OwnedDeps, Reply, SubMsgResponse, SubMsgResult,
    Uint128,
};
use interface::token_info_provider::DenomInfoResponse;
use neutron_sdk::bindings::query::NeutronQuery;

use crate::{
    contract::{compute_current_round_id, convert_lockup_to_dtoken, execute, instantiate, reply},
    msg::{
        ConvertLockupPayload, ExecuteMsg, ProposalToLockups, ReplyPayload,
        TokenInfoProviderInstantiateMsg,
    },
    score_keeper::get_total_power_for_proposal,
    state::{
        DropTokenInfo, LockEntryV2, DROP_TOKEN_INFO, LOCKS_MAP_V2, LOCKS_PENDING_SLASHES,
        TOKEN_INFO_PROVIDERS, USER_LOCKS,
    },
    testing::{
        get_default_instantiate_msg, get_message_info, set_default_validator_for_rounds,
        IBC_DENOM_1, ONE_MONTH_IN_NANO_SECONDS, VALIDATOR_1, VALIDATOR_1_LST_DENOM_1, VALIDATOR_2,
    },
    testing_lsm_integration::set_validator_infos_for_round,
    testing_mocks::{
        denom_trace_grpc_query_mock, mock_dependencies, token_info_provider_derivative_mock,
        MockQuerier, MockWasmQuerier,
    },
    token_manager::{TokenInfoProvider, TokenInfoProviderDerivative},
    utils::load_current_constants,
};
pub const DROP_D_TOKEN_DENOM: &str =
    "factory/neutron1k6hr0f83e7un2wjf29cspk7j69jrnskk65k3ek2nj9dztrlzpj6q00rtsa/udatom";

pub const D_ATOM_TOKEN_GROUP: &str = "datom";

#[test]
fn convert_lockup_to_dtoken_test() {
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-1".to_string(),
        HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
    );
    let (mut deps, mut env) = (mock_dependencies(grpc_query), mock_env());
    let user_address = deps.api.addr_make("addr0000");
    let info = get_message_info(&deps.api, "addr0000", &[]);

    let mut instantiate_msg: crate::msg::InstantiateMsg = get_default_instantiate_msg(&deps.api);
    instantiate_msg.token_info_providers[0] = TokenInfoProviderInstantiateMsg::LSM {
        max_validator_shares_participating: 100,
        hub_connection_id: "connection-0".to_string(),
        hub_transfer_channel_id: "channel-1".to_string(),
        icq_update_period: 100,
    };
    let lock_epoch_length = instantiate_msg.lock_epoch_length;

    let res = instantiate(deps.as_mut(), env.clone(), info, instantiate_msg.clone());
    assert!(res.is_ok());

    // simulate user locking 1000 tokens for 1 month, one day after the round started
    env.block.time = env.block.time.plus_days(1);

    set_default_validator_for_rounds(deps.as_mut(), 0, 100);

    let first_lockup_amount: u128 = 1000;
    let info = get_message_info(
        &deps.api,
        "addr0000",
        &[Coin::new(first_lockup_amount, IBC_DENOM_1.to_string())],
    );
    let msg = ExecuteMsg::LockTokens {
        lock_duration: lock_epoch_length,
        proof: None,
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok());

    let ids: Vec<u64> = vec![1, 2];

    USER_LOCKS
        .save(
            &mut deps.storage,
            user_address.clone(),
            &ids,
            env.block.height,
        )
        .unwrap();

    // simulate user locking 2000 tokens for 3 months, two days after the round started
    env.block.time = env.block.time.plus_days(1);

    let result = set_validator_infos_for_round(
        &mut deps.storage,
        0,
        vec![VALIDATOR_1.to_string(), VALIDATOR_2.to_string()],
    );
    assert!(result.is_ok());

    let first_lockup_amount: u128 = 1000;
    let info = get_message_info(
        &deps.api,
        "addr0000",
        &[Coin::new(first_lockup_amount, IBC_DENOM_1.to_string())],
    );
    let msg = ExecuteMsg::LockTokens {
        lock_duration: 3 * ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok());

    let second_lockup_amount: u128 = 2000;
    let info = get_message_info(
        &deps.api,
        "addr0000",
        &[Coin::new(second_lockup_amount, IBC_DENOM_1.to_string())],
    );
    let msg = ExecuteMsg::LockTokens {
        lock_duration: 3 * ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok());

    let drop_address = deps.api.addr_make("drop");
    let puppeteer_address = deps.api.addr_make("puppeteer");
    let d_token_denom =
        "factory/neutron1k6hr0f83e7un2wjf29cspk7j69jrnskk65k3ek2nj9dztrlzpj6q00rtsa/udatom"
            .to_string();

    let drop_token_info = DropTokenInfo {
        address: drop_address,
        d_token_denom,
        puppeteer_address,
    };

    let drop_contract = DROP_TOKEN_INFO.save(&mut deps.storage, &drop_token_info);
    assert!(drop_contract.is_ok(), "failed to save drop contract info");

    let lock_duration = 3 * ONE_MONTH_IN_NANO_SECONDS;

    let res_lock = LOCKS_MAP_V2.save(
        &mut deps.storage,
        1,
        &LockEntryV2 {
            lock_id: 1,
            funds: Coin::new(Uint128::from(1000u128), IBC_DENOM_1.to_string()),
            owner: user_address.clone(),
            lock_start: env.block.time,
            lock_end: env.block.time.plus_nanos(lock_duration),
        },
        env.block.height,
    );
    assert!(res_lock.is_ok(), "failed to save lock");

    let res_lock_2 = LOCKS_MAP_V2.save(
        &mut deps.storage,
        2,
        &LockEntryV2 {
            lock_id: 2,
            funds: Coin::new(Uint128::from(2000u128), IBC_DENOM_1.to_string()),
            owner: user_address.clone(),
            lock_start: env.block.time,
            lock_end: env.block.time.plus_nanos(lock_duration),
        },
        env.block.height,
    );
    assert!(res_lock_2.is_ok(), "failed to save lock");

    let constants = load_current_constants(&deps.as_ref(), &env).unwrap();
    let current_round_id = compute_current_round_id(&env, &constants).unwrap();

    let proposal_msgs = vec![
        ExecuteMsg::CreateProposal {
            round_id: Some(current_round_id),
            tranche_id: 1,
            title: "proposal title 1".to_string(),
            description: "proposal description 1".to_string(),
            minimum_atom_liquidity_request: Uint128::zero(),
            deployment_duration: 1,
        },
        ExecuteMsg::CreateProposal {
            round_id: Some(current_round_id),
            tranche_id: 1,
            title: "proposal title 2".to_string(),
            description: "proposal description 2".to_string(),
            minimum_atom_liquidity_request: Uint128::zero(),
            deployment_duration: 1,
        },
    ];

    let info_prop = get_message_info(&deps.api, "addr0000", &[]);

    for proposal_msg in proposal_msgs {
        let res = execute(deps.as_mut(), env.clone(), info_prop.clone(), proposal_msg);
        assert!(res.is_ok());
    }

    let votes = vec![ProposalToLockups {
        proposal_id: 1,
        lock_ids: vec![1],
    }];

    let vote_msg = ExecuteMsg::Vote {
        tranche_id: 1,
        proposals_votes: votes.clone(),
    };
    let res = execute(deps.as_mut(), env.clone(), info_prop.clone(), vote_msg);
    assert!(res.is_ok());

    let info = get_message_info(&deps.api, "addr0000", &[]);

    let res = convert_lockup_to_dtoken(deps.as_mut(), env, info, vec![1, 2]).unwrap();
    assert_eq!(res.messages.len(), 2);
    assert_eq!(res.attributes[0].value, "convert_lockup_to_dtoken");

    let contract_address = deps.api.addr_make("dtoken_info_provider");
    let current_ratio = Decimal::from_str("1.15").unwrap();
    setup_d_atom_token_info_provider_mock(&mut deps, contract_address.clone(), current_ratio);

    let payload = ConvertLockupPayload {
        lock_id: 1,
        amount: Uint128::new(1000),
        sender: user_address.clone(),
    };

    let reply_payload = ReplyPayload::ConvertLockup(payload);
    let serialized_payload = to_json_binary(&reply_payload).unwrap();

    // simulate replies
    let reply_1 = Reply {
        id: 1,
        payload: serialized_payload,
        gas_used: 0,
        #[allow(deprecated)]
        result: SubMsgResult::Ok(SubMsgResponse {
            events: vec![Event::new("wasm").add_attribute("issue_amount", "1000")],
            data: None,
            msg_responses: vec![],
        }),
    };

    let power_before = get_total_power_for_proposal(&deps.storage, 1).unwrap();
    let atomics = Uint128::from_str("1500000000000000000000").unwrap();
    let before = Decimal::from_atomics(atomics, 18).unwrap();
    assert_eq!(power_before, before);

    let env = mock_env();
    let reply_response_1 = reply(deps.as_mut(), env.clone(), reply_1).unwrap();
    assert_eq!(
        reply_response_1.attributes[0].value,
        "convert_lockup_success"
    );

    let updated_lock_1 = LOCKS_MAP_V2.load(&deps.storage, 1).unwrap();
    assert_eq!(updated_lock_1.funds.denom, drop_token_info.d_token_denom);
    assert_eq!(updated_lock_1.funds.amount, Uint128::new(1000));

    let power_after = get_total_power_for_proposal(&deps.storage, 1).unwrap();
    let atomics = Uint128::from_str("1725000000000000000000").unwrap();
    let after = Decimal::from_atomics(atomics, 18).unwrap();
    assert_eq!(power_after, after);

    let payload = ConvertLockupPayload {
        lock_id: 2,
        amount: Uint128::new(2000),
        sender: user_address,
    };

    let reply_payload = ReplyPayload::ConvertLockup(payload);
    let serialized_payload = to_json_binary(&reply_payload).unwrap();

    let reply_2 = Reply {
        id: 2,
        payload: serialized_payload,
        gas_used: 0,
        #[allow(deprecated)]
        result: SubMsgResult::Ok(SubMsgResponse {
            events: vec![Event::new("wasm").add_attribute("issue_amount", "2000")],
            data: None,
            msg_responses: vec![],
        }),
    };

    let reply_response_2 = reply(deps.as_mut(), env.clone(), reply_2).unwrap();
    assert_eq!(
        reply_response_2.attributes[0].value,
        "convert_lockup_success"
    );

    let updated_lock_2 = LOCKS_MAP_V2.load(&deps.storage, 2).unwrap();
    assert_eq!(updated_lock_2.funds.denom, drop_token_info.d_token_denom);
    assert_eq!(updated_lock_2.funds.amount, Uint128::new(2000));
}
#[test]
fn convert_lockup_to_dtoken_with_pending_slash_conversion_test() {
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-1".to_string(),
        HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
    );
    let (mut deps, mut env) = (mock_dependencies(grpc_query), mock_env());
    let user_address = deps.api.addr_make("addr0000");
    let info = get_message_info(&deps.api, "addr0000", &[]);

    let mut instantiate_msg: crate::msg::InstantiateMsg = get_default_instantiate_msg(&deps.api);
    instantiate_msg.token_info_providers[0] = TokenInfoProviderInstantiateMsg::LSM {
        max_validator_shares_participating: 100,
        hub_connection_id: "connection-0".to_string(),
        hub_transfer_channel_id: "channel-1".to_string(),
        icq_update_period: 100,
    };
    let lock_epoch_length = instantiate_msg.lock_epoch_length;

    let res = instantiate(deps.as_mut(), env.clone(), info, instantiate_msg.clone());
    assert!(res.is_ok());

    // simulate user locking 1000 tokens for 1 month, one day after the round started
    env.block.time = env.block.time.plus_days(1);

    set_default_validator_for_rounds(deps.as_mut(), 0, 100);

    let first_lockup_amount: u128 = 1000;
    let info = get_message_info(
        &deps.api,
        "addr0000",
        &[Coin::new(first_lockup_amount, IBC_DENOM_1.to_string())],
    );
    let msg = ExecuteMsg::LockTokens {
        lock_duration: lock_epoch_length,
        proof: None,
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok());

    let ids: Vec<u64> = vec![1];

    USER_LOCKS
        .save(
            &mut deps.storage,
            user_address.clone(),
            &ids,
            env.block.height,
        )
        .unwrap();

    // simulate user locking 2000 tokens for 3 months, two days after the round started
    env.block.time = env.block.time.plus_days(1);

    let result = set_validator_infos_for_round(
        &mut deps.storage,
        0,
        vec![VALIDATOR_1.to_string(), VALIDATOR_2.to_string()],
    );
    assert!(result.is_ok());

    let first_lockup_amount: u128 = 1000;
    let info = get_message_info(
        &deps.api,
        "addr0000",
        &[Coin::new(first_lockup_amount, IBC_DENOM_1.to_string())],
    );
    let msg = ExecuteMsg::LockTokens {
        lock_duration: 3 * ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok());

    let second_lockup_amount: u128 = 2000;
    let info = get_message_info(
        &deps.api,
        "addr0000",
        &[Coin::new(second_lockup_amount, IBC_DENOM_1.to_string())],
    );
    let msg = ExecuteMsg::LockTokens {
        lock_duration: 3 * ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };

    let res = execute(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok());

    let drop_address = deps.api.addr_make("drop");
    let puppeteer_address = deps.api.addr_make("puppeteer");
    let d_token_denom =
        "factory/neutron1k6hr0f83e7un2wjf29cspk7j69jrnskk65k3ek2nj9dztrlzpj6q00rtsa/udatom"
            .to_string();

    let drop_token_info = DropTokenInfo {
        address: drop_address,
        d_token_denom,
        puppeteer_address,
    };

    let drop_contract = DROP_TOKEN_INFO.save(&mut deps.storage, &drop_token_info);
    assert!(drop_contract.is_ok(), "failed to save drop contract info");

    let lock_duration = 3 * ONE_MONTH_IN_NANO_SECONDS;

    let res_lock = LOCKS_MAP_V2.save(
        &mut deps.storage,
        1,
        &LockEntryV2 {
            lock_id: 1,
            funds: Coin::new(Uint128::from(1000u128), IBC_DENOM_1.to_string()),
            owner: user_address.clone(),
            lock_start: env.block.time,
            lock_end: env.block.time.plus_nanos(lock_duration),
        },
        env.block.height,
    );
    assert!(res_lock.is_ok(), "failed to save lock");

    let constants = load_current_constants(&deps.as_ref(), &env).unwrap();
    let current_round_id = compute_current_round_id(&env, &constants).unwrap();

    let proposal_msgs = vec![
        ExecuteMsg::CreateProposal {
            round_id: Some(current_round_id),
            tranche_id: 1,
            title: "proposal title 1".to_string(),
            description: "proposal description 1".to_string(),
            minimum_atom_liquidity_request: Uint128::zero(),
            deployment_duration: 1,
        },
        ExecuteMsg::CreateProposal {
            round_id: Some(current_round_id),
            tranche_id: 1,
            title: "proposal title 2".to_string(),
            description: "proposal description 2".to_string(),
            minimum_atom_liquidity_request: Uint128::zero(),
            deployment_duration: 1,
        },
    ];

    let info_prop = get_message_info(&deps.api, "addr0000", &[]);

    for proposal_msg in proposal_msgs {
        let res = execute(deps.as_mut(), env.clone(), info_prop.clone(), proposal_msg);
        assert!(res.is_ok());
    }

    let votes = vec![ProposalToLockups {
        proposal_id: 1,
        lock_ids: vec![1],
    }];

    let vote_msg = ExecuteMsg::Vote {
        tranche_id: 1,
        proposals_votes: votes.clone(),
    };
    let res = execute(deps.as_mut(), env.clone(), info_prop.clone(), vote_msg);
    assert!(res.is_ok());

    let info = get_message_info(&deps.api, "addr0000", &[]);

    let res = convert_lockup_to_dtoken(deps.as_mut(), env, info, vec![1]).unwrap();
    assert_eq!(res.messages.len(), 1);
    assert_eq!(res.attributes[0].value, "convert_lockup_to_dtoken");

    let contract_address = deps.api.addr_make("dtoken_info_provider");
    let current_ratio = Decimal::from_str("1.15").unwrap();
    setup_d_atom_token_info_provider_mock(&mut deps, contract_address.clone(), current_ratio);

    let _ = LOCKS_PENDING_SLASHES.save(&mut deps.storage, 1, &Uint128::new(500));

    let payload = ConvertLockupPayload {
        lock_id: 1,
        amount: Uint128::new(1000),
        sender: user_address.clone(),
    };

    let reply_payload = ReplyPayload::ConvertLockup(payload);
    let serialized_payload = to_json_binary(&reply_payload).unwrap();

    // simulate replies
    let reply_1 = Reply {
        id: 1,
        payload: serialized_payload,
        gas_used: 0,
        #[allow(deprecated)]
        result: SubMsgResult::Ok(SubMsgResponse {
            events: vec![Event::new("wasm").add_attribute("issue_amount", "1000")],
            data: None,
            msg_responses: vec![],
        }),
    };

    let power_before = get_total_power_for_proposal(&deps.storage, 1).unwrap();
    let atomics = Uint128::from_str("1500000000000000000000").unwrap();
    let before = Decimal::from_atomics(atomics, 18).unwrap();
    assert_eq!(power_before, before);

    let env = mock_env();
    let reply_response_1 = reply(deps.as_mut(), env.clone(), reply_1).unwrap();
    assert_eq!(
        reply_response_1.attributes[0].value,
        "convert_lockup_success"
    );

    let result = LOCKS_PENDING_SLASHES.may_load(&deps.storage, 1);
    assert_eq!(result.unwrap(), Some(Uint128::new(575))); // 500 converted to 575
}
pub fn setup_d_atom_token_info_provider_mock(
    deps: &mut OwnedDeps<MockStorage, MockApi, MockQuerier, NeutronQuery>,
    token_info_provider_addr: Addr,
    token_group_ratio: Decimal,
) {
    TOKEN_INFO_PROVIDERS
        .save(
            &mut deps.storage,
            token_info_provider_addr.to_string(),
            &TokenInfoProvider::Derivative(TokenInfoProviderDerivative {
                contract: token_info_provider_addr.to_string(),
                cache: HashMap::new(),
            }),
        )
        .unwrap();

    let wasm_querier = MockWasmQuerier::new(token_info_provider_derivative_mock(
        token_info_provider_addr.to_string(),
        DenomInfoResponse {
            denom: DROP_D_TOKEN_DENOM.to_string(),
            token_group_id: D_ATOM_TOKEN_GROUP.to_string(),
            ratio: token_group_ratio,
        },
    ));

    deps.querier.update_wasm(move |q| wasm_querier.handler(q));
}
