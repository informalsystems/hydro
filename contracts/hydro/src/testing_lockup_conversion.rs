use std::{collections::HashMap, str::FromStr};

use cosmwasm_std::{testing::mock_env, Coin, Decimal, Uint128};

use crate::{
    contract::{execute, instantiate, query_converted_token_num, query_proposal},
    cw721::query_all_tokens,
    msg::{ExecuteMsg, ProposalToLockups},
    state::{
        AVAILABLE_CONVERSION_FUNDS, LOCKED_TOKENS, LOCKS_MAP_V2, LOCKS_PENDING_SLASHES, VOTE_MAP_V2,
    },
    testing::{
        get_d_atom_denom_info_mock_data, get_default_instantiate_msg, get_message_info,
        get_st_atom_denom_info_mock_data, get_validator_info_mock_data,
        setup_multiple_token_info_provider_mocks, D_ATOM_ON_NEUTRON, IBC_DENOM_1, IBC_DENOM_2,
        LSM_TOKEN_PROVIDER_ADDR, ST_ATOM_ON_NEUTRON, ST_ATOM_TOKEN_GROUP, VALIDATOR_1,
        VALIDATOR_1_LST_DENOM_1, VALIDATOR_2,
    },
    testing_mocks::{denom_trace_grpc_query_mock, mock_dependencies},
};

