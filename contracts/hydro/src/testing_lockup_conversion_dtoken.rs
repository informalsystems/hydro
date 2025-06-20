use std::collections::HashMap;

use cosmwasm_std::{
    testing::mock_env, Binary, Coin, Event, Reply, SubMsgResponse, SubMsgResult, Timestamp, Uint128,
};

use crate::{
    contract::{
        compute_current_round_id, convert_lockup_to_dtoken, convert_lockup_to_dtoken_reply,
        execute, instantiate,
    },
    msg::{ExecuteMsg, ProposalToLockups},
    state::{DropTokenInfo, LockEntryV2, DROP_TOKEN_INFO, LOCKS_MAP_V2, USER_LOCKS},
    testing::{
        get_default_instantiate_msg, get_message_info, set_default_validator_for_rounds,
        IBC_DENOM_1, ONE_MONTH_IN_NANO_SECONDS, VALIDATOR_1, VALIDATOR_1_LST_DENOM_1, VALIDATOR_2,
    },
    testing_lsm_integration::set_validator_infos_for_round,
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
    let info = get_message_info(&deps.api, &user_address.to_string(), &[]);

    let instantiate_msg: crate::msg::InstantiateMsg = get_default_instantiate_msg(&deps.api);
    let lock_epoch_length = instantiate_msg.lock_epoch_length;

    let res = instantiate(deps.as_mut(), env.clone(), info, instantiate_msg.clone());
    assert!(res.is_ok());

    // simulate user locking 1000 tokens for 1 month, one day after the round started
    env.block.time = env.block.time.plus_days(1);

    set_default_validator_for_rounds(deps.as_mut(), 0, 100);

    let first_lockup_amount: u128 = 1000;
    let info = get_message_info(
        &deps.api,
        &user_address.to_string(),
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
        &user_address.to_string(),
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
        &user_address.to_string(),
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

    let info = get_message_info(&deps.api, &user_address.to_string(), &[]);

    let res = convert_lockup_to_dtoken(deps.as_mut(), env, info, vec![1, 2]).unwrap();
    assert_eq!(res.messages.len(), 2);
    assert_eq!(res.attributes[0].value, "convert_lockup_to_dtoken");

    // simulate replies
    let reply_1 = Reply {
        id: 1,
        payload: Binary::default(),
        gas_used: 0,
        result: SubMsgResult::Ok(SubMsgResponse {
            events: vec![Event::new("wasm").add_attribute("issue_amount", "1000")],
            data: None,
            msg_responses: vec![],
        }),
    };

    let env = mock_env();
    let reply_response_1 =
        convert_lockup_to_dtoken_reply(deps.as_mut(), env.clone(), reply_1).unwrap();
    assert_eq!(
        reply_response_1.attributes[0].value,
        "convert_lockup_success"
    );

    let updated_lock_1 = LOCKS_MAP_V2.load(&deps.storage, 1).unwrap();
    assert_eq!(updated_lock_1.funds.denom, drop_token_info.d_token_denom);
    assert_eq!(updated_lock_1.funds.amount, Uint128::new(1000));

    let reply_2 = Reply {
        id: 2,
        payload: Binary::default(),
        gas_used: 0,
        result: SubMsgResult::Ok(SubMsgResponse {
            events: vec![Event::new("wasm").add_attribute("issue_amount", "2000")],
            data: None,
            msg_responses: vec![],
        }),
    };

    let reply_response_2 =
        convert_lockup_to_dtoken_reply(deps.as_mut(), env.clone(), reply_2).unwrap();
    assert_eq!(
        reply_response_2.attributes[0].value,
        "convert_lockup_success"
    );

    let updated_lock_2 = LOCKS_MAP_V2.load(&deps.storage, 2).unwrap();
    assert_eq!(updated_lock_2.funds.denom, drop_token_info.d_token_denom);
    assert_eq!(updated_lock_2.funds.amount, Uint128::new(2000));
}
