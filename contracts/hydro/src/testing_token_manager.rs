use std::{collections::HashMap, str::FromStr};

use cosmwasm_std::{
    testing::{mock_env, MockApi, MockStorage},
    to_json_vec, Binary, Coin, CosmosMsg, Decimal, Env, MsgResponse, OwnedDeps, Reply, Storage,
    SubMsgResponse, SubMsgResult, Uint128, WasmMsg,
};
use neutron_sdk::bindings::query::NeutronQuery;

use crate::{
    contract::{
        add_token_info_provider, execute, instantiate, query_token_info_providers,
        query_user_voting_power, remove_token_info_provider, reply,
    },
    msg::{ExecuteMsg, ProposalToLockups, ReplyPayload, TokenInfoProviderInstantiateMsg},
    state::{
        Constants, RoundLockPowerSchedule, CONSTANTS, PROPOSAL_MAP, PROPOSAL_TOTAL_MAP,
        TOKEN_INFO_PROVIDERS, TOTAL_VOTING_POWER_PER_ROUND, WHITELIST_ADMINS,
    },
    testing::{
        get_default_cw721_collection_info, get_default_instantiate_msg,
        get_default_lsm_token_info_provider_init_msg, get_default_power_schedule, get_message_info,
        setup_st_atom_token_info_provider_mock, IBC_DENOM_1, IBC_DENOM_2,
        ONE_MONTH_IN_NANO_SECONDS, ST_ATOM_ON_NEUTRON, ST_ATOM_TOKEN_GROUP, VALIDATOR_1,
        VALIDATOR_1_LST_DENOM_1, VALIDATOR_2, VALIDATOR_2_LST_DENOM_1,
    },
    testing_lsm_integration::set_validator_infos_for_round,
    testing_mocks::{
        denom_trace_grpc_query_mock, mock_dependencies, no_op_grpc_query_mock, MockQuerier,
    },
    token_manager::{
        TokenInfoProvider, TokenInfoProviderDerivative, TokenInfoProviderLSM,
        LSM_TOKEN_INFO_PROVIDER_ID,
    },
    utils::load_current_constants,
};

// This test verifies that Hydro contract can be instantiated with only LSM token info provider.
#[test]
fn instantiate_with_lsm_token_info_provider_test() {
    let (mut deps, env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());
    let info = get_message_info(&deps.api, "addr0000", &[]);

    let mut msg = get_default_instantiate_msg(&deps.api);

    let lsm_token_info_provider = TokenInfoProviderLSM {
        max_validator_shares_participating: 100,
        hub_connection_id: "connection-0".to_string(),
        hub_transfer_channel_id: "channel-0".to_string(),
        icq_update_period: 100,
    };

    msg.token_info_providers = vec![TokenInfoProviderInstantiateMsg::LSM {
        max_validator_shares_participating: lsm_token_info_provider
            .max_validator_shares_participating,
        hub_connection_id: lsm_token_info_provider.hub_connection_id.clone(),
        hub_transfer_channel_id: lsm_token_info_provider.hub_transfer_channel_id.clone(),
        icq_update_period: lsm_token_info_provider.icq_update_period,
    }];

    let res = instantiate(deps.as_mut(), env.clone(), info, msg.clone());
    assert!(res.is_ok());

    let res = query_token_info_providers(deps.as_ref());
    assert!(res.is_ok());

    let token_info_providers = res.unwrap().providers;
    assert_eq!(token_info_providers.len(), 1);

    match token_info_providers[0].clone() {
        TokenInfoProvider::Derivative(_) => {
            panic!("Expected LSM token provider, found derivative one.");
        }
        TokenInfoProvider::LSM(provider) => {
            assert_eq!(
                lsm_token_info_provider.hub_connection_id,
                provider.hub_connection_id
            );
            assert_eq!(
                lsm_token_info_provider.hub_transfer_channel_id,
                provider.hub_transfer_channel_id
            );
            assert_eq!(
                lsm_token_info_provider.icq_update_period,
                provider.icq_update_period
            );
            assert_eq!(
                lsm_token_info_provider.max_validator_shares_participating,
                provider.max_validator_shares_participating
            );
        }
    }
}