#[test]
fn lockup_conversion_test() {
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
    );

    let round_id = 0;
    let tranche_id = 1;
    let lock_id1 = 0;
    let prop_id1 = 0;

    let (mut deps, mut env) = (mock_dependencies(grpc_query), mock_env());

    let whitelist_admin_address = deps.api.addr_make("addr0001");

    let mut instantiate_msg = get_default_instantiate_msg(&deps.api);

    instantiate_msg.round_length = instantiate_msg.lock_epoch_length;
    instantiate_msg.whitelist_admins = vec![whitelist_admin_address.to_string()];

    let whitelist_info = get_message_info(&deps.api, "addr0000", &[]);
    let whitelist_admin_info = get_message_info(&deps.api, "addr0001", &[]);

    let res = instantiate(
        deps.as_mut(),
        env.clone(),
        whitelist_admin_info.clone(),
        instantiate_msg.clone(),
    );
    assert!(res.is_ok());

    let d_token_info_provider_addr = deps.api.addr_make("dtoken_info_provider");
    let st_token_info_provider_addr = deps.api.addr_make("sttoken_info_provider");

    let d_atom_ratio = Decimal::from_str("1.2").unwrap();
    let st_atom_ratio = Decimal::from_str("1.6").unwrap();

    let derivative_providers = HashMap::from([
        get_d_atom_denom_info_mock_data(
            d_token_info_provider_addr.to_string(),
            (0..=1)
                .map(|round_id: u64| (round_id, d_atom_ratio))
                .collect(),
        ),
        get_st_atom_denom_info_mock_data(
            st_token_info_provider_addr.to_string(),
            (0..=1)
                .map(|round_id: u64| (round_id, st_atom_ratio))
                .collect(),
        ),
    ]);

    let lsm_token_info_provider_addr = deps.api.addr_make(LSM_TOKEN_PROVIDER_ADDR);
    let lsm_provider = Some((
        lsm_token_info_provider_addr.to_string(),
        HashMap::from_iter((0..=1).map(|round_id: u64| {
            (
                round_id,
                HashMap::from([
                    get_validator_info_mock_data(VALIDATOR_1.to_string(), Decimal::one()),
                    get_validator_info_mock_data(VALIDATOR_2.to_string(), Decimal::one()),
                ]),
            )
        })),
    ));

    setup_multiple_token_info_provider_mocks(
        &mut deps,
        derivative_providers.clone(),
        lsm_provider.clone(),
        true,
    );

    // simulate user locking 1000 dATOM tokens for 3 months, one day after the round started
    env.block.time = env.block.time.plus_days(1);

    let lockup_amount = 1000u128;
    let user1_info = get_message_info(
        &deps.api,
        "addr0002",
        &[Coin::new(lockup_amount, D_ATOM_ON_NEUTRON.to_string())],
    );

    let msg = ExecuteMsg::LockTokens {
        lock_duration: 3 * instantiate_msg.lock_epoch_length,
        proof: None,
    };

    let time_weight_multiplier = Decimal::from_str("1.5").unwrap();

    let res = execute(deps.as_mut(), env.clone(), user1_info.clone(), msg);
    assert!(res.is_ok());

    let create_proposal_msg = ExecuteMsg::CreateProposal {
        round_id: None,
        tranche_id,
        title: "proposal title 1".to_string(),
        description: "proposal description 1".to_string(),
        minimum_atom_liquidity_request: Uint128::zero(),
        deployment_duration: 1,
    };

    let res = execute(
        deps.as_mut(),
        env.clone(),
        whitelist_info.clone(),
        create_proposal_msg,
    );
    assert!(res.is_ok());

    let vote_msg = ExecuteMsg::Vote {
        tranche_id: 1,
        proposals_votes: vec![ProposalToLockups {
            proposal_id: prop_id1,
            lock_ids: vec![lock_id1],
        }],
    };

    // Add a pending slash to verify that they get converted properly
    LOCKS_PENDING_SLASHES
        .save(&mut deps.storage, lock_id1, &Uint128::from(100u128))
        .unwrap();

    let res = execute(deps.as_mut(), env.clone(), user1_info.clone(), vote_msg);
    assert!(res.is_ok());

    let expected_user_power = Uint128::new(1800); // 1000 * 1.5 * 1.2

    let proposal_power_before = query_proposal(deps.as_ref(), round_id, tranche_id, prop_id1)
        .unwrap()
        .proposal
        .power;
    assert_eq!(proposal_power_before, expected_user_power);

    // dATOM lockup is a token- check that it gets returned by the all_tokens() query
    let all_tokens = query_all_tokens(deps.as_ref(), env.clone(), None, None)
        .unwrap()
        .tokens;
    assert_eq!(all_tokens.len(), 1);
    assert_eq!(all_tokens[0], lock_id1.to_string());

    // Verify that queries give correct number of tokens required to convert the lockup
    let tokens_to_receive_user_provides_no_funds = query_converted_token_num(
        deps.as_ref(),
        env.clone(),
        lock_id1,
        IBC_DENOM_1.to_string(),
        false,
    )
    .unwrap();
    assert_eq!(tokens_to_receive_user_provides_no_funds, Uint128::new(1176));

    let tokens_to_receive_user_provides_funds = query_converted_token_num(
        deps.as_ref(),
        env.clone(),
        lock_id1,
        IBC_DENOM_1.to_string(),
        true,
    )
    .unwrap();
    assert_eq!(tokens_to_receive_user_provides_funds, Uint128::new(1200));

    // Have non-owner user try to convert lockup - should fail
    let user2_info = get_message_info(&deps.api, "addr0003", &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        user2_info.clone(),
        ExecuteMsg::ConvertLockup {
            lock_id: lock_id1,
            target_denom: IBC_DENOM_1.to_string(),
        },
    );
    assert!(res.unwrap_err().to_string().contains("Unauthorized"));

    // Have lockup owner try to convert by providing funds of a wrong denom - should fail
    let user1_info = get_message_info(
        &deps.api,
        "addr0002",
        &[Coin::new(2000u128, IBC_DENOM_2.to_string())],
    );
    let res = execute(
        deps.as_mut(),
        env.clone(),
        user1_info.clone(),
        ExecuteMsg::ConvertLockup {
            lock_id: lock_id1,
            target_denom: IBC_DENOM_1.to_string(),
        },
    );
    assert!(res
        .unwrap_err()
        .to_string()
        .contains(format!("Must send reserve token '{IBC_DENOM_1}'").as_str()));

    // Have lockup owner try to convert to the same denom - should fail
    let user1_info = get_message_info(&deps.api, "addr0002", &[]);
    let res = execute(
        deps.as_mut(),
        env.clone(),
        user1_info.clone(),
        ExecuteMsg::ConvertLockup {
            lock_id: lock_id1,
            target_denom: D_ATOM_ON_NEUTRON.to_string(),
        },
    );
    assert!(res
        .unwrap_err()
        .to_string()
        .contains("source and target denoms are the same"));

    // Have lockup owner try to convert to denom that can't be locked in Hydro- should fail
    let res = execute(
        deps.as_mut(),
        env.clone(),
        user1_info.clone(),
        ExecuteMsg::ConvertLockup {
            lock_id: lock_id1,
            target_denom: "invalid_denom".to_string(),
        },
    );
    assert!(res
        .unwrap_err()
        .to_string()
        .contains("failed to obtain token ratio for denom: invalid_denom"));

    // Have lockup owner try to convert to denom without providing the funds,
    // while the funds aren't provided by the whitelist admin either- should fail
    let res = execute(
        deps.as_mut(),
        env.clone(),
        user1_info.clone(),
        ExecuteMsg::ConvertLockup {
            lock_id: lock_id1,
            target_denom: IBC_DENOM_1.to_string(),
        },
    );
    assert!(res.unwrap_err().to_string().contains(format!("insufficient funds to perform conversion into denom: {IBC_DENOM_1}. required funds: {}, available funds: {}", Uint128::new(1176), Uint128::new(0)).as_str()));

    // Have lockup owner try to convert to denom while providing insufficient funds - should fail
    let user1_info = get_message_info(
        &deps.api,
        "addr0002",
        &[Coin::new(1199u128, IBC_DENOM_1.to_string())],
    );
    let res = execute(
        deps.as_mut(),
        env.clone(),
        user1_info.clone(),
        ExecuteMsg::ConvertLockup {
            lock_id: lock_id1,
            target_denom: IBC_DENOM_1.to_string(),
        },
    );
    assert!(res.unwrap_err().to_string().contains(format!("funds provided for conversion must be exact match to required amount; provided: {}, required: {}", Uint128::new(1199), Uint128::new(1200)).as_str()));

    // Have lockup owner convert to denom by providing the required amount of tokens - should succeed
    let user1_info = get_message_info(
        &deps.api,
        "addr0002",
        &[Coin::new(1200u128, IBC_DENOM_1.to_string())],
    );
    let res = execute(
        deps.as_mut(),
        env.clone(),
        user1_info.clone(),
        ExecuteMsg::ConvertLockup {
            lock_id: lock_id1,
            target_denom: IBC_DENOM_1.to_string(),
        },
    );
    assert!(res.is_ok());

    // Verify that the correct number of dATOM tokens are sent back to the user
    let messages = res.unwrap().messages;
    assert_eq!(messages.len(), 1);

    match messages[0].clone().msg {
        cosmwasm_std::CosmosMsg::Bank(bank_msg) => match bank_msg {
            cosmwasm_std::BankMsg::Send { to_address, amount } => {
                assert_eq!(to_address, user1_info.sender.to_string());
                assert_eq!(amount[0].denom, D_ATOM_ON_NEUTRON.to_string());
                assert_eq!(amount[0].amount, Uint128::new(lockup_amount));
            }
            _ => panic!("expected bank send message"),
        },
        _ => panic!("expected bank message"),
    }

    // Verify that lockup entry has been updated
    let expected_lock_funds = Uint128::new(1200);
    let updated_lock = LOCKS_MAP_V2.load(&deps.storage, lock_id1).unwrap();
    assert_eq!(updated_lock.funds.denom, IBC_DENOM_1.to_string());
    assert_eq!(updated_lock.funds.amount, expected_lock_funds);

    // Verify that the lockup is no longer a token (since it is LSM lockup now)
    let all_tokens = query_all_tokens(deps.as_ref(), env.clone(), None, None)
        .unwrap()
        .tokens;
    assert_eq!(all_tokens.len(), 0);

    // Verify that pending slash entry has been updated
    assert_eq!(
        LOCKS_PENDING_SLASHES
            .load(&deps.storage, lock_id1)
            .unwrap()
            .u128(),
        120
    );

    // Check the lockup vote
    // Expected shares: 1200 (token num) * 1.5 (time weight) = 1800
    let expected_time_weighted_shares = Decimal::from_ratio(expected_lock_funds, Uint128::one())
        .checked_mul(time_weight_multiplier)
        .unwrap();

    let vote = VOTE_MAP_V2
        .load(&deps.storage, ((round_id, tranche_id), lock_id1))
        .unwrap();
    assert_eq!(vote.prop_id, prop_id1);
    assert_eq!(vote.time_weighted_shares.0, VALIDATOR_1.to_string());
    assert_eq!(vote.time_weighted_shares.1, expected_time_weighted_shares);

    // Verify that the proposal power is unchanged after conversion
    let proposal_power_after = query_proposal(deps.as_ref(), round_id, tranche_id, prop_id1)
        .unwrap()
        .proposal
        .power;
    assert_eq!(proposal_power_after, proposal_power_before);

    // Verify that the total number of locked tokens is updated
    assert_eq!(
        LOCKED_TOKENS.load(&deps.storage).unwrap(),
        Uint128::new(1200).u128()
    );

    // Verify that the query gives correct number of tokens required to convert the lockup into stATOM
    let tokens_to_receive_user_provides_no_funds = query_converted_token_num(
        deps.as_ref(),
        env.clone(),
        lock_id1,
        ST_ATOM_ON_NEUTRON.to_string(),
        false,
    )
    .unwrap();
    assert_eq!(tokens_to_receive_user_provides_no_funds, Uint128::new(735));

    // Have whitelist admin provide some (but not enough) funds to convert into stATOM
    let whitelist_admin_info = get_message_info(
        &deps.api,
        "addr0001",
        &[Coin::new(500u128, ST_ATOM_ON_NEUTRON.to_string())],
    );

    let msg = ExecuteMsg::ProvideConversionFunds {};
    let res = execute(
        deps.as_mut(),
        env.clone(),
        whitelist_admin_info.clone(),
        msg,
    );
    assert!(res.is_ok());

    // Have lockup owner try to convert to stATOM without providing any funds
    // Since whitelisted address didn't provide enough funds - should fail
    let user1_info = get_message_info(&deps.api, "addr0002", &[]);

    let res = execute(
        deps.as_mut(),
        env.clone(),
        user1_info.clone(),
        ExecuteMsg::ConvertLockup {
            lock_id: lock_id1,
            target_denom: ST_ATOM_ON_NEUTRON.to_string(),
        },
    );
    assert!(res.unwrap_err().to_string().contains(format!("insufficient funds to perform conversion into denom: {ST_ATOM_ON_NEUTRON}. required funds: {}, available funds: {}", Uint128::new(735), Uint128::new(500)).as_str()));

    // Even though previous execute failed, the storage update wasn't reverted, so we need to revert it manually
    assert_eq!(
        AVAILABLE_CONVERSION_FUNDS
            .load(&deps.storage, IBC_DENOM_1.to_string())
            .unwrap(),
        Uint128::new(1200) // this value should be 0
    );
    AVAILABLE_CONVERSION_FUNDS
        .save(&mut deps.storage, IBC_DENOM_1.to_string(), &Uint128::zero())
        .unwrap();

    // Have whitelist admin provide missing funds to convert into stATOM
    let whitelist_admin_info = get_message_info(
        &deps.api,
        "addr0001",
        &[Coin::new(235u128, ST_ATOM_ON_NEUTRON.to_string())],
    );

    let msg = ExecuteMsg::ProvideConversionFunds {};
    let res = execute(
        deps.as_mut(),
        env.clone(),
        whitelist_admin_info.clone(),
        msg,
    );
    assert!(res.is_ok());

    // Have lockup owner successfully convert to stATOM without providing any funds- should succeed
    let user1_info = get_message_info(&deps.api, "addr0002", &[]);

    let res = execute(
        deps.as_mut(),
        env.clone(),
        user1_info.clone(),
        ExecuteMsg::ConvertLockup {
            lock_id: lock_id1,
            target_denom: ST_ATOM_ON_NEUTRON.to_string(),
        },
    );
    assert!(res.is_ok());

    // Verify that no tokens are sent back to the user, since user didn't provide any conversion funds
    assert_eq!(res.unwrap().messages.len(), 0);

    // User lost 2% of their funds due to conversion without providing funds
    let expected_lock_funds = Uint128::new(735);

    // Expected power: 735 (token num) * 1.5 (time weight) * 1.6 (stATOM ratio) = 1764
    let expected_user_power = Decimal::from_ratio(expected_lock_funds, Uint128::one())
        .checked_mul(time_weight_multiplier)
        .unwrap()
        .checked_mul(st_atom_ratio)
        .unwrap();

    // Verify that lockup entry has been updated
    let updated_lock = LOCKS_MAP_V2.load(&deps.storage, lock_id1).unwrap();
    assert_eq!(updated_lock.funds.denom, ST_ATOM_ON_NEUTRON.to_string());
    assert_eq!(updated_lock.funds.amount, expected_lock_funds);

    // Verify that the lockup is again a token (since it is LST lockup now)
    let all_tokens = query_all_tokens(deps.as_ref(), env.clone(), None, None)
        .unwrap()
        .tokens;
    assert_eq!(all_tokens.len(), 1);
    assert_eq!(all_tokens[0], lock_id1.to_string());

    // Verify that pending slash entry has been updated
    assert_eq!(
        LOCKS_PENDING_SLASHES
            .load(&deps.storage, lock_id1)
            .unwrap()
            .u128(),
        73
    );

    // Check the lockup vote
    // Expected time weighted shares: 735 (token num) * 1.5 (time weight) = 1102
    let expected_time_weighted_shares = Decimal::from_ratio(Uint128::new(1102), Uint128::one());

    let vote = VOTE_MAP_V2
        .load(&deps.storage, ((round_id, tranche_id), lock_id1))
        .unwrap();
    assert_eq!(vote.prop_id, prop_id1);
    assert_eq!(vote.time_weighted_shares.0, ST_ATOM_TOKEN_GROUP.to_string());
    assert_eq!(vote.time_weighted_shares.1, expected_time_weighted_shares);

    // Verify that the proposal power is also reduced by 2%
    let expected_proposal_power = expected_user_power.to_uint_ceil();
    let proposal_power = query_proposal(deps.as_ref(), round_id, tranche_id, prop_id1)
        .unwrap()
        .proposal
        .power;
    assert_eq!(proposal_power, expected_proposal_power);

    // Verify that the total number of locked tokens is updated
    assert_eq!(
        LOCKED_TOKENS.load(&deps.storage).unwrap(),
        expected_lock_funds.u128()
    );

    // Verify that the available conversion funds have been updated for both source and target denoms
    assert_eq!(
        AVAILABLE_CONVERSION_FUNDS
            .load(&deps.storage, IBC_DENOM_1.to_string())
            .unwrap(),
        Uint128::new(1200)
    );

    assert_eq!(
        AVAILABLE_CONVERSION_FUNDS
            .load(&deps.storage, ST_ATOM_ON_NEUTRON.to_string())
            .unwrap(),
        Uint128::zero()
    );

    // Have a whitelist admin withdraw all available conversion funds for IBC_DENOM_1 by specifying
    // more than the available amount- should succeed and withdraw only what is available
    let whitelist_admin_info = get_message_info(&deps.api, "addr0001", &[]);

    let msg = ExecuteMsg::WithdrawConversionFunds {
        funds_to_withdraw: vec![Coin::new(1500u128, IBC_DENOM_1.to_string())],
    };
    let res = execute(
        deps.as_mut(),
        env.clone(),
        whitelist_admin_info.clone(),
        msg,
    );
    assert!(res.is_ok());

    // Verify that the correct number of IBC_DENOM_1 tokens are sent back to the whitelist admin
    let messages = res.unwrap().messages;
    assert_eq!(messages.len(), 1);

    match messages[0].clone().msg {
        cosmwasm_std::CosmosMsg::Bank(bank_msg) => match bank_msg {
            cosmwasm_std::BankMsg::Send { to_address, amount } => {
                assert_eq!(to_address, whitelist_admin_info.sender.to_string());
                assert_eq!(amount[0].denom, IBC_DENOM_1.to_string());
                assert_eq!(amount[0].amount, Uint128::new(1200));
            }
            _ => panic!("expected bank send message"),
        },
        _ => panic!("expected bank message"),
    }

    // Verify that the available conversion funds have been updated correctly
    assert_eq!(
        AVAILABLE_CONVERSION_FUNDS
            .load(&deps.storage, IBC_DENOM_1.to_string())
            .unwrap(),
        Uint128::zero()
    );
}

