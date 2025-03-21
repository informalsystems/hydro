use std::collections::HashMap;

use cosmwasm_std::{
    testing::mock_env, to_json_vec, Binary, CosmosMsg, MsgResponse, Reply, SubMsgResponse,
    SubMsgResult, WasmMsg,
};

use crate::{
    contract::{
        add_token_info_provider, instantiate, query_token_info_providers,
        remove_token_info_provider, reply,
    },
    msg::{ReplyPayload, TokenInfoProviderInstantiateMsg},
    state::{Constants, RoundLockPowerSchedule, CONSTANTS, TOKEN_INFO_PROVIDERS, WHITELIST_ADMINS},
    testing::{get_default_instantiate_msg, get_message_info},
    testing_mocks::{mock_dependencies, no_op_grpc_query_mock},
    token_manager::{
        TokenInfoProvider, TokenInfoProviderDerivative, TokenInfoProviderLSM,
        LSM_TOKEN_INFO_PROVIDER_ID,
    },
};

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

#[test]
fn handle_token_info_provider_instantiate_reply_test() {
    let (mut deps, env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());

    let token_info_provider = TokenInfoProvider::Derivative(TokenInfoProviderDerivative {
        contract: String::new(),
        cache: HashMap::new(),
    });

    let contract_address = deps.api.addr_make("token_info_provider_1").to_string();

    let mut encoded_data = vec![];
    prost::encoding::string::encode(1, &contract_address, &mut encoded_data);

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

    let res = TOKEN_INFO_PROVIDERS.load(&deps.storage, contract_address.clone());
    assert!(
        res.is_ok(),
        "expect token info provider not found in the store"
    );

    match res.unwrap() {
        TokenInfoProvider::LSM(_) => {
            panic!("expected derivative token info provider, found LSM one.")
        }
        TokenInfoProvider::Derivative(provider) => {
            assert_eq!(provider.contract, contract_address);
        }
    }
}

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
                round_length: 0,
                lock_epoch_length: 0,
                first_round_start: env.block.time,
                max_locked_tokens: 0,
                known_users_cap: 0,
                paused: false,
                max_deployment_duration: 0,
                round_lock_power_schedule: RoundLockPowerSchedule::new(vec![]),
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

    let res = add_token_info_provider(deps.as_mut(), env.clone(), info.clone(), new_provider_info);
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

    let res = add_token_info_provider(deps.as_mut(), env.clone(), info.clone(), new_provider_info);
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
    let contract_address = deps.api.addr_make("token_info_provider_1").to_string();
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

    assert_eq!(
        query_token_info_providers(deps.as_ref())
            .unwrap()
            .providers
            .len(),
        2
    );

    // Remove the newly added smart contract token info provider
    let res = remove_token_info_provider(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        contract_address.clone(),
    );
    assert!(res.is_ok(), "failed to remove token info provider");

    assert!(!TOKEN_INFO_PROVIDERS.has(&deps.storage, contract_address.clone()));

    // Remove LSM token info provider
    let res = remove_token_info_provider(
        deps.as_mut(),
        env.clone(),
        info.clone(),
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