// This test verifies that Hydro contract can be instantiated with only one smart contract token info provider.
#[test]
fn instantiate_with_derivative_token_info_provider_test() {
    let (mut deps, env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());
    let info = get_message_info(&deps.api, "addr0000", &[]);

    let mut msg = get_default_instantiate_msg(&deps.api);

    let init_code_id = 7;
    let init_msg = Binary::new(vec![1, 3, 5, 7, 9]);
    let init_label = String::from("stATOM Token Info Provider");
    let init_admin = None;

    msg.token_info_providers = vec![TokenInfoProviderInstantiateMsg::TokenInfoProviderContract {
        code_id: init_code_id,
        msg: init_msg.clone(),
        label: init_label.clone(),
        admin: init_admin.clone(),
    }];

    let res = instantiate(deps.as_mut(), env.clone(), info, msg.clone());
    assert!(res.is_ok());

    let submsgs = res.unwrap().messages;
    assert_eq!(submsgs.len(), 1);

    match submsgs[0].msg.clone() {
        CosmosMsg::Wasm(wasm_msg) => match wasm_msg {
            WasmMsg::Instantiate {
                admin,
                code_id,
                msg,
                funds,
                label,
            } => {
                assert_eq!(code_id, init_code_id);
                assert_eq!(msg, init_msg);
                assert_eq!(admin, init_admin);
                assert_eq!(label, init_label);
                assert_eq!(funds.len(), 0);
            }
            _ => panic!("Unexpected Wasm message type!"),
        },
        _ => panic!("Unexpected SubMsg type!"),
    }

    let res = query_token_info_providers(deps.as_ref());
    assert!(res.is_ok());

    // Token info provider smart contract isn't instantiated yet, so it is expected not to have any in the store
    let token_info_providers = res.unwrap().providers;
    assert_eq!(token_info_providers.len(), 0);
}

// This test verifies that at least one token info provider must be specified on Hydro contract instantiation.
#[test]
fn instantiate_without_token_info_providers_test() {
    let (mut deps, env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());
    let info = get_message_info(&deps.api, "addr0000", &[]);

    let mut msg = get_default_instantiate_msg(&deps.api);
    msg.token_info_providers = vec![];

    let res = instantiate(deps.as_mut(), env.clone(), info, msg.clone());
    assert!(res.is_err());

    assert!(res
        .unwrap_err()
        .to_string()
        .to_lowercase()
        .contains("at least one token info provider must be specifed."));
}

// This test verifies that Hydro contract cannot be instantiated with multiple LSM token info providers.
#[test]
fn instantiate_with_multiple_lsm_token_info_providers_test() {
    let (mut deps, env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());
    let info = get_message_info(&deps.api, "addr0000", &[]);

    let mut msg = get_default_instantiate_msg(&deps.api);

    let lsm_token_info_provider = TokenInfoProviderLSM {
        max_validator_shares_participating: 100,
        hub_connection_id: "connection-0".to_string(),
        hub_transfer_channel_id: "channel-0".to_string(),
        icq_update_period: 100,
    };

    msg.token_info_providers = vec![
        TokenInfoProviderInstantiateMsg::LSM {
            max_validator_shares_participating: lsm_token_info_provider
                .max_validator_shares_participating,
            hub_connection_id: lsm_token_info_provider.hub_connection_id.clone(),
            hub_transfer_channel_id: lsm_token_info_provider.hub_transfer_channel_id.clone(),
            icq_update_period: lsm_token_info_provider.icq_update_period,
        },
        TokenInfoProviderInstantiateMsg::LSM {
            max_validator_shares_participating: lsm_token_info_provider
                .max_validator_shares_participating,
            hub_connection_id: lsm_token_info_provider.hub_connection_id.clone(),
            hub_transfer_channel_id: lsm_token_info_provider.hub_transfer_channel_id.clone(),
            icq_update_period: lsm_token_info_provider.icq_update_period,
        },
    ];

    let res = instantiate(deps.as_mut(), env.clone(), info, msg.clone());
    assert!(res.is_err());

    assert!(res
        .unwrap_err()
        .to_string()
        .to_lowercase()
        .contains("only one lsm token info provider can be used."));
}