#[test]
fn query_all_available_conversion_funds_test() {
    use crate::contract::query_all_available_conversion_funds;

    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
    );

    let round_id = 0u64;

    let (mut deps, env) = (mock_dependencies(grpc_query), mock_env());

    let whitelist_admin_address = deps.api.addr_make("addr0001");

    let mut instantiate_msg = get_default_instantiate_msg(&deps.api);
    instantiate_msg.round_length = instantiate_msg.lock_epoch_length;
    instantiate_msg.whitelist_admins = vec![whitelist_admin_address.to_string()];

    let whitelist_admin_info = get_message_info(&deps.api, "addr0001", &[]);

    let res = instantiate(
        deps.as_mut(),
        env.clone(),
        whitelist_admin_info.clone(),
        instantiate_msg.clone(),
    );
    assert!(res.is_ok());

    let d_token_info_provider_addr = deps.api.addr_make("dtoken_info_provider");
    let st_token_info_provider_addr = deps.api.addr_make("sttoken_info_provider");

    let d_atom_ratio = Decimal::from_str("1.2").unwrap();
    let st_atom_ratio = Decimal::from_str("1.6").unwrap();

    let derivative_providers = HashMap::from([
        get_d_atom_denom_info_mock_data(
            d_token_info_provider_addr.to_string(),
            (0..=1)
                .map(|round_id: u64| (round_id, d_atom_ratio))
                .collect(),
        ),
        get_st_atom_denom_info_mock_data(
            st_token_info_provider_addr.to_string(),
            (0..=1)
                .map(|round_id: u64| (round_id, st_atom_ratio))
                .collect(),
        ),
    ]);

    let lsm_token_info_provider_addr = deps.api.addr_make(LSM_TOKEN_PROVIDER_ADDR);
    let lsm_provider = Some((
        lsm_token_info_provider_addr.to_string(),
        HashMap::from_iter((0..=1).map(|round_id: u64| {
            (
                round_id,
                HashMap::from([
                    get_validator_info_mock_data(VALIDATOR_1.to_string(), Decimal::one()),
                    get_validator_info_mock_data(VALIDATOR_2.to_string(), Decimal::one()),
                ]),
            )
        })),
    ));

    setup_multiple_token_info_provider_mocks(
        &mut deps,
        derivative_providers.clone(),
        lsm_provider.clone(),
        true,
    );

    // Test 1: Query when no conversion funds exist - should return empty response
    let response =
        query_all_available_conversion_funds(deps.as_ref(), round_id, None, None).unwrap();
    assert_eq!(response.round_id, round_id);
    assert!(response.funds.is_empty());
    assert_eq!(response.total_base_token_equivalent, Uint128::zero());
    assert!(!response.has_more);

    // Test 2: Provide conversion funds for stATOM (ratio 1.6) and dATOM (ratio 1.2)
    let st_atom_amount = 1000u128;
    let d_atom_amount = 500u128;

    // Provide stATOM conversion funds
    let whitelist_admin_info = get_message_info(
        &deps.api,
        "addr0001",
        &[Coin::new(st_atom_amount, ST_ATOM_ON_NEUTRON.to_string())],
    );
    let msg = ExecuteMsg::ProvideConversionFunds {};
    let res = execute(
        deps.as_mut(),
        env.clone(),
        whitelist_admin_info.clone(),
        msg,
    );
    assert!(res.is_ok());

    // Provide dATOM conversion funds
    let whitelist_admin_info = get_message_info(
        &deps.api,
        "addr0001",
        &[Coin::new(d_atom_amount, D_ATOM_ON_NEUTRON.to_string())],
    );
    let msg = ExecuteMsg::ProvideConversionFunds {};
    let res = execute(
        deps.as_mut(),
        env.clone(),
        whitelist_admin_info.clone(),
        msg,
    );
    assert!(res.is_ok());

    // Query all available conversion funds
    let response =
        query_all_available_conversion_funds(deps.as_ref(), round_id, None, None).unwrap();

    assert_eq!(response.round_id, round_id);
    assert_eq!(response.funds.len(), 2);
    assert!(!response.has_more);

    // Expected base token equivalents:
    // stATOM: 1000 * 1.6 = 1600
    // dATOM: 500 * 1.2 = 600
    // Total: 2200
    let expected_total = Uint128::new(1600 + 600);
    assert_eq!(response.total_base_token_equivalent, expected_total);

    // Verify individual fund entries (order may vary due to map iteration)
    let st_atom_fund = response
        .funds
        .iter()
        .find(|f| f.denom == ST_ATOM_ON_NEUTRON)
        .expect("stATOM fund should exist");
    assert_eq!(st_atom_fund.amount, Uint128::new(st_atom_amount));
    assert_eq!(st_atom_fund.ratio, st_atom_ratio);
    assert_eq!(st_atom_fund.base_token_equivalent, Uint128::new(1600));

    let d_atom_fund = response
        .funds
        .iter()
        .find(|f| f.denom == D_ATOM_ON_NEUTRON)
        .expect("dATOM fund should exist");
    assert_eq!(d_atom_fund.amount, Uint128::new(d_atom_amount));
    assert_eq!(d_atom_fund.ratio, d_atom_ratio);
    assert_eq!(d_atom_fund.base_token_equivalent, Uint128::new(600));

    // Test 3: Add funds for an unknown denom (not recognized by any token info provider)
    let unknown_denom = "unknown_token";
    let unknown_amount = 2000u128;
    AVAILABLE_CONVERSION_FUNDS
        .save(
            &mut deps.storage,
            unknown_denom.to_string(),
            &Uint128::new(unknown_amount),
        )
        .unwrap();

    let response =
        query_all_available_conversion_funds(deps.as_ref(), round_id, None, None).unwrap();

    assert_eq!(response.funds.len(), 3);
    assert!(!response.has_more);

    // Total should still be 2200 (unknown token contributes 0 due to zero ratio)
    assert_eq!(response.total_base_token_equivalent, expected_total);

    // Verify unknown denom has zero ratio and zero equivalent
    let unknown_fund = response
        .funds
        .iter()
        .find(|f| f.denom == unknown_denom)
        .expect("unknown fund should exist");
    assert_eq!(unknown_fund.amount, Uint128::new(unknown_amount));
    assert_eq!(unknown_fund.ratio, Decimal::zero());
    assert_eq!(unknown_fund.base_token_equivalent, Uint128::zero());
}

