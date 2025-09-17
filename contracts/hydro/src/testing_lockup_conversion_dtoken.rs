use std::{collections::HashMap, str::FromStr};

use cosmwasm_std::{
    from_json, testing::mock_env, to_json_binary, Coin, Decimal, Event, Reply, SubMsgResponse,
    SubMsgResult, Uint128,
};

use crate::{
    contract::{
        compute_current_round_id, convert_lockup_to_dtoken, execute, instantiate, query, reply,
    },
    msg::{ConvertLockupPayload, ExecuteMsg, ProposalToLockups, ReplyPayload},
    query::{QueryMsg, TokensResponse},
    score_keeper::get_total_power_for_proposal,
    state::{DropTokenInfo, DROP_TOKEN_INFO, LOCKS_MAP_V2, LOCKS_PENDING_SLASHES},
    testing::{
        get_d_atom_denom_info_mock_data, get_default_instantiate_msg, get_message_info,
        get_validator_info_mock_data, setup_multiple_token_info_provider_mocks, IBC_DENOM_1,
        LSM_TOKEN_PROVIDER_ADDR, ONE_MONTH_IN_NANO_SECONDS, VALIDATOR_1, VALIDATOR_1_LST_DENOM_1,
    },
    testing_mocks::{denom_trace_grpc_query_mock, mock_dependencies},
    utils::load_current_constants,
};

#[test]
fn convert_lockup_to_dtoken_test() {
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
    );
    let (mut deps, mut env) = (mock_dependencies(grpc_query), mock_env());
    let user_address = deps.api.addr_make("addr0000");
    let info = get_message_info(&deps.api, "addr0000", &[]);

    let instantiate_msg: crate::msg::InstantiateMsg = get_default_instantiate_msg(&deps.api);
    let lock_epoch_length = instantiate_msg.lock_epoch_length;

    let res = instantiate(deps.as_mut(), env.clone(), info, instantiate_msg.clone());
    assert!(res.is_ok());

    // simulate user locking 1000 tokens for 1 month, one day after the round started
    env.block.time = env.block.time.plus_days(1);

    let d_token_info_provider_addr = deps.api.addr_make("dtoken_info_provider");
    let d_atom_ratio = Decimal::from_str("1.15").unwrap();

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

    // simulate user locking 2000 tokens for 3 months, two days after the round started
    env.block.time = env.block.time.plus_days(1);

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

    // Query all tokens before conversion - should return nothing since both lockups are LSM-based
    let query_msg = QueryMsg::AllTokens {
        start_after: None,
        limit: None,
    };
    let query_res = query(deps.as_ref(), env.clone(), query_msg);
    assert!(
        query_res.is_ok(),
        "Failed to query all tokens before conversion"
    );
    let tokens_before: TokensResponse = from_json(query_res.unwrap()).unwrap();
    assert_eq!(
        tokens_before.tokens.len(),
        0,
        "Expected no tokens before conversion since lockups are LSM-based"
    );

    let res = convert_lockup_to_dtoken(deps.as_mut(), env, info, vec![1, 2]).unwrap();
    assert_eq!(res.messages.len(), 2);
    assert_eq!(res.attributes[0].value, "convert_lockup_to_dtoken");

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

    // Mock pending slash for lock with id 2
    LOCKS_PENDING_SLASHES
        .save(&mut deps.storage, 2, &Uint128::from(100u128))
        .unwrap();

    let reply_2 = Reply {
        id: 2,
        payload: serialized_payload,
        gas_used: 0,
        #[allow(deprecated)]
        result: SubMsgResult::Ok(SubMsgResponse {
            events: vec![Event::new("wasm").add_attribute("issue_amount", "1700")],
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
    assert_eq!(updated_lock_2.funds.amount, Uint128::new(1700));

    assert_eq!(
        LOCKS_PENDING_SLASHES.load(&deps.storage, 2).unwrap().u128(),
        85
    );

    // Query all tokens after conversion - should return both lockups since they are now d-tokens (non-LSM)
    let query_msg = QueryMsg::AllTokens {
        start_after: None,
        limit: None,
    };
    let query_res = query(deps.as_ref(), env.clone(), query_msg);
    assert!(
        query_res.is_ok(),
        "Failed to query all tokens after conversion"
    );
    let tokens_after: TokensResponse = from_json(query_res.unwrap()).unwrap();
    assert_eq!(
        tokens_after.tokens.len(),
        2,
        "Expected both tokens after conversion to d-tokens"
    );
    assert_eq!(
        tokens_after.tokens,
        vec!["1".to_string(), "2".to_string()],
        "Expected tokens 1 and 2 to be returned"
    );
}

#[test]
fn convert_lockup_to_dtoken_with_pending_slash_conversion_test() {
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
    );
    let (mut deps, mut env) = (mock_dependencies(grpc_query), mock_env());
    let user_address = deps.api.addr_make("addr0000");
    let info = get_message_info(&deps.api, "addr0000", &[]);

    let instantiate_msg = get_default_instantiate_msg(&deps.api);

    let lock_epoch_length = instantiate_msg.lock_epoch_length;

    let res = instantiate(deps.as_mut(), env.clone(), info, instantiate_msg.clone());
    assert!(res.is_ok());

    // simulate user locking 1000 tokens for 1 month, one day after the round started
    env.block.time = env.block.time.plus_days(1);

    let d_token_info_provider_addr = deps.api.addr_make("dtoken_info_provider");
    let d_atom_ratio = Decimal::from_str("1.15").unwrap();

    let derivative_providers = HashMap::from([get_d_atom_denom_info_mock_data(
        d_token_info_provider_addr.to_string(),
        vec![(0, d_atom_ratio)],
    )]);

    let lsm_token_info_provider_addr = deps.api.addr_make(LSM_TOKEN_PROVIDER_ADDR);
    let lsm_provider = Some((
        lsm_token_info_provider_addr.to_string(),
        HashMap::from_iter((0..1).map(|round_id: u64| {
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

    // simulate user locking 2000 tokens for 3 months, two days after the round started
    env.block.time = env.block.time.plus_days(1);

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
    assert_eq!(result.unwrap(), Some(Uint128::new(500)));
}