// In case of the smart contract token info provider is instanatiated, verifies that the Hydro storage
// is updated as expected upon receiving the SubMsg response.
#[test]
fn handle_token_info_provider_instantiate_reply_test() {
    let (mut deps, env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());

    let constants = Constants {
        round_length: ONE_MONTH_IN_NANO_SECONDS,
        lock_epoch_length: ONE_MONTH_IN_NANO_SECONDS,
        first_round_start: env.block.time,
        max_locked_tokens: 50000,
        known_users_cap: 0,
        paused: false,
        max_deployment_duration: 3,
        round_lock_power_schedule: get_default_power_schedule(),
        cw721_collection_info: get_default_cw721_collection_info(),
    };
    CONSTANTS
        .save(&mut deps.storage, env.block.time.nanos(), &constants)
        .unwrap();

    let token_info_provider = TokenInfoProvider::Derivative(TokenInfoProviderDerivative {
        contract: String::new(),
        cache: HashMap::new(),
    });

    let contract_address = deps.api.addr_make("token_info_provider_1");
    setup_st_atom_token_info_provider_mock(&mut deps, contract_address.clone(), Decimal::one());

    let mut encoded_data = vec![];
    prost::encoding::string::encode(1, &contract_address.to_string(), &mut encoded_data);

    let reply_msg = Reply {
        id: 0,
        gas_used: 0,
        payload: Binary::new(
            to_json_vec(&ReplyPayload::InstantiateTokenInfoProvider(
                token_info_provider,
            ))
            .unwrap(),
        ),
        // `data` field is deprecated, but it must be set because otherwise the compiler gives an error
        #[allow(deprecated)]
        result: SubMsgResult::Ok(SubMsgResponse {
            events: vec![],
            msg_responses: vec![MsgResponse {
                type_url: String::new(), // not used in the test
                value: Binary::from(encoded_data),
            }],
            data: None,
        }),
    };

    let res = reply(deps.as_mut(), env, reply_msg);
    assert!(res.is_ok());

    let res = TOKEN_INFO_PROVIDERS.load(&deps.storage, contract_address.to_string());
    assert!(
        res.is_ok(),
        "expect token info provider not found in the store"
    );

    match res.unwrap() {
        TokenInfoProvider::LSM(_) => {
            panic!("expected derivative token info provider, found LSM one.")
        }
        TokenInfoProvider::Derivative(provider) => {
            assert_eq!(provider.contract, contract_address.to_string());
        }
    }
}