#[test]
fn query_all_available_conversion_funds_pagination_test() {
    use crate::contract::query_all_available_conversion_funds;

    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
    );

    let round_id = 0u64;

    let (mut deps, env) = (mock_dependencies(grpc_query), mock_env());

    let whitelist_admin_address = deps.api.addr_make("addr0001");

    let mut instantiate_msg = get_default_instantiate_msg(&deps.api);
    instantiate_msg.round_length = instantiate_msg.lock_epoch_length;
    instantiate_msg.whitelist_admins = vec![whitelist_admin_address.to_string()];

    let whitelist_admin_info = get_message_info(&deps.api, "addr0001", &[]);

    let res = instantiate(
        deps.as_mut(),
        env.clone(),
        whitelist_admin_info.clone(),
        instantiate_msg.clone(),
    );
    assert!(res.is_ok());

    // Add 5 denoms directly to storage for pagination testing
    // Using alphabetically ordered denoms: denom_a, denom_b, denom_c, denom_d, denom_e
    let denoms = ["denom_a", "denom_b", "denom_c", "denom_d", "denom_e"];
    for (i, denom) in denoms.iter().enumerate() {
        AVAILABLE_CONVERSION_FUNDS
            .save(
                &mut deps.storage,
                denom.to_string(),
                &Uint128::new((i as u128 + 1) * 100), // 100, 200, 300, 400, 500
            )
            .unwrap();
    }

    // Test 1: Query with limit=2 - should return first 2 denoms and has_more=true
    let response =
        query_all_available_conversion_funds(deps.as_ref(), round_id, None, Some(2)).unwrap();
    assert_eq!(response.funds.len(), 2);
    assert!(response.has_more);
    assert_eq!(response.funds[0].denom, "denom_a");
    assert_eq!(response.funds[0].amount, Uint128::new(100));
    assert_eq!(response.funds[1].denom, "denom_b");
    assert_eq!(response.funds[1].amount, Uint128::new(200));

    // Test 2: Query with start_after="denom_b" and limit=2 - should return denom_c and denom_d
    let response = query_all_available_conversion_funds(
        deps.as_ref(),
        round_id,
        Some("denom_b".to_string()),
        Some(2),
    )
    .unwrap();
    assert_eq!(response.funds.len(), 2);
    assert!(response.has_more);
    assert_eq!(response.funds[0].denom, "denom_c");
    assert_eq!(response.funds[0].amount, Uint128::new(300));
    assert_eq!(response.funds[1].denom, "denom_d");
    assert_eq!(response.funds[1].amount, Uint128::new(400));

    // Test 3: Query with start_after="denom_d" and limit=2 - should return denom_e and has_more=false
    let response = query_all_available_conversion_funds(
        deps.as_ref(),
        round_id,
        Some("denom_d".to_string()),
        Some(2),
    )
    .unwrap();
    assert_eq!(response.funds.len(), 1);
    assert!(!response.has_more);
    assert_eq!(response.funds[0].denom, "denom_e");
    assert_eq!(response.funds[0].amount, Uint128::new(500));

    // Test 4: Query with start_after="denom_e" - should return empty with has_more=false
    let response = query_all_available_conversion_funds(
        deps.as_ref(),
        round_id,
        Some("denom_e".to_string()),
        Some(2),
    )
    .unwrap();
    assert!(response.funds.is_empty());
    assert!(!response.has_more);

    // Test 5: Query all at once (limit=10) - should return all 5 with has_more=false
    let response =
        query_all_available_conversion_funds(deps.as_ref(), round_id, None, Some(10)).unwrap();
    assert_eq!(response.funds.len(), 5);
    assert!(!response.has_more);

    // Test 6: Verify default limit works (no limit specified)
    let response =
        query_all_available_conversion_funds(deps.as_ref(), round_id, None, None).unwrap();
    assert_eq!(response.funds.len(), 5);
    assert!(!response.has_more);

    // Test 7: Verify total_base_token_equivalent is correct for paginated results
    // Since these denoms are not recognized by any token info provider, ratios are 0
    // and base_token_equivalent should be 0
    let response =
        query_all_available_conversion_funds(deps.as_ref(), round_id, None, Some(3)).unwrap();
    assert_eq!(response.funds.len(), 3);
    assert_eq!(response.total_base_token_equivalent, Uint128::zero()); // All ratios are 0
}
