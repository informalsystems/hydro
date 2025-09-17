use std::collections::HashSet;

use cosmwasm_std::{testing::mock_env, CosmosMsg, Decimal, Order, Uint128};
use interface::lsm::ValidatorInfo;
use neutron_sdk::bindings::msg::NeutronMsg;

use crate::{
    contract::{execute, instantiate},
    msg::ExecuteMsg,
    state::{
        ICQ_MANAGERS, QUERY_ID_TO_VALIDATOR, VALIDATORS_INFO, VALIDATORS_PER_ROUND,
        VALIDATORS_STORE_INITIALIZED, VALIDATOR_TO_QUERY_ID,
    },
    testing::{
        get_default_instantiate_msg, get_message_info, VALIDATOR_1, VALIDATOR_2, VALIDATOR_3,
    },
    testing_mocks::{mock_dependencies, no_op_grpc_query_mock},
};

#[test]
fn remove_icqs_test() {
    let instantiate_address_str = "addr0000";
    let icq_manager_address_str = "addr0001";
    let non_icq_manager_address_str = "addr0002";

    let grpc_query = no_op_grpc_query_mock();
    let (mut deps, env) = (mock_dependencies(grpc_query), mock_env());

    let msg = get_default_instantiate_msg(&deps.api);
    let info = get_message_info(&deps.api, instantiate_address_str, &[]);

    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());

    let icq_manager_info = get_message_info(&deps.api, icq_manager_address_str, &[]);
    let non_icq_manager_info = get_message_info(&deps.api, non_icq_manager_address_str, &[]);

    // Set one ICQ manager in the store in order to test permissioned execution
    ICQ_MANAGERS
        .save(&mut deps.storage, icq_manager_info.sender.clone(), &true)
        .unwrap();

    // Setup ICQ specific stores
    let icq_id_1 = 153;
    let icq_id_2 = 154;
    let icq_id_3 = 155;
    let non_existing_icq_id_1 = 273;

    for validator_to_id in [
        (&VALIDATOR_1.to_string(), icq_id_1),
        (&VALIDATOR_2.to_string(), icq_id_2),
        (&VALIDATOR_3.to_string(), icq_id_3),
    ] {
        QUERY_ID_TO_VALIDATOR
            .save(&mut deps.storage, validator_to_id.1, validator_to_id.0)
            .unwrap();
        VALIDATOR_TO_QUERY_ID
            .save(
                &mut deps.storage,
                validator_to_id.0.clone(),
                &validator_to_id.1,
            )
            .unwrap();
    }

    // Unauthorized user tries to remove ICQs
    let msg = ExecuteMsg::RemoveInterchainQueries {
        query_ids: vec![icq_id_1, icq_id_2, icq_id_3],
    };
    let res = execute(
        deps.as_mut(),
        env.clone(),
        non_icq_manager_info.clone(),
        msg,
    );
    assert!(res.unwrap_err().to_string().contains("Unauthorized"));

    // ICQ manager removes some valid, some non-existing and some duplicate query IDs
    let expected_removed_queries: HashSet<u64> = HashSet::from_iter([icq_id_1, icq_id_2]);

    let msg = ExecuteMsg::RemoveInterchainQueries {
        query_ids: vec![
            icq_id_1,
            icq_id_2,
            icq_id_1,
            icq_id_2,
            non_existing_icq_id_1,
        ],
    };

    let res = execute(deps.as_mut(), env.clone(), icq_manager_info.clone(), msg).unwrap();
    assert_eq!(res.messages.len(), expected_removed_queries.len());

    for message in res.messages {
        match message.msg {
            CosmosMsg::Custom(NeutronMsg::RemoveInterchainQuery { query_id }) => {
                assert!(expected_removed_queries.contains(&query_id));
            }
            _ => {
                panic!("unexpected msg type");
            }
        }
    }

    assert!(!QUERY_ID_TO_VALIDATOR.has(&deps.storage, icq_id_1));
    assert!(!QUERY_ID_TO_VALIDATOR.has(&deps.storage, icq_id_2));
    assert!(QUERY_ID_TO_VALIDATOR.has(&deps.storage, icq_id_3));

    assert!(!VALIDATOR_TO_QUERY_ID.has(&deps.storage, VALIDATOR_1.to_string()));
    assert!(!VALIDATOR_TO_QUERY_ID.has(&deps.storage, VALIDATOR_2.to_string()));
    assert!(VALIDATOR_TO_QUERY_ID.has(&deps.storage, VALIDATOR_3.to_string()));

    // Remove the remaining ICQ
    let expected_removed_queries: HashSet<u64> = HashSet::from_iter([icq_id_3]);

    let msg = ExecuteMsg::RemoveInterchainQueries {
        query_ids: vec![icq_id_3, icq_id_3, icq_id_1, icq_id_2],
    };

    let res = execute(deps.as_mut(), env.clone(), icq_manager_info.clone(), msg).unwrap();
    assert_eq!(res.messages.len(), expected_removed_queries.len());

    for message in res.messages {
        match message.msg {
            CosmosMsg::Custom(NeutronMsg::RemoveInterchainQuery { query_id }) => {
                assert!(expected_removed_queries.contains(&query_id));
            }
            _ => {
                panic!("unexpected msg type");
            }
        }
    }

    assert!(!QUERY_ID_TO_VALIDATOR.has(&deps.storage, icq_id_3));
    assert!(!VALIDATOR_TO_QUERY_ID.has(&deps.storage, VALIDATOR_3.to_string()));
}