// Tests that the token info providers can be added and removed during the Hydro smart contract lifecycle.
#[test]
fn add_remove_token_info_provider_test() {
    let (mut deps, env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());
    let info = get_message_info(&deps.api, "addr0000", &[]);

    let lsm_token_info_provider = TokenInfoProviderLSM {
        max_validator_shares_participating: 100,
        hub_connection_id: "connection-0".to_string(),
        hub_transfer_channel_id: "channel-0".to_string(),
        icq_update_period: 100,
    };

    CONSTANTS
        .save(
            &mut deps.storage,
            env.block.height,
            &Constants {
                round_length: ONE_MONTH_IN_NANO_SECONDS,
                lock_epoch_length: ONE_MONTH_IN_NANO_SECONDS,
                first_round_start: env.block.time,
                max_locked_tokens: 50000000,
                known_users_cap: 0,
                paused: false,
                max_deployment_duration: 3,
                round_lock_power_schedule: RoundLockPowerSchedule::new(vec![]),
                cw721_collection_info: get_default_cw721_collection_info(),
            },
        )
        .unwrap();

    WHITELIST_ADMINS
        .save(&mut deps.storage, &vec![info.sender.clone()])
        .unwrap();

    // Initially save only the LSM token info provider
    TOKEN_INFO_PROVIDERS
        .save(
            &mut deps.storage,
            LSM_TOKEN_INFO_PROVIDER_ID.to_string(),
            &TokenInfoProvider::LSM(lsm_token_info_provider.clone()),
        )
        .unwrap();

    // Try to add one more LSM token info provider and validate there can't be multiple of its type
    let new_provider_info = TokenInfoProviderInstantiateMsg::LSM {
        max_validator_shares_participating: 100,
        hub_connection_id: "connection-0".to_string(),
        hub_transfer_channel_id: "channel-0".to_string(),
        icq_update_period: 100,
    };

    let constants = load_current_constants(&deps.as_ref(), &env).unwrap();
    let res = add_token_info_provider(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        &constants,
        new_provider_info,
    );
    assert!(
        res.is_err(),
        "having multiple LSM token info providers shouldn't be allowed"
    );
    assert!(res
        .unwrap_err()
        .to_string()
        .to_lowercase()
        .contains("only one lsm token info provider can be used."));

    // Verify that a new Derivative token info provider can be added
    let init_code_id = 7;
    let init_msg = Binary::new(vec![1, 3, 5, 7, 9]);
    let init_label = String::from("stATOM Token Info Provider");
    let init_admin = None;

    let new_provider_info = TokenInfoProviderInstantiateMsg::TokenInfoProviderContract {
        code_id: init_code_id,
        msg: init_msg.clone(),
        label: init_label.clone(),
        admin: init_admin.clone(),
    };

    let constants = load_current_constants(&deps.as_ref(), &env).unwrap();
    let res = add_token_info_provider(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        &constants,
        new_provider_info,
    );
    assert!(
        res.is_ok(),
        "failed to add new Derivative token info provider"
    );

    let result_submsgs = res.unwrap().messages;
    assert_eq!(
        result_submsgs.len(),
        1,
        "expected one submsg in response, to instantitate a new token info provider contract"
    );

    match result_submsgs[0].clone().msg {
        CosmosMsg::Wasm(wasm_msg) => match wasm_msg {
            WasmMsg::Instantiate {
                admin,
                code_id,
                msg,
                funds,
                label,
            } => {
                assert_eq!(code_id, init_code_id);
                assert_eq!(msg, init_msg);
                assert_eq!(admin, init_admin);
                assert_eq!(label, init_label);
                assert_eq!(funds.len(), 0);
            }
            _ => panic!("Unexpected Wasm message type"),
        },
        _ => panic!("Unexpected SubMsg type"),
    }

    // Add one more provider into the store manually, in order to test the removal
    let contract_address = deps.api.addr_make("token_info_provider_1");
    TOKEN_INFO_PROVIDERS
        .save(
            &mut deps.storage,
            contract_address.to_string(),
            &TokenInfoProvider::Derivative(TokenInfoProviderDerivative {
                contract: contract_address.to_string(),
                cache: HashMap::new(),
            }),
        )
        .unwrap();

    setup_st_atom_token_info_provider_mock(&mut deps, contract_address.clone(), Decimal::one());

    assert_eq!(
        query_token_info_providers(deps.as_ref())
            .unwrap()
            .providers
            .len(),
        2
    );

    // Remove the newly added smart contract token info provider
    let constants = load_current_constants(&deps.as_ref(), &env).unwrap();
    let res = remove_token_info_provider(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        &constants,
        contract_address.to_string(),
    );
    assert!(res.is_ok(), "failed to remove token info provider");

    assert!(!TOKEN_INFO_PROVIDERS.has(&deps.storage, contract_address.to_string()));

    // Remove LSM token info provider
    let res = remove_token_info_provider(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        &constants,
        LSM_TOKEN_INFO_PROVIDER_ID.to_string(),
    );
    assert!(res.is_ok(), "failed to remove LSM token info provider.");

    assert_eq!(
        query_token_info_providers(deps.as_ref())
            .unwrap()
            .providers
            .len(),
        0
    );
}

// This test verifies that the proposal powers and round total powers are updated as expected in following cases:
//  1) When a token info provider executes transaction to update token group ratio in Hydro contract
//  2) When a token info provider is removed from the list of all token info providers held by Hydro contract
//  3) When a new token info provider is added to the Hydro contract
// Test also verifies that users voting power is updated when token group ratio gets changed, or the
// token info providers are added and removed.
#[test]
fn token_info_provider_lifecycle_test() {
    let user_address = "addr0000";
    let token_info_provider = "token_info_provider_1";

    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([
            (IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string()),
            (IBC_DENOM_2.to_string(), VALIDATOR_2_LST_DENOM_1.to_string()),
        ]),
    );

    let (mut deps, mut env) = (mock_dependencies(grpc_query), mock_env());

    let admin_info = get_message_info(&deps.api, user_address, &[]);
    let token_info_provider_addr = deps.api.addr_make(token_info_provider);

    let mut init_msg = get_default_instantiate_msg(&deps.api);
    init_msg.round_length = ONE_MONTH_IN_NANO_SECONDS;
    init_msg
        .whitelist_admins
        .push(admin_info.sender.to_string());

    let res = instantiate(
        deps.as_mut(),
        env.clone(),
        admin_info.clone(),
        init_msg.clone(),
    );
    assert!(res.is_ok());

    // Sets the token power ratio to 1 for each validator
    set_validator_infos_for_round(
        &mut deps.storage,
        0,
        vec![VALIDATOR_1.to_string(), VALIDATOR_2.to_string()],
    )
    .unwrap();

    // Sets the stATOM token group ratio to 1.5
    setup_st_atom_token_info_provider_mock(
        &mut deps,
        token_info_provider_addr.clone(),
        Decimal::from_str("1.5").unwrap(),
    );

    env.block.time = env.block.time.plus_days(1);

    let first_round_id = 0;
    let second_round_id = 1;
    let tranche_id = 1;
    let first_proposal_id = 0;
    let second_proposal_id = 1;

    // Create two proposals
    let msg = ExecuteMsg::CreateProposal {
        round_id: None,
        tranche_id,
        title: "proposal 1".to_string(),
        description: "proposal 1 desc".to_string(),
        deployment_duration: 1,
        minimum_atom_liquidity_request: Uint128::zero(),
    };
    let res = execute(deps.as_mut(), env.clone(), admin_info.clone(), msg);
    assert!(res.is_ok());

    let msg = ExecuteMsg::CreateProposal {
        round_id: None,
        tranche_id,
        title: "proposal 2".to_string(),
        description: "proposal 2 desc".to_string(),
        deployment_duration: 1,
        minimum_atom_liquidity_request: Uint128::zero(),
    };
    let res = execute(deps.as_mut(), env.clone(), admin_info.clone(), msg);
    assert!(res.is_ok());

    // Have user lock some tokens for 2 rounds
    for token_to_lock in [
        Coin::new(3000u128, IBC_DENOM_1),
        Coin::new(5000u128, IBC_DENOM_2),
        Coin::new(1000u128, ST_ATOM_ON_NEUTRON),
        Coin::new(1000u128, IBC_DENOM_1),
        Coin::new(2000u128, IBC_DENOM_2),
        Coin::new(4000u128, ST_ATOM_ON_NEUTRON),
    ] {
        let locking_info = get_message_info(&deps.api, user_address, &[token_to_lock]);
        let msg = ExecuteMsg::LockTokens {
            lock_duration: init_msg.lock_epoch_length * 2,
            proof: None,
        };

        let res = execute(deps.as_mut(), env.clone(), locking_info.clone(), msg);
        assert!(res.is_ok());
    }

    verify_current_user_voting_power(&deps, env.clone(), user_address, 23125);

    // Have user vote for both proposals with all of its lockups
    let voting_info = get_message_info(&deps.api, user_address, &[]);
    let msg = ExecuteMsg::Vote {
        tranche_id,
        proposals_votes: vec![
            ProposalToLockups {
                proposal_id: first_proposal_id,
                lock_ids: vec![0, 1, 2],
            },
            ProposalToLockups {
                proposal_id: second_proposal_id,
                lock_ids: vec![3, 4, 5],
            },
        ],
    };

    let res = execute(deps.as_mut(), env.clone(), voting_info.clone(), msg);
    assert!(res.is_ok());

    verify_proposals_and_rounds_powers(
        &deps.storage,
        first_round_id,
        tranche_id,
        &[(first_proposal_id, 11875), (second_proposal_id, 11250)],
        &[(first_round_id, 23125), (second_round_id, 18500)],
    );

    // Mock that stATOM token group ratio changed to 1.6
    setup_st_atom_token_info_provider_mock(
        &mut deps,
        token_info_provider_addr.clone(),
        Decimal::from_str("1.6").unwrap(),
    );

    // Update stATOM token ratio, which should update all proposal powers and round total powers
    let msg = ExecuteMsg::UpdateTokenGroupRatio {
        token_group_id: ST_ATOM_TOKEN_GROUP.to_string(),
        old_ratio: Decimal::from_str("1.5").unwrap(),
        new_ratio: Decimal::from_str("1.6").unwrap(),
    };

    let info = get_message_info(&deps.api, token_info_provider, &[]);
    let res = execute(deps.as_mut(), env.clone(), info, msg);
    assert!(res.is_ok());

    verify_proposals_and_rounds_powers(
        &deps.storage,
        first_round_id,
        tranche_id,
        &[(first_proposal_id, 12000), (second_proposal_id, 11750)],
        &[(first_round_id, 23750), (second_round_id, 19000)],
    );

    verify_current_user_voting_power(&deps, env.clone(), user_address, 23750);

    // Remove both stATOM and LSM token info provider. This should bring all proposal and round powers to zero
    for token_provider in [
        token_info_provider_addr.to_string(),
        LSM_TOKEN_INFO_PROVIDER_ID.to_string(),
    ] {
        let msg = ExecuteMsg::RemoveTokenInfoProvider {
            provider_id: token_provider,
        };

        let res = execute(deps.as_mut(), env.clone(), admin_info.clone(), msg);
        assert!(res.is_ok());
    }

    verify_proposals_and_rounds_powers(
        &deps.storage,
        first_round_id,
        tranche_id,
        &[(first_proposal_id, 0), (second_proposal_id, 0)],
        &[(first_round_id, 0), (second_round_id, 0)],
    );

    verify_current_user_voting_power(&deps, env.clone(), user_address, 0);

    // Re-add both token info providers and verify the voting powers again.
    let constants = load_current_constants(&deps.as_ref(), &env).unwrap();
    add_token_info_provider(
        deps.as_mut(),
        env.clone(),
        admin_info.clone(),
        &constants,
        get_default_lsm_token_info_provider_init_msg(),
    )
    .unwrap();

    let add_provider_res = add_token_info_provider(
        deps.as_mut(),
        env.clone(),
        admin_info.clone(),
        &constants,
        TokenInfoProviderInstantiateMsg::TokenInfoProviderContract {
            code_id: 1000,
            msg: Binary::new(vec![]),
            label: String::new(),
            admin: None,
        },
    )
    .unwrap();
    assert_eq!(add_provider_res.messages.len(), 1);

    let mut encoded_data = vec![];
    prost::encoding::string::encode(1, &token_info_provider_addr.to_string(), &mut encoded_data);

    let reply_msg = Reply {
        id: 0,
        gas_used: 0,
        payload: add_provider_res.messages[0].clone().payload,
        // `data` field is deprecated, but it must be set because otherwise the compiler gives an error
        #[allow(deprecated)]
        result: SubMsgResult::Ok(SubMsgResponse {
            events: vec![],
            msg_responses: vec![MsgResponse {
                type_url: String::new(), // not used in the test
                value: Binary::from(encoded_data),
            }],
            data: None,
        }),
    };

    reply(deps.as_mut(), env.clone(), reply_msg).unwrap();

    verify_proposals_and_rounds_powers(
        &deps.storage,
        first_round_id,
        tranche_id,
        &[(first_proposal_id, 12000), (second_proposal_id, 11750)],
        &[(first_round_id, 23750), (second_round_id, 19000)],
    );

    verify_current_user_voting_power(&deps, env.clone(), user_address, 23750);

    // Remove contract token info provider again, advance the chain by three rounds and unlock tokens
    let res = execute(
        deps.as_mut(),
        env.clone(),
        admin_info.clone(),
        ExecuteMsg::RemoveTokenInfoProvider {
            provider_id: token_info_provider_addr.to_string(),
        },
    );
    assert!(res.is_ok());

    env.block.time = env.block.time.plus_nanos(init_msg.lock_epoch_length * 3);

    // 2 and 5 are the lock IDs of previously created stATOM lockups
    let res = execute(
        deps.as_mut(),
        env.clone(),
        admin_info.clone(),
        ExecuteMsg::UnlockTokens {
            lock_ids: Some(vec![2, 5]),
        },
    );
    assert!(res.is_ok());
}