#[test]
fn remove_round_validators_data_test() {
    let instantiate_address_str = "addr0000";
    let icq_manager_address_str = "addr0001";
    let non_icq_manager_address_str = "addr0002";

    let grpc_query = no_op_grpc_query_mock();
    let (mut deps, env) = (mock_dependencies(grpc_query), mock_env());

    let msg = get_default_instantiate_msg(&deps.api);
    let info = get_message_info(&deps.api, instantiate_address_str, &[]);

    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone());
    assert!(res.is_ok());

    let icq_manager_info = get_message_info(&deps.api, icq_manager_address_str, &[]);
    let non_icq_manager_info = get_message_info(&deps.api, non_icq_manager_address_str, &[]);

    // Set one ICQ manager in the store in order to test permissioned execution
    ICQ_MANAGERS
        .save(&mut deps.storage, icq_manager_info.sender.clone(), &true)
        .unwrap();

    let round_id_1 = 0;
    let round_id_2 = 1;
    let round_id_3 = 2;

    let dummy_val_tokens = 53100u128;

    for round_setup_data in [
        (
            round_id_1,
            vec![
                VALIDATOR_1.to_string(),
                VALIDATOR_2.to_string(),
                VALIDATOR_3.to_string(),
            ],
        ),
        (
            round_id_2,
            vec![
                VALIDATOR_1.to_string(),
                VALIDATOR_2.to_string(),
                VALIDATOR_3.to_string(),
            ],
        ),
        (
            round_id_3,
            vec![VALIDATOR_1.to_string(), VALIDATOR_2.to_string()],
        ),
    ] {
        VALIDATORS_STORE_INITIALIZED
            .save(&mut deps.storage, round_setup_data.0, &true)
            .unwrap();

        for validator in round_setup_data.1 {
            VALIDATORS_PER_ROUND
                .save(
                    &mut deps.storage,
                    (round_setup_data.0, dummy_val_tokens, validator.clone()),
                    &validator,
                )
                .unwrap();

            VALIDATORS_INFO
                .save(
                    &mut deps.storage,
                    (round_setup_data.0, validator.clone()),
                    &ValidatorInfo {
                        address: validator.clone(),
                        delegated_tokens: Uint128::new(dummy_val_tokens),
                        power_ratio: Decimal::one(),
                    },
                )
                .unwrap();
        }
    }

    // Unauthorized user tries to remove round validators data
    let msg = ExecuteMsg::RemoveRoundValidatorsData { round_id: 0 };
    let res = execute(
        deps.as_mut(),
        env.clone(),
        non_icq_manager_info.clone(),
        msg,
    );
    assert!(res.unwrap_err().to_string().contains("Unauthorized"));

    // ICQ manager removes round 0 data
    let msg = ExecuteMsg::RemoveRoundValidatorsData {
        round_id: round_id_1,
    };
    execute(deps.as_mut(), env.clone(), icq_manager_info.clone(), msg).unwrap();

    assert!(!VALIDATORS_STORE_INITIALIZED.has(&deps.storage, round_id_1));
    assert_eq!(
        VALIDATORS_PER_ROUND
            .sub_prefix(round_id_1)
            .range(&deps.storage, None, None, Order::Ascending)
            .count(),
        0
    );
    assert_eq!(
        VALIDATORS_INFO
            .prefix(round_id_1)
            .range(&deps.storage, None, None, Order::Ascending)
            .count(),
        0
    );
}