fn verify_proposals_and_rounds_powers(
    storage: &dyn Storage,
    proposals_round_id: u64,
    proposals_tranche_id: u64,
    expected_proposal_powers: &[(u64, u128)],
    expected_round_powers: &[(u64, u128)],
) {
    for expected_proposal_power in expected_proposal_powers {
        let proposal = PROPOSAL_MAP
            .load(
                storage,
                (
                    proposals_round_id,
                    proposals_tranche_id,
                    expected_proposal_power.0,
                ),
            )
            .unwrap();
        assert_eq!(proposal.power.u128(), expected_proposal_power.1);

        assert_eq!(
            PROPOSAL_TOTAL_MAP
                .load(storage, expected_proposal_power.0)
                .unwrap()
                .to_uint_ceil()
                .u128(),
            expected_proposal_power.1
        );
    }

    for expected_round_power in expected_round_powers {
        assert_eq!(
            TOTAL_VOTING_POWER_PER_ROUND
                .load(storage, expected_round_power.0)
                .unwrap()
                .u128(),
            expected_round_power.1
        );
    }
}

fn verify_current_user_voting_power(
    deps: &OwnedDeps<MockStorage, MockApi, MockQuerier, NeutronQuery>,
    env: Env,
    user_address: &str,
    expected_power: u128,
) {
    let user_voting_power = query_user_voting_power(
        deps.as_ref(),
        env,
        deps.api.addr_make(user_address).to_string(),
    )
    .unwrap();
    assert_eq!(user_voting_power.voting_power, expected_power);
}
