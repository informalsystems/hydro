use cosmwasm_std::{attr, from_json, Binary, Response, Uint128};
use cw_utils::Expiration;
use neutron_sdk::bindings::msg::NeutronMsg;
use std::collections::HashMap;

use crate::{
    contract::{
        execute, instantiate, query, query_all_votes, query_specific_user_lockups,
        MIN_DEPLOYMENT_DURATION,
    },
    cw721::{query_approval, query_collection_info, query_nft_info, query_owner_of},
    msg::{CollectionInfo, ExecuteMsg, ProposalToLockups, ReceiverExecuteMsg},
    query::{NumTokensResponse, OperatorsResponse, QueryMsg, TokensResponse},
    state::NFT_APPROVALS,
    testing::{
        get_address_as_str, get_default_instantiate_msg, get_message_info,
        set_default_validator_for_rounds, setup_contract_info_mock,
        setup_st_atom_token_info_provider_mock, IBC_DENOM_1, ONE_MONTH_IN_NANO_SECONDS,
        ST_ATOM_ON_NEUTRON, ST_ATOM_ON_STRIDE, VALIDATOR_1_LST_DENOM_1,
    },
    testing_mocks::{denom_trace_grpc_query_mock, mock_dependencies, no_op_grpc_query_mock},
};

use cosmwasm_std::testing::mock_env;
use cosmwasm_std::{Coin, Decimal};

#[test]
fn test_handle_execute_transfer_lsm_fail() {
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([
            (IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string()),
            (
                ST_ATOM_ON_NEUTRON.to_string(),
                ST_ATOM_ON_STRIDE.to_string(),
            ),
        ]),
    );

    // Setup initial state
    let (mut deps, env) = (mock_dependencies(grpc_query), mock_env());

    let owner = "owner";
    let info = get_message_info(&deps.api, owner, &[]);

    // Proper contract initialization
    let msg = get_default_instantiate_msg(&deps.api);
    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok(), "Failed to instantiate contract: {:?}", res);

    // Set up validators for rounds
    set_default_validator_for_rounds(deps.as_mut(), 0, 100);

    // Lock tokens first to create a lock entry
    let lock_info = get_message_info(
        &deps.api,
        owner,
        &[Coin::new(1000u64, IBC_DENOM_1.to_string())],
    );
    let lock_msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };
    let lock_res = execute(deps.as_mut(), env.clone(), lock_info, lock_msg);
    assert!(lock_res.is_ok(), "Failed to lock tokens: {:?}", lock_res);

    let recipient = get_address_as_str(&deps.api, "recipient");
    let token_id = "0".to_string(); // First lock ID is 0
    let execute_msg = ExecuteMsg::TransferNft {
        recipient: recipient.clone(),
        token_id: token_id.clone(),
    };

    // Execute the message using the contract's execute function
    let res = execute(deps.as_mut(), env.clone(), info.clone(), execute_msg);

    // Verify the response
    assert!(
        res.is_err(),
        "Should not be able to transfer LSM lockups {:?}",
        res
    );
    assert!(res
        .unwrap_err()
        .to_string()
        .to_lowercase()
        .contains("cannot transfer lsm lockups"));
}

#[test]
fn test_handle_execute_transfer_st_atom_success() {
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([
            (IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string()),
            (
                ST_ATOM_ON_NEUTRON.to_string(),
                ST_ATOM_ON_STRIDE.to_string(),
            ),
        ]),
    );

    // Setup initial state
    let (mut deps, env) = (mock_dependencies(grpc_query), mock_env());

    let owner = "owner";
    let info = get_message_info(&deps.api, owner, &[]);

    // Proper contract initialization
    let msg = get_default_instantiate_msg(&deps.api);
    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok(), "Failed to instantiate contract: {:?}", res);

    // Setup ST_ATOM token info provider
    let token_info_provider_addr = deps.api.addr_make("token_info_provider_1");
    setup_st_atom_token_info_provider_mock(&mut deps, token_info_provider_addr, Decimal::one());

    // Lock tokens first to create a lock entry
    let lock_info = get_message_info(
        &deps.api,
        owner,
        &[Coin::new(1000u64, ST_ATOM_ON_NEUTRON.to_string())],
    );
    let lock_msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };
    let lock_res = execute(deps.as_mut(), env.clone(), lock_info, lock_msg);
    assert!(lock_res.is_ok(), "Failed to lock tokens: {:?}", lock_res);

    let recipient = get_address_as_str(&deps.api, "recipient");
    let token_id = "0".to_string(); // First lock ID is 0
    let execute_msg = ExecuteMsg::TransferNft {
        recipient: recipient.clone(),
        token_id: token_id.clone(),
    };

    // Execute the message using the contract's execute function
    let res = execute(deps.as_mut(), env.clone(), info.clone(), execute_msg);

    // Verify the response
    assert!(res.is_ok());
    let res = res.unwrap();

    // Check that action is set correctly
    assert!(res
        .attributes
        .iter()
        .any(|attr| attr.key == "action" && attr.value == "transfer_nft"));

    // Check that the owner does not have the lock anymore
    let res = query_specific_user_lockups(
        &deps.as_ref(),
        &env,
        info.sender.to_string(),
        vec![0], // Query only the first lockup
    );

    assert!(res.is_ok());
    let res = res.unwrap();
    assert_eq!(0, res.lockups.len());

    // Check that the new owner (recipient) has the lock
    let res = query_specific_user_lockups(
        &deps.as_ref(),
        &env,
        recipient.to_string(),
        vec![0], // Query only the first lockup
    );
    assert!(res.is_ok());
    let res = res.unwrap();
    assert_eq!(1, res.lockups.len());
    assert_eq!(0, res.lockups[0].lock_entry.lock_id);
    assert_eq!(1000, res.lockups[0].lock_entry.funds.amount.u128());
    assert_eq!(ST_ATOM_ON_NEUTRON, res.lockups[0].lock_entry.funds.denom);

    // Also check via the Owner Of query
    let res = query_owner_of(deps.as_ref(), env, token_id, None);
    assert!(res.is_ok());

    let res = res.unwrap();
    assert_eq!(res.owner, recipient.to_string());
}

#[test]
fn test_handle_execute_transfer_st_atom_with_vote_success() {
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([
            (IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string()),
            (
                ST_ATOM_ON_NEUTRON.to_string(),
                ST_ATOM_ON_STRIDE.to_string(),
            ),
        ]),
    );

    // Setup initial state
    let (mut deps, env) = (mock_dependencies(grpc_query), mock_env());

    let user_address = "addr0000";
    let info = get_message_info(&deps.api, user_address, &[]);

    // Proper contract initialization
    let msg = get_default_instantiate_msg(&deps.api);
    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok(), "Failed to instantiate contract: {:?}", res);

    // Setup ST_ATOM token info provider
    let token_info_provider_addr = deps.api.addr_make("token_info_provider_1");
    setup_st_atom_token_info_provider_mock(&mut deps, token_info_provider_addr, Decimal::one());

    // Lock tokens first to create a lock entry
    let lock_info = get_message_info(
        &deps.api,
        user_address,
        &[Coin::new(1000u64, ST_ATOM_ON_NEUTRON.to_string())],
    );
    let lock_msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };
    let lock_res = execute(deps.as_mut(), env.clone(), lock_info, lock_msg);
    assert!(lock_res.is_ok(), "Failed to lock tokens: {:?}", lock_res);

    // Create simple test proposal
    let proposal_msg = ExecuteMsg::CreateProposal {
        round_id: None,
        tranche_id: 1,
        title: "Test proposal".to_string(),
        description: "1 month deployment".to_string(),
        deployment_duration: MIN_DEPLOYMENT_DURATION,
        minimum_atom_liquidity_request: Uint128::zero(),
    };
    let proposal_res = execute(deps.as_mut(), env.clone(), info.clone(), proposal_msg);
    assert!(
        proposal_res.is_ok(),
        "Failed to create proposal: {:?}",
        proposal_res
    );

    // Vote on proposal
    let vote_msg = ExecuteMsg::Vote {
        tranche_id: 1,
        proposals_votes: vec![ProposalToLockups {
            proposal_id: 0,
            lock_ids: vec![0],
        }],
    };
    let vote_res = execute(deps.as_mut(), env.clone(), info.clone(), vote_msg);
    assert!(vote_res.is_ok(), "Failed to vote: {:?}", vote_res);

    // verify user's vote worked
    let vote_query_res = query_all_votes(deps.as_ref(), 0, 100);
    assert!(
        vote_query_res.is_ok(),
        "Vote query should not fail: {:?}",
        vote_query_res
    );
    let votes = vote_query_res.unwrap().votes;
    assert_eq!(1, votes.len());
    let vote = votes.first().unwrap();
    assert_eq!(vote.sender_addr, info.sender);
    assert_eq!(0, vote.lock_id);
    assert_eq!(0, vote.vote.prop_id);

    let recipient = get_address_as_str(&deps.api, "recipient");
    let token_id = "0".to_string(); // First lock ID is 0
    let transfer_msg = ExecuteMsg::TransferNft {
        recipient: recipient.clone(),
        token_id: token_id.clone(),
    };

    // Execute the message using the contract's execute function
    let transfer_res = execute(deps.as_mut(), env.clone(), info.clone(), transfer_msg);

    // Verify the response
    assert!(
        transfer_res.is_ok(),
        "Failed to transfer NFT: {:?}",
        transfer_res
    );
    let transfer_res = transfer_res.unwrap();

    // Check that action is set correctly
    assert!(transfer_res
        .attributes
        .iter()
        .any(|attr| attr.key == "action" && attr.value == "transfer_nft"));

    // Check that the owner does not have the lock anymore
    let res = query_specific_user_lockups(
        &deps.as_ref(),
        &env,
        info.sender.to_string(),
        vec![0], // Query only the first lockup
    );

    assert!(res.is_ok());
    let res = res.unwrap();
    assert_eq!(0, res.lockups.len());

    // Check that the new owner (recipient) has the lock
    let res = query_specific_user_lockups(
        &deps.as_ref(),
        &env,
        recipient.to_string(),
        vec![0], // Query only the first lockup
    );
    assert!(res.is_ok());
    let res = res.unwrap();
    assert_eq!(1, res.lockups.len());
    assert_eq!(0, res.lockups[0].lock_entry.lock_id);
    assert_eq!(1000, res.lockups[0].lock_entry.funds.amount.u128());
    assert_eq!(ST_ATOM_ON_NEUTRON, res.lockups[0].lock_entry.funds.denom);

    // Also check via the Owner Of query
    let res = query_owner_of(deps.as_ref(), env, token_id, None);
    assert!(res.is_ok());

    let res = res.unwrap();
    assert_eq!(res.owner, recipient.to_string());
}

#[test]
fn test_handle_execute_send_nft_lsm_fail() {
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([
            (IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string()),
            (
                ST_ATOM_ON_NEUTRON.to_string(),
                ST_ATOM_ON_STRIDE.to_string(),
            ),
        ]),
    );

    // Setup initial state
    let (mut deps, env) = (mock_dependencies(grpc_query), mock_env());

    let owner = "owner";
    let contract_address = "contract-address";
    let info = get_message_info(&deps.api, owner, &[]);

    // Proper contract initialization
    let msg = get_default_instantiate_msg(&deps.api);
    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok(), "Failed to instantiate contract: {:?}", res);

    // Setup dependencies with custom wasm querier that recognizes "contract-address" as a contract
    let contract_addr = deps.api.addr_make(contract_address);
    setup_contract_info_mock(&mut deps, contract_addr.clone());

    // Set up validators for rounds
    set_default_validator_for_rounds(deps.as_mut(), 0, 100);

    // Lock tokens first to create a lock entry
    let lock_info = get_message_info(
        &deps.api,
        owner,
        &[Coin::new(1000u64, IBC_DENOM_1.to_string())],
    );
    let lock_msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };
    let lock_res = execute(deps.as_mut(), env.clone(), lock_info, lock_msg);
    assert!(lock_res.is_ok(), "Failed to lock tokens: {:?}", lock_res);

    // Recipient is the contract address
    let recipient = contract_addr.to_string();
    let token_id = "0".to_string(); // First lock ID is 0
    let execute_msg = ExecuteMsg::SendNft {
        contract: recipient.clone(),
        token_id: token_id.clone(),
        msg: Binary::from(b""),
    };

    // Execute the message using the contract's execute function
    let res = execute(deps.as_mut(), env.clone(), info.clone(), execute_msg);

    // Verify the response
    assert!(
        res.is_err(),
        "Should not be able to send LSM lockup {:?}",
        res
    );
    assert!(res
        .unwrap_err()
        .to_string()
        .to_lowercase()
        .contains("cannot transfer lsm lockups"));
}

#[test]
fn test_handle_execute_send_nft_st_atom_success() {
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([
            (IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string()),
            (
                ST_ATOM_ON_NEUTRON.to_string(),
                ST_ATOM_ON_STRIDE.to_string(),
            ),
        ]),
    );

    // Setup initial state
    let (mut deps, env) = (mock_dependencies(grpc_query), mock_env());

    let owner = "owner";
    let contract_address = "contract-address";
    let info = get_message_info(&deps.api, owner, &[]);

    // Proper contract initialization
    let msg = get_default_instantiate_msg(&deps.api);
    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok(), "Failed to instantiate contract: {:?}", res);

    // Setup ST_ATOM token info provider
    let token_info_provider_addr = deps.api.addr_make("token_info_provider_1");
    setup_st_atom_token_info_provider_mock(&mut deps, token_info_provider_addr, Decimal::one());

    // Lock tokens first to create a lock entry with ST_ATOM_ON_NEUTRON
    let lock_info = get_message_info(
        &deps.api,
        owner,
        &[Coin::new(1000u64, ST_ATOM_ON_NEUTRON.to_string())],
    );
    let lock_msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };
    let lock_res = execute(deps.as_mut(), env.clone(), lock_info, lock_msg);
    assert!(lock_res.is_ok(), "Failed to lock tokens: {:?}", lock_res);

    // Now that we locked, we setup dependencies with custom wasm querier that recognizes "contract-address" as a contract
    let contract_addr = deps.api.addr_make(contract_address);
    setup_contract_info_mock(&mut deps, contract_addr.clone());

    // Recipient is the contract address
    let recipient = contract_addr.to_string();
    let token_id = "0".to_string(); // First lock ID is 0
    let execute_msg = ExecuteMsg::SendNft {
        contract: recipient.clone(),
        token_id: token_id.clone(),
        msg: Binary::from(b""),
    };

    // Execute the message using the contract's execute function
    let res = execute(deps.as_mut(), env.clone(), info.clone(), execute_msg);

    // Verify the response
    assert!(res.is_ok(), "Failed to send NFT: {:?}", res);
    let res = res.unwrap();

    check_send_nft_result(
        res,
        info.sender.as_str(),
        recipient.as_str(),
        token_id.as_str(),
    );
}

#[test]
fn test_handle_execute_send_nft_st_atom_with_vote_success() {
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([
            (IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string()),
            (
                ST_ATOM_ON_NEUTRON.to_string(),
                ST_ATOM_ON_STRIDE.to_string(),
            ),
        ]),
    );

    // Setup initial state
    let (mut deps, env) = (mock_dependencies(grpc_query), mock_env());

    let admin_address = "addr0000";
    let admin_info = get_message_info(&deps.api, admin_address, &[]);

    // Proper contract initialization
    let msg = get_default_instantiate_msg(&deps.api);
    let res = instantiate(deps.as_mut(), env.clone(), admin_info.clone(), msg);
    assert!(res.is_ok(), "Failed to instantiate contract: {:?}", res);

    // Setup ST_ATOM token info provider
    let token_info_provider_addr = deps.api.addr_make("token_info_provider_1");
    setup_st_atom_token_info_provider_mock(&mut deps, token_info_provider_addr, Decimal::one());

    // Create a proposal to vote on
    let create_proposal_msg = ExecuteMsg::CreateProposal {
        round_id: None,
        tranche_id: 1,
        title: "TEST PROPOSAL".to_string(),
        description: "".to_string(),
        deployment_duration: MIN_DEPLOYMENT_DURATION,
        minimum_atom_liquidity_request: Uint128::new(1_000_000),
    };

    execute(
        deps.as_mut(),
        env.clone(),
        admin_info.clone(),
        create_proposal_msg,
    )
    .expect("Should create proposal");

    // Setup the owner and their lockup
    let owner = "owner";

    // Lock tokens first to create a lock entry with ST_ATOM_ON_NEUTRON
    let lock_info = get_message_info(
        &deps.api,
        owner,
        &[Coin::new(1000u64, ST_ATOM_ON_NEUTRON.to_string())],
    );
    let lock_msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };
    let lock_res = execute(deps.as_mut(), env.clone(), lock_info, lock_msg);
    assert!(lock_res.is_ok(), "Failed to lock tokens: {:?}", lock_res);

    // Now we vote on the proposal with the locked tokens
    let info = get_message_info(&deps.api, owner, &[]);
    let vote_msg = ExecuteMsg::Vote {
        tranche_id: 1,
        proposals_votes: vec![ProposalToLockups {
            lock_ids: vec![0],
            proposal_id: 0,
        }],
    };
    execute(deps.as_mut(), env.clone(), info.clone(), vote_msg).expect("Should create vote");

    // verify user's vote worked
    let vote_query_res = query_all_votes(deps.as_ref(), 0, 100);
    assert!(
        vote_query_res.is_ok(),
        "Vote query should not fail: {:?}",
        vote_query_res
    );
    let votes = vote_query_res.unwrap().votes;
    assert_eq!(1, votes.len());
    let vote = votes.first().unwrap();
    assert_eq!(vote.sender_addr, info.sender);
    assert_eq!(0, vote.lock_id);
    assert_eq!(0, vote.vote.prop_id);

    // Now that we locked and voted, we setup dependencies with custom wasm querier that recognizes "contract-address" as a contract
    let contract_address = "contract-address";
    let contract_addr = deps.api.addr_make(contract_address);
    setup_contract_info_mock(&mut deps, contract_addr.clone());

    // Recipient of the NFT is the contract address
    let recipient = contract_addr.to_string();
    let token_id = "0".to_string(); // First lock ID is 0

    let send_msg = ExecuteMsg::SendNft {
        contract: recipient.clone(),
        token_id: token_id.clone(),
        msg: Binary::from(b""),
    };

    // Execute the message using the contract's execute function
    let send_res = execute(deps.as_mut(), env.clone(), info.clone(), send_msg);

    // Verify the response
    assert!(send_res.is_ok(), "Failed to send NFT: {:?}", send_res);
    let send_res = send_res.unwrap();

    check_send_nft_result(
        send_res,
        info.sender.as_str(),
        recipient.as_str(),
        token_id.as_str(),
    );
}

fn check_send_nft_result(
    send_res: Response<NeutronMsg>,
    sender: &str,
    receiving_contract: &str,
    token_id: &str,
) {
    // Check that action is set correctly
    assert!(send_res
        .attributes
        .iter()
        .any(|attr| attr.key == "action" && attr.value == "send_nft"));

    // Check the from and to addresses
    assert!(send_res
        .attributes
        .iter()
        .any(|attr| attr.key == "from" && attr.value == sender));
    assert!(send_res
        .attributes
        .iter()
        .any(|attr| attr.key == "to" && attr.value == receiving_contract));

    // Check the token_id
    assert!(send_res
        .attributes
        .iter()
        .any(|attr| attr.key == "token_id" && attr.value == token_id));

    // Verify that we have a WasmMsg::Execute message in the response
    let messages = send_res.messages;
    assert_eq!(messages.len(), 1);

    // This tests that we've correctly set up the Receive message
    match &messages[0].msg {
        cosmwasm_std::CosmosMsg::Wasm(cosmwasm_std::WasmMsg::Execute {
            contract_addr,
            msg,
            ..
        }) => {
            // Check contract address
            assert_eq!(contract_addr, receiving_contract);

            // Parse the message to check its contents
            let parsed_msg: ReceiverExecuteMsg = from_json(msg).unwrap();
            let ReceiverExecuteMsg::ReceiveNft(receive_msg) = parsed_msg;

            // Check sender value
            assert_eq!(receive_msg.sender, sender);

            // Check token_id value
            assert_eq!(receive_msg.token_id, token_id);
        }
        _ => panic!("Expected WasmMsg::Execute"),
    }
}

#[test]
fn test_handle_execute_approve() {
    // Setup mock for grpc queries
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([
            (IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string()),
            (
                ST_ATOM_ON_NEUTRON.to_string(),
                ST_ATOM_ON_STRIDE.to_string(),
            ),
        ]),
    );

    // Setup initial state
    let (mut deps, env) = (mock_dependencies(grpc_query), mock_env());

    let owner = "owner";
    let info = get_message_info(&deps.api, owner, &[]);

    // Proper contract initialization
    let msg = get_default_instantiate_msg(&deps.api);
    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok(), "Failed to instantiate contract: {:?}", res);

    // Set up validators for rounds
    set_default_validator_for_rounds(deps.as_mut(), 0, 100);

    // Setup ST_ATOM token info provider
    let token_info_provider_addr = deps.api.addr_make("token_info_provider_1");
    setup_st_atom_token_info_provider_mock(&mut deps, token_info_provider_addr, Decimal::one());

    // Lock tokens first to create a lock entry
    let lock_info = get_message_info(
        &deps.api,
        owner,
        &[Coin::new(1000u64, ST_ATOM_ON_NEUTRON.to_string())],
    );
    let lock_msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };
    let lock_res = execute(deps.as_mut(), env.clone(), lock_info, lock_msg);
    assert!(lock_res.is_ok(), "Failed to lock tokens: {:?}", lock_res);

    // Create an approval message for spender over token_id
    let spender = "spender";
    let spender_addr = deps.api.addr_make(spender);
    let token_id = "0".to_string(); // First lock ID is 0
    let execute_msg = ExecuteMsg::Approve {
        spender: spender_addr.to_string(),
        token_id: token_id.clone(),
        expires: None,
    };

    // Execute the message using the contract's execute function
    let res = execute(deps.as_mut(), env.clone(), info.clone(), execute_msg);

    // Verify the response
    assert!(res.is_ok());
    let res = res.unwrap();

    // Check that action is set correctly
    assert!(res
        .attributes
        .iter()
        .any(|attr| attr.key == "action" && attr.value == "approve"));

    let res = query_owner_of(deps.as_ref(), env.clone(), token_id, None);
    assert!(res.is_ok());

    let res = res.unwrap();
    assert_eq!(res.owner, get_address_as_str(&deps.api, owner));

    // Check that the approval list is not empty
    assert!(!res.approvals.is_empty());

    // Check that the spender is in the approval list and the approval does not expire
    let approval = res.approvals.first().unwrap();
    assert_eq!(approval.spender, spender_addr.to_string());
    assert_eq!(approval.expires, Expiration::Never {});

    // Verify that spender can transfer the token
    let spender_info = get_message_info(&deps.api, spender, &[]);
    let recipient = get_address_as_str(&deps.api, "recipient");
    let transfer_msg = ExecuteMsg::TransferNft {
        recipient: recipient.clone(),
        token_id: "0".to_string(),
    };
    let transfer_res = execute(deps.as_mut(), env.clone(), spender_info, transfer_msg);
    assert!(
        transfer_res.is_ok(),
        "Spender should be able to transfer token: {:?}",
        transfer_res
    );

    // Verify the token was transferred
    let owner_of_res = query_owner_of(deps.as_ref(), env, "0".to_string(), None);
    assert!(owner_of_res.is_ok());
    let owner_of = owner_of_res.unwrap();
    assert_eq!(owner_of.owner, recipient);
}

#[test]
fn test_handle_execute_approve_fail_for_lsm() {
    // Setup mock for grpc queries
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
    );

    // Setup initial state
    let (mut deps, env) = (mock_dependencies(grpc_query), mock_env());

    let owner = "owner";
    let info = get_message_info(&deps.api, owner, &[]);

    // Proper contract initialization
    let msg = get_default_instantiate_msg(&deps.api);
    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok(), "Failed to instantiate contract: {:?}", res);

    // Set up validators for rounds
    set_default_validator_for_rounds(deps.as_mut(), 0, 100);

    // Lock tokens first to create a lock entry
    let lock_info = get_message_info(
        &deps.api,
        owner,
        &[Coin::new(1000u64, IBC_DENOM_1.to_string())],
    );
    let lock_msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };
    let lock_res = execute(deps.as_mut(), env.clone(), lock_info, lock_msg);
    assert!(lock_res.is_ok(), "Failed to lock tokens: {:?}", lock_res);

    // Try to create an approval for spender
    let spender = get_address_as_str(&deps.api, "spender");
    let token_id = "0".to_string(); // First lock ID is 0
    let execute_msg = ExecuteMsg::Approve {
        spender: spender.clone(),
        token_id: token_id.clone(),
        expires: None,
    };

    // Execute the message using the contract's execute function
    let res = execute(deps.as_mut(), env.clone(), info.clone(), execute_msg);

    // Verify the response
    assert!(res.is_err());
    assert!(res
        .unwrap_err()
        .to_string()
        .to_lowercase()
        .contains("cannot approve lsm lockups"));
}

#[test]
fn test_handle_execute_revoke() {
    // Setup mock for grpc queries
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([
            (IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string()),
            (
                ST_ATOM_ON_NEUTRON.to_string(),
                ST_ATOM_ON_STRIDE.to_string(),
            ),
        ]),
    );

    // Setup initial state
    let (mut deps, env) = (mock_dependencies(grpc_query), mock_env());

    let owner = "owner";
    let info = get_message_info(&deps.api, owner, &[]);

    // Proper contract initialization
    let msg = get_default_instantiate_msg(&deps.api);
    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok(), "Failed to instantiate contract: {:?}", res);

    // Set up validators for rounds
    set_default_validator_for_rounds(deps.as_mut(), 0, 100);

    // Setup ST_ATOM token info provider
    let token_info_provider_addr = deps.api.addr_make("token_info_provider_1");
    setup_st_atom_token_info_provider_mock(&mut deps, token_info_provider_addr, Decimal::one());

    // Lock tokens first to create a lock entry
    let lock_info = get_message_info(
        &deps.api,
        owner,
        &[Coin::new(1000u64, ST_ATOM_ON_NEUTRON.to_string())],
    );
    let lock_msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };
    let lock_res = execute(deps.as_mut(), env.clone(), lock_info, lock_msg);
    assert!(lock_res.is_ok(), "Failed to lock tokens: {:?}", lock_res);

    // Try to create an approval for spender
    let spender = get_address_as_str(&deps.api, "spender");
    let token_id = "0".to_string(); // First lock ID is 0
    let execute_msg = ExecuteMsg::Approve {
        spender: spender.clone(),
        token_id: token_id.clone(),
        expires: None,
    };

    // Execute the message using the contract's execute function
    let res = execute(deps.as_mut(), env.clone(), info.clone(), execute_msg);

    // Verify the response
    assert!(res.is_ok());

    // **3. Revoke the approval
    let execute_msg = ExecuteMsg::Revoke {
        spender: spender.clone(),
        token_id: token_id.clone(),
    };

    // Execute the message using the contract's execute function
    let res = execute(deps.as_mut(), env.clone(), info.clone(), execute_msg);
    assert!(res.is_ok());

    let res = res.unwrap();
    assert_eq!(
        res.attributes,
        vec![
            attr("action", "revoke"),
            attr("spender", spender.clone()),
            attr("token_id", &token_id)
        ]
    );

    let res = query_owner_of(deps.as_ref(), env.clone(), token_id, None);
    assert!(res.is_ok());

    let res = res.unwrap();
    assert_eq!(res.owner, get_address_as_str(&deps.api, owner));

    // Check that the approval list is empty
    assert!(res.approvals.is_empty());

    // Verify that spender cannot transfer the token
    let spender_info = get_message_info(&deps.api, &spender, &[]);
    let recipient = get_address_as_str(&deps.api, "recipient");
    let transfer_msg = ExecuteMsg::TransferNft {
        recipient: recipient.clone(),
        token_id: "0".to_string(),
    };
    let transfer_res = execute(deps.as_mut(), env.clone(), spender_info, transfer_msg);
    assert!(transfer_res.is_err());
}

#[test]
fn test_handle_execute_revoke_fail_for_lsm() {
    // Setup mock for grpc queries
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
    );

    // Setup initial state
    let (mut deps, env) = (mock_dependencies(grpc_query), mock_env());

    let owner = "owner";
    let info = get_message_info(&deps.api, owner, &[]);

    // Proper contract initialization
    let msg = get_default_instantiate_msg(&deps.api);
    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok(), "Failed to instantiate contract: {:?}", res);

    // Set up validators for rounds
    set_default_validator_for_rounds(deps.as_mut(), 0, 100);

    // Lock tokens first to create a lock entry
    let lock_info = get_message_info(
        &deps.api,
        owner,
        &[Coin::new(1000u64, IBC_DENOM_1.to_string())],
    );
    let lock_msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };
    let lock_res = execute(deps.as_mut(), env.clone(), lock_info, lock_msg);
    assert!(lock_res.is_ok(), "Failed to lock tokens: {:?}", lock_res);

    // Try to create a revoke for spender
    let spender = get_address_as_str(&deps.api, "spender");
    let token_id = "0".to_string(); // First lock ID is 0
    let revoke_msg = ExecuteMsg::Revoke {
        spender: spender.clone(),
        token_id: token_id.clone(),
    };

    // Execute the message using the contract's execute function
    let revoke_res = execute(deps.as_mut(), env.clone(), info.clone(), revoke_msg);
    assert!(revoke_res.is_err());
    assert!(revoke_res
        .unwrap_err()
        .to_string()
        .to_lowercase()
        .contains("cannot revoke lsm lockups"));
}

#[test]
fn test_query_owner_of() {
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
    );

    // Setup initial state
    let (mut deps, env) = (mock_dependencies(grpc_query), mock_env());

    let owner = "owner";
    let info = get_message_info(&deps.api, owner, &[]);

    // Proper contract initialization
    let msg = get_default_instantiate_msg(&deps.api);
    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok(), "Failed to instantiate contract: {:?}", res);

    // Set up validators for rounds
    set_default_validator_for_rounds(deps.as_mut(), 0, 100);

    // Lock tokens first to create a lock entry
    let lock_info = get_message_info(
        &deps.api,
        owner,
        &[Coin::new(1000u64, IBC_DENOM_1.to_string())],
    );
    let lock_msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };
    let lock_res = execute(deps.as_mut(), env.clone(), lock_info, lock_msg);
    assert!(lock_res.is_ok(), "Failed to lock tokens: {:?}", lock_res);

    // Query owner of the lock
    let token_id = "0".to_string(); // First lock ID is 0
    let res = query_owner_of(deps.as_ref(), env, token_id, None);
    assert!(res.is_ok());

    let res = res.unwrap();
    assert_eq!(res.owner, get_address_as_str(&deps.api, owner));

    // Check that the approval list is empty
    assert!(res.approvals.is_empty());
}

#[test]
fn test_query_nft_info() {
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string())]),
    );

    // Setup initial state
    let (mut deps, env) = (mock_dependencies(grpc_query), mock_env());

    let owner = "owner";
    let info = get_message_info(&deps.api, owner, &[]);

    // Proper contract initialization
    let msg = get_default_instantiate_msg(&deps.api);
    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok(), "Failed to instantiate contract: {:?}", res);

    // Set up validators for rounds
    set_default_validator_for_rounds(deps.as_mut(), 0, 100);

    // Lock tokens first to create a lock entry
    let lock_info = get_message_info(
        &deps.api,
        owner,
        &[Coin::new(1000u64, IBC_DENOM_1.to_string())],
    );
    let lock_msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };
    let lock_res = execute(deps.as_mut(), env.clone(), lock_info, lock_msg);
    assert!(lock_res.is_ok(), "Failed to lock tokens: {:?}", lock_res);

    // Query NFT Info
    let token_id = "0".to_string(); // First lock ID is 0

    let res = query_nft_info(deps.as_ref(), env, token_id);
    assert!(res.is_ok());

    let res = res.unwrap();
    assert_eq!(
        res.extension.lock_with_power.lock_entry.owner,
        deps.api.addr_make(owner)
    );
}

#[test]
fn test_query_approval() {
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([
            (IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string()),
            (
                ST_ATOM_ON_NEUTRON.to_string(),
                ST_ATOM_ON_STRIDE.to_string(),
            ),
        ]),
    );

    // Setup initial state
    let (mut deps, env) = (mock_dependencies(grpc_query), mock_env());

    let owner = "owner";
    let info = get_message_info(&deps.api, owner, &[]);

    // Proper contract initialization
    let msg = get_default_instantiate_msg(&deps.api);
    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok(), "Failed to instantiate contract: {:?}", res);

    // Set up validators for rounds
    set_default_validator_for_rounds(deps.as_mut(), 0, 100);

    // Setup ST_ATOM token info provider
    let token_info_provider_addr = deps.api.addr_make("token_info_provider_1");
    setup_st_atom_token_info_provider_mock(&mut deps, token_info_provider_addr, Decimal::one());

    // Lock tokens first to create a lock entry
    let lock_info = get_message_info(
        &deps.api,
        owner,
        &[Coin::new(1000u64, ST_ATOM_ON_NEUTRON.to_string())],
    );
    let lock_msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };
    let lock_res = execute(deps.as_mut(), env.clone(), lock_info, lock_msg);
    assert!(lock_res.is_ok(), "Failed to lock tokens: {:?}", lock_res);

    // Create an approval for spender
    let spender = get_address_as_str(&deps.api, "spender");
    let token_id = "0".to_string(); // First lock ID is 0
    let execute_msg = ExecuteMsg::Approve {
        spender: spender.clone(),
        token_id: token_id.clone(),
        expires: None,
    };

    // Execute the message using the contract's execute function
    let res = execute(deps.as_mut(), env.clone(), info.clone(), execute_msg);
    assert!(res.is_ok());

    // Query approval
    let res = query_approval(deps.as_ref(), env, token_id, spender, Some(false));
    assert!(res.is_ok());

    let res = res.unwrap();
    assert_eq!(
        res.approval.spender,
        get_address_as_str(&deps.api, "spender")
    );
}

#[test]
fn test_query_approval_for_owner() {
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([
            (IBC_DENOM_1.to_string(), VALIDATOR_1_LST_DENOM_1.to_string()),
            (
                ST_ATOM_ON_NEUTRON.to_string(),
                ST_ATOM_ON_STRIDE.to_string(),
            ),
        ]),
    );

    // Setup initial state
    let (mut deps, env) = (mock_dependencies(grpc_query), mock_env());

    let owner = "owner";
    let info = get_message_info(&deps.api, owner, &[]);

    // Proper contract initialization
    let msg = get_default_instantiate_msg(&deps.api);
    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok(), "Failed to instantiate contract: {:?}", res);

    // Set up validators for rounds
    set_default_validator_for_rounds(deps.as_mut(), 0, 100);

    // Setup ST_ATOM token info provider
    let token_info_provider_addr = deps.api.addr_make("token_info_provider_1");
    setup_st_atom_token_info_provider_mock(&mut deps, token_info_provider_addr, Decimal::one());

    // Lock tokens first to create a lock entry
    let lock_info = get_message_info(
        &deps.api,
        owner,
        &[Coin::new(1000u64, ST_ATOM_ON_NEUTRON.to_string())],
    );
    let lock_msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };
    let lock_res = execute(deps.as_mut(), env.clone(), lock_info, lock_msg);
    assert!(lock_res.is_ok(), "Failed to lock tokens: {:?}", lock_res);

    // No need to create an Approval, owners are always approved
    let spender = get_address_as_str(&deps.api, "owner");
    let token_id = "0".to_string(); // First lock ID is 0

    // Query approval
    let res = query_approval(deps.as_ref(), env, token_id, spender.clone(), Some(false));
    assert!(res.is_ok());

    let res = res.unwrap();
    assert_eq!(res.approval.spender, spender);
}

#[test]
fn test_handle_execute_approve_all() {
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(
            ST_ATOM_ON_NEUTRON.to_string(),
            ST_ATOM_ON_STRIDE.to_string(),
        )]),
    );

    // Setup initial state
    let (mut deps, env) = (mock_dependencies(grpc_query), mock_env());

    let owner = "owner";
    let owner_addr = deps.api.addr_make(owner);
    let operator = "operator";
    let operator_addr = deps.api.addr_make(operator);
    let info = get_message_info(&deps.api, owner, &[]);

    // Proper contract initialization
    let msg = get_default_instantiate_msg(&deps.api);
    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok(), "Failed to instantiate contract: {:?}", res);

    // Setup ST_ATOM token info provider
    let token_info_provider_addr = deps.api.addr_make("token_info_provider_1");
    setup_st_atom_token_info_provider_mock(&mut deps, token_info_provider_addr, Decimal::one());

    // Approve operator for all tokens (existing and future)
    let approve_all_msg = ExecuteMsg::ApproveAll {
        operator: operator_addr.to_string(),
        expires: None,
    };
    let approve_res = execute(deps.as_mut(), env.clone(), info.clone(), approve_all_msg);
    assert!(
        approve_res.is_ok(),
        "Failed to approve operator: {:?}",
        approve_res
    );

    // Verify the response attributes
    let res = approve_res.unwrap();
    assert!(res
        .attributes
        .iter()
        .any(|attr| attr.key == "action" && attr.value == "approve_all"));
    assert!(res
        .attributes
        .iter()
        .any(|attr| attr.key == "owner" && attr.value == owner_addr.to_string()));
    assert!(res
        .attributes
        .iter()
        .any(|attr| attr.key == "operator" && attr.value == operator_addr.to_string()));

    // Lock tokens to create a lock entry
    let lock_info = get_message_info(
        &deps.api,
        owner,
        &[Coin::new(1000u64, ST_ATOM_ON_NEUTRON.to_string())],
    );
    let lock_msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };
    let lock_res = execute(deps.as_mut(), env.clone(), lock_info, lock_msg);
    assert!(lock_res.is_ok(), "Failed to lock tokens: {:?}", lock_res);

    // Verify that operator can transfer the token
    let operator_info = get_message_info(&deps.api, operator, &[]);
    let recipient = get_address_as_str(&deps.api, "recipient");
    let transfer_msg = ExecuteMsg::TransferNft {
        recipient: recipient.clone(),
        token_id: "0".to_string(),
    };
    let transfer_res = execute(deps.as_mut(), env.clone(), operator_info, transfer_msg);
    assert!(
        transfer_res.is_ok(),
        "Operator should be able to transfer token: {:?}",
        transfer_res
    );

    // Verify the token was transferred
    let owner_of_res = query_owner_of(deps.as_ref(), env, "0".to_string(), None);
    assert!(owner_of_res.is_ok());
    let owner_of = owner_of_res.unwrap();
    assert_eq!(owner_of.owner, recipient);
}

#[test]
fn test_handle_execute_approve_all_fail_expired() {
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(
            ST_ATOM_ON_NEUTRON.to_string(),
            ST_ATOM_ON_STRIDE.to_string(),
        )]),
    );

    // Setup initial state
    let (mut deps, env) = (mock_dependencies(grpc_query), mock_env());

    let owner = "owner";
    let operator = "operator";
    let operator_addr = deps.api.addr_make(operator);
    let info = get_message_info(&deps.api, owner, &[]);

    // Proper contract initialization
    let msg = get_default_instantiate_msg(&deps.api);
    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok(), "Failed to instantiate contract: {:?}", res);

    // Setup ST_ATOM token info provider
    let token_info_provider_addr = deps.api.addr_make("token_info_provider_1");
    setup_st_atom_token_info_provider_mock(&mut deps, token_info_provider_addr, Decimal::one());

    // Lock tokens to create a lock entry
    let lock_msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };
    // Lock tokens first to create a lock entry
    let lock_info = get_message_info(
        &deps.api,
        owner,
        &[Coin::new(1000u64, ST_ATOM_ON_NEUTRON.to_string())],
    );
    let lock_res = execute(deps.as_mut(), env.clone(), lock_info.clone(), lock_msg);
    println!("lock_res: {:?}", lock_res);
    assert!(lock_res.is_ok());

    // Try to approve operator with expired date
    let expired_time = env.block.time.minus_seconds(1);
    let approve_all_msg = ExecuteMsg::ApproveAll {
        operator: operator_addr.to_string(),
        expires: Some(Expiration::AtTime(expired_time)),
    };
    let approve_res = execute(deps.as_mut(), env.clone(), info.clone(), approve_all_msg);
    assert!(approve_res.is_err());
    assert_eq!(
        approve_res.unwrap_err().to_string(),
        "expiration already expired"
    );

    // Verify that operator cannot transfer the token
    let operator_info = get_message_info(&deps.api, operator, &[]);
    let recipient = get_address_as_str(&deps.api, "recipient");
    let transfer_msg = ExecuteMsg::TransferNft {
        recipient: recipient.clone(),
        token_id: "0".to_string(),
    };
    let transfer_res = execute(deps.as_mut(), env.clone(), operator_info, transfer_msg);
    assert!(transfer_res.is_err());
}

#[test]
fn test_handle_execute_revoke_all() {
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(
            ST_ATOM_ON_NEUTRON.to_string(),
            ST_ATOM_ON_STRIDE.to_string(),
        )]),
    );

    // Setup initial state
    let (mut deps, env) = (mock_dependencies(grpc_query), mock_env());

    let owner = "owner";
    let owner_addr = deps.api.addr_make(owner);
    let operator = "operator";
    let operator_addr = deps.api.addr_make(operator);
    let info = get_message_info(&deps.api, owner, &[]);

    // Proper contract initialization
    let msg = get_default_instantiate_msg(&deps.api);
    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok(), "Failed to instantiate contract: {:?}", res);

    // Setup ST_ATOM token info provider
    let token_info_provider_addr = deps.api.addr_make("token_info_provider_1");
    setup_st_atom_token_info_provider_mock(&mut deps, token_info_provider_addr, Decimal::one());

    // Lock tokens to create a lock entry
    let lock_msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };
    // Lock tokens first to create a lock entry
    let lock_info = get_message_info(
        &deps.api,
        owner,
        &[Coin::new(1000u64, ST_ATOM_ON_NEUTRON.to_string())],
    );
    let lock_res = execute(
        deps.as_mut(),
        env.clone(),
        lock_info.clone(),
        lock_msg.clone(),
    );
    assert!(lock_res.is_ok());

    // Approve operator for all tokens
    let approve_all_msg = ExecuteMsg::ApproveAll {
        operator: operator_addr.to_string(),
        expires: None,
    };
    let approve_res = execute(deps.as_mut(), env.clone(), info.clone(), approve_all_msg);
    assert!(approve_res.is_ok());

    // Verify the response
    let res = approve_res.unwrap();
    assert!(res
        .attributes
        .iter()
        .any(|attr| attr.key == "action" && attr.value == "approve_all"));
    assert!(res
        .attributes
        .iter()
        .any(|attr| attr.key == "owner" && attr.value == owner_addr.to_string()));
    assert!(res
        .attributes
        .iter()
        .any(|attr| attr.key == "operator" && attr.value == operator_addr.to_string()));

    // Revoke all approvals for operator
    let revoke_all_msg = ExecuteMsg::RevokeAll {
        operator: operator_addr.to_string(),
    };
    let revoke_res = execute(deps.as_mut(), env.clone(), info.clone(), revoke_all_msg);
    assert!(revoke_res.is_ok());

    // Verify the response
    let res = revoke_res.unwrap();
    assert!(res
        .attributes
        .iter()
        .any(|attr| attr.key == "action" && attr.value == "revoke_all"));
    assert!(res
        .attributes
        .iter()
        .any(|attr| attr.key == "owner" && attr.value == owner_addr.to_string()));
    assert!(res
        .attributes
        .iter()
        .any(|attr| attr.key == "operator" && attr.value == operator_addr.to_string()));

    // Verify that operator cannot transfer the token anymore
    let operator_info = get_message_info(&deps.api, operator, &[]);
    let recipient = get_address_as_str(&deps.api, "recipient");
    let transfer_msg = ExecuteMsg::TransferNft {
        recipient: recipient.clone(),
        token_id: "0".to_string(),
    };
    let transfer_res = execute(deps.as_mut(), env.clone(), operator_info, transfer_msg);
    assert!(transfer_res.is_err());
}

//We can revoke all for an operator even if there is no approval for that operator, this operator should not be able to transfer the token
#[test]
fn test_handle_execute_revoke_all_no_approval() {
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(
            ST_ATOM_ON_NEUTRON.to_string(),
            ST_ATOM_ON_STRIDE.to_string(),
        )]),
    );

    // Setup initial state
    let (mut deps, env) = (mock_dependencies(grpc_query), mock_env());

    let owner = "owner";
    let owner_addr = deps.api.addr_make(owner);
    let operator = "operator";
    let operator_addr = deps.api.addr_make(operator);
    let info = get_message_info(&deps.api, owner, &[]);

    // Proper contract initialization
    let msg = get_default_instantiate_msg(&deps.api);
    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok(), "Failed to instantiate contract: {:?}", res);

    // Setup ST_ATOM token info provider
    let token_info_provider_addr = deps.api.addr_make("token_info_provider_1");
    setup_st_atom_token_info_provider_mock(&mut deps, token_info_provider_addr, Decimal::one());

    // Lock tokens to create a lock entry
    let lock_msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };
    // Lock tokens first to create a lock entry
    let lock_info = get_message_info(
        &deps.api,
        owner,
        &[Coin::new(1000u64, ST_ATOM_ON_NEUTRON.to_string())],
    );
    let lock_res = execute(deps.as_mut(), env.clone(), lock_info.clone(), lock_msg);
    assert!(lock_res.is_ok());

    // Try to revoke all approvals for operator without any previous approval
    let revoke_all_msg = ExecuteMsg::RevokeAll {
        operator: operator_addr.to_string(),
    };
    let revoke_res = execute(deps.as_mut(), env.clone(), info.clone(), revoke_all_msg);
    assert!(revoke_res.is_ok());

    // Verify the response
    let res = revoke_res.unwrap();
    assert!(res
        .attributes
        .iter()
        .any(|attr| attr.key == "action" && attr.value == "revoke_all"));
    assert!(res
        .attributes
        .iter()
        .any(|attr| attr.key == "owner" && attr.value == owner_addr.to_string()));
    assert!(res
        .attributes
        .iter()
        .any(|attr| attr.key == "operator" && attr.value == operator_addr.to_string()));

    // Verify that operator still cannot transfer the token
    let operator_info = get_message_info(&deps.api, operator, &[]);
    let recipient = get_address_as_str(&deps.api, "recipient");
    let transfer_msg = ExecuteMsg::TransferNft {
        recipient: recipient.clone(),
        token_id: "0".to_string(),
    };
    let transfer_res = execute(deps.as_mut(), env.clone(), operator_info, transfer_msg);
    assert!(transfer_res.is_err());
}

#[test]
fn test_query_num_tokens() {
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(
            ST_ATOM_ON_NEUTRON.to_string(),
            ST_ATOM_ON_STRIDE.to_string(),
        )]),
    );

    // Setup initial state
    let (mut deps, env) = (mock_dependencies(grpc_query), mock_env());

    let owner = "owner";
    let info = get_message_info(&deps.api, owner, &[]);

    // Proper contract initialization
    let msg = get_default_instantiate_msg(&deps.api);
    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok(), "Failed to instantiate contract: {:?}", res);

    // Setup ST_ATOM token info provider
    let token_info_provider_addr = deps.api.addr_make("token_info_provider_1");
    setup_st_atom_token_info_provider_mock(&mut deps, token_info_provider_addr, Decimal::one());

    // Query num_tokens before any lock is created
    let query_msg = QueryMsg::NumTokens {};
    let query_res = query(deps.as_ref(), env.clone(), query_msg);
    assert!(query_res.is_ok());
    let res: NumTokensResponse = from_json(query_res.unwrap()).unwrap();
    assert!(res.count == 0);

    // Lock tokens to create a lock entry
    let lock_msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };
    let lock_info = get_message_info(
        &deps.api,
        owner,
        &[Coin::new(1000u64, ST_ATOM_ON_NEUTRON.to_string())],
    );
    let lock_res = execute(deps.as_mut(), env.clone(), lock_info.clone(), lock_msg);
    assert!(lock_res.is_ok());

    // Query num_tokens after creating a lock
    let query_msg = QueryMsg::NumTokens {};
    let query_res = query(deps.as_ref(), env.clone(), query_msg);
    assert!(query_res.is_ok());
    let num_tokens: NumTokensResponse = from_json(query_res.unwrap()).unwrap();
    assert_eq!(num_tokens.count, 1);
}

#[test]
fn test_query_tokens() {
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(
            ST_ATOM_ON_NEUTRON.to_string(),
            ST_ATOM_ON_STRIDE.to_string(),
        )]),
    );

    // Setup initial state
    let (mut deps, env) = (mock_dependencies(grpc_query), mock_env());

    let owner = "owner";
    let owner_addr = deps.api.addr_make(owner);
    let info = get_message_info(&deps.api, owner, &[]);

    // Proper contract initialization
    let msg = get_default_instantiate_msg(&deps.api);
    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok(), "Failed to instantiate contract: {:?}", res);

    // Setup ST_ATOM token info provider
    let token_info_provider_addr = deps.api.addr_make("token_info_provider_1");
    setup_st_atom_token_info_provider_mock(&mut deps, token_info_provider_addr, Decimal::one());

    // Query tokens before any lock is created
    let query_msg = QueryMsg::Tokens {
        owner: owner_addr.to_string(),
        start_after: None,
        limit: None,
    };
    let query_res = query(deps.as_ref(), env.clone(), query_msg);

    assert!(query_res.is_err());
    assert!(query_res.unwrap_err().to_string().contains("not found"));

    // Lock tokens to create a lock entry
    let lock_msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };
    let lock_info = get_message_info(
        &deps.api,
        owner,
        &[Coin::new(1000u64, ST_ATOM_ON_NEUTRON.to_string())],
    );
    let lock_res = execute(deps.as_mut(), env.clone(), lock_info.clone(), lock_msg);
    assert!(lock_res.is_ok());

    // Query tokens after creating a lock
    let query_msg = QueryMsg::Tokens {
        owner: owner_addr.to_string(),
        start_after: None,
        limit: None,
    };
    let query_res = query(deps.as_ref(), env.clone(), query_msg);
    assert!(query_res.is_ok());
    let tokens: TokensResponse = from_json(query_res.unwrap()).unwrap();
    assert_eq!(tokens.tokens.len(), 1);
    assert_eq!(tokens.tokens[0], "0");
}

#[test]
fn test_query_all_tokens() {
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(
            ST_ATOM_ON_NEUTRON.to_string(),
            ST_ATOM_ON_STRIDE.to_string(),
        )]),
    );

    // Setup initial state
    let (mut deps, env) = (mock_dependencies(grpc_query), mock_env());

    let owner1 = "owner1";
    let owner1_addr = deps.api.addr_make(owner1);
    let owner2 = "owner2";
    let owner2_addr = deps.api.addr_make(owner2);
    let info = get_message_info(&deps.api, owner1, &[]);

    // Proper contract initialization
    let msg = get_default_instantiate_msg(&deps.api);
    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok(), "Failed to instantiate contract: {:?}", res);

    // Setup ST_ATOM token info provider
    let token_info_provider_addr = deps.api.addr_make("token_info_provider_1");
    setup_st_atom_token_info_provider_mock(&mut deps, token_info_provider_addr, Decimal::one());

    // Query all tokens before any lock is created
    let query_msg = QueryMsg::AllTokens {
        start_after: None,
        limit: None,
    };
    let query_res = query(deps.as_ref(), env.clone(), query_msg);
    assert!(query_res.is_ok());
    let tokens: TokensResponse = from_json(query_res.unwrap()).unwrap();
    assert_eq!(tokens.tokens.len(), 0);

    // Create first lock for owner1
    let lock_msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };
    let lock_info = get_message_info(
        &deps.api,
        owner1_addr.as_ref(),
        &[Coin::new(1000u64, ST_ATOM_ON_NEUTRON.to_string())],
    );
    let lock_res = execute(
        deps.as_mut(),
        env.clone(),
        lock_info.clone(),
        lock_msg.clone(),
    );
    assert!(lock_res.is_ok());

    // Create second lock for owner2
    let lock_info2 = get_message_info(
        &deps.api,
        owner2_addr.as_ref(),
        &[Coin::new(1000u64, ST_ATOM_ON_NEUTRON.to_string())],
    );
    let lock_res2 = execute(deps.as_mut(), env.clone(), lock_info2.clone(), lock_msg);
    assert!(lock_res2.is_ok());

    // Query all tokens after creating two locks
    let query_msg = QueryMsg::AllTokens {
        start_after: None,
        limit: None,
    };
    let query_res = query(deps.as_ref(), env.clone(), query_msg);
    assert!(query_res.is_ok());
    let tokens: TokensResponse = from_json(query_res.unwrap()).unwrap();
    assert_eq!(tokens.tokens.len(), 2);
    assert_eq!(tokens.tokens[0], "0");
    assert_eq!(tokens.tokens[1], "1");
}

#[test]
fn test_query_all_operators() {
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(
            ST_ATOM_ON_NEUTRON.to_string(),
            ST_ATOM_ON_STRIDE.to_string(),
        )]),
    );

    // Setup initial state
    let (mut deps, env) = (mock_dependencies(grpc_query), mock_env());

    let owner = "owner";
    let owner_addr = deps.api.addr_make(owner);
    let operator = "operator";
    let operator_addr = deps.api.addr_make(operator);
    let info = get_message_info(&deps.api, owner, &[]);

    // Proper contract initialization
    let msg = get_default_instantiate_msg(&deps.api);
    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok(), "Failed to instantiate contract: {:?}", res);

    // Setup ST_ATOM token info provider
    let token_info_provider_addr = deps.api.addr_make("token_info_provider_1");
    setup_st_atom_token_info_provider_mock(&mut deps, token_info_provider_addr, Decimal::one());

    // Query all operators before any approval
    let query_msg = QueryMsg::AllOperators {
        owner: owner_addr.to_string(),
        include_expired: None,
        start_after: None,
        limit: None,
    };
    let query_res = query(deps.as_ref(), env.clone(), query_msg);
    assert!(query_res.is_ok());
    let operators: OperatorsResponse = from_json(query_res.unwrap()).unwrap();
    assert_eq!(operators.operators.len(), 0);

    // Lock tokens to create a lock entry
    let lock_msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };
    let lock_info = get_message_info(
        &deps.api,
        owner,
        &[Coin::new(1000u64, ST_ATOM_ON_NEUTRON.to_string())],
    );
    let lock_res = execute(deps.as_mut(), env.clone(), lock_info.clone(), lock_msg);
    assert!(lock_res.is_ok());

    // Approve operator for all tokens
    let approve_all_msg = ExecuteMsg::ApproveAll {
        operator: operator_addr.to_string(),
        expires: None,
    };
    let approve_res = execute(deps.as_mut(), env.clone(), info.clone(), approve_all_msg);
    assert!(approve_res.is_ok());

    // Query all operators after approval
    let query_msg = QueryMsg::AllOperators {
        owner: owner_addr.to_string(),
        include_expired: None,
        start_after: None,
        limit: None,
    };
    let query_res = query(deps.as_ref(), env.clone(), query_msg);
    assert!(query_res.is_ok());
    let operators: OperatorsResponse = from_json(query_res.unwrap()).unwrap();
    assert_eq!(operators.operators.len(), 1);
    assert_eq!(operators.operators[0].spender, operator_addr.to_string());
    assert!(matches!(
        operators.operators[0].expires,
        Expiration::Never {}
    ));
}

#[test]
fn test_query_collection_info() {
    let user_address = "addr0000";
    let (mut deps, env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());
    let info = get_message_info(&deps.api, user_address, &[]);

    // Proper contract initialization
    let mut instantiate_msg = get_default_instantiate_msg(&deps.api);
    instantiate_msg.cw721_collection_info = Some(CollectionInfo {
        name: "Hydro Lockups for test".to_string(),
        symbol: "hydro-lockups-for-test".to_string(),
    });
    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), instantiate_msg);
    assert!(res.is_ok(), "Failed to instantiate contract: {:?}", res);

    let res = query_collection_info(deps.as_ref(), env);
    assert!(res.is_ok(), "Failed to query collection info: {:?}", res);

    let res = res.unwrap();
    assert_eq!(res.name, "Hydro Lockups for test");
    assert_eq!(res.symbol, "hydro-lockups-for-test");
}

#[test]
fn test_query_tokens_with_transfer() {
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(
            ST_ATOM_ON_NEUTRON.to_string(),
            ST_ATOM_ON_STRIDE.to_string(),
        )]),
    );

    // Setup initial state
    let (mut deps, env) = (mock_dependencies(grpc_query), mock_env());
    let owner1 = "owner1";
    let owner2 = "owner2";
    let owner2_addr = deps.api.addr_make(owner2);
    let info = get_message_info(&deps.api, owner1, &[]);

    // Initialize contract
    let instantiate_msg = get_default_instantiate_msg(&deps.api);
    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), instantiate_msg);
    assert!(res.is_ok(), "Failed to instantiate contract: {:?}", res);
    // Setup ST_ATOM token info provider
    let token_info_provider_addr = deps.api.addr_make("token_info_provider_1");
    setup_st_atom_token_info_provider_mock(&mut deps, token_info_provider_addr, Decimal::one());
    // Create lockup for owner1
    let lock_msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };
    let lock_info_owner1 = get_message_info(
        &deps.api,
        owner1,
        &[Coin::new(1000u64, ST_ATOM_ON_NEUTRON.to_string())],
    );
    let res = execute(
        deps.as_mut(),
        env.clone(),
        lock_info_owner1.clone(),
        lock_msg,
    );
    assert!(res.is_ok(), "Failed to create lockup for owner1: {:?}", res);

    // Create lockup for owner2
    let lock_info_owner2 = get_message_info(
        &deps.api,
        owner2,
        &[Coin::new(1000u64, ST_ATOM_ON_NEUTRON.to_string())],
    );
    let lock_msg2 = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };
    let res = execute(
        deps.as_mut(),
        env.clone(),
        lock_info_owner2.clone(),
        lock_msg2,
    );
    assert!(res.is_ok(), "Failed to create lockup for owner2: {:?}", res);

    // Transfer lockup from owner1 to owner2
    let transfer_msg = ExecuteMsg::TransferNft {
        recipient: owner2_addr.to_string(),
        token_id: "0".to_string(),
    };
    let res = execute(deps.as_mut(), env.clone(), info.clone(), transfer_msg);
    assert!(res.is_ok(), "Failed to transfer lockup: {:?}", res);

    // Query tokens for owner2 with limit 1
    let query_msg = QueryMsg::Tokens {
        owner: owner2_addr.to_string(),
        start_after: None,
        limit: Some(1),
    };
    let res = query(deps.as_ref(), env.clone(), query_msg);
    assert!(res.is_ok(), "Failed to query tokens: {:?}", res);
    let tokens: TokensResponse = from_json(res.unwrap()).unwrap();
    assert_eq!(tokens.tokens.len(), 1);
    assert_eq!(tokens.tokens[0], "0");

    // Query tokens for owner2 with no limit
    let query_msg = QueryMsg::Tokens {
        owner: owner2_addr.to_string(),
        start_after: None,
        limit: None,
    };
    let res = query(deps.as_ref(), env, query_msg);
    assert!(res.is_ok(), "Failed to query tokens: {:?}", res);
    let tokens: TokensResponse = from_json(res.unwrap()).unwrap();
    assert_eq!(tokens.tokens.len(), 2);
    assert_eq!(tokens.tokens[0], "0");
    assert_eq!(tokens.tokens[1], "1");
}

#[test]
fn test_operator_approve_for_token() {
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(
            ST_ATOM_ON_NEUTRON.to_string(),
            ST_ATOM_ON_STRIDE.to_string(),
        )]),
    );

    // Setup initial state
    let (mut deps, env) = (mock_dependencies(grpc_query), mock_env());

    let owner = "owner";
    let operator = "operator";
    let operator_addr = deps.api.addr_make(operator);
    let info = get_message_info(&deps.api, owner, &[]);

    // Proper contract initialization
    let msg = get_default_instantiate_msg(&deps.api);
    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok(), "Failed to instantiate contract: {:?}", res);

    // Setup ST_ATOM token info provider
    let token_info_provider_addr = deps.api.addr_make("token_info_provider_1");
    setup_st_atom_token_info_provider_mock(&mut deps, token_info_provider_addr, Decimal::one());

    // Lock tokens to create a lock entry
    let lock_info = get_message_info(
        &deps.api,
        owner,
        &[Coin::new(1000u64, ST_ATOM_ON_NEUTRON.to_string())],
    );
    let lock_msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };
    let lock_res = execute(deps.as_mut(), env.clone(), lock_info, lock_msg);
    assert!(lock_res.is_ok(), "Failed to lock tokens: {:?}", lock_res);

    // Create an approval message for spender over token_id
    let spender = get_address_as_str(&deps.api, "spender");
    let token_id = "0".to_string(); // First lock ID is 0
    let approve_msg = ExecuteMsg::Approve {
        spender: spender.clone(),
        token_id: token_id.clone(),
        expires: None,
    };

    // Verify that operator cannot create an Approval on the token (not yet operator)
    let operator_info = get_message_info(&deps.api, operator, &[]);
    let approve_res = execute(
        deps.as_mut(),
        env.clone(),
        operator_info.clone(),
        approve_msg.clone(),
    );
    assert!(approve_res.is_err());

    // Approve operator for all tokens
    let approve_all_msg = ExecuteMsg::ApproveAll {
        operator: operator_addr.to_string(),
        expires: None,
    };
    let approve_res = execute(deps.as_mut(), env.clone(), info.clone(), approve_all_msg);
    assert!(
        approve_res.is_ok(),
        "Failed to approve operator: {:?}",
        approve_res
    );

    // Verify that operator can now create an Approval on the token
    let approve_res = execute(
        deps.as_mut(),
        env.clone(),
        operator_info.clone(),
        approve_msg,
    );
    assert!(approve_res.is_ok());

    let approve_res = approve_res.unwrap();
    assert!(approve_res
        .attributes
        .iter()
        .any(|attr| attr.key == "action" && attr.value == "approve"));
    assert!(approve_res
        .attributes
        .iter()
        .any(|attr| attr.key == "spender" && attr.value == spender));
    assert!(approve_res
        .attributes
        .iter()
        .any(|attr| attr.key == "token_id" && attr.value == token_id));
}

#[test]
fn test_operator_revoke_for_token() {
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(
            ST_ATOM_ON_NEUTRON.to_string(),
            ST_ATOM_ON_STRIDE.to_string(),
        )]),
    );

    // Setup initial state
    let (mut deps, env) = (mock_dependencies(grpc_query), mock_env());

    let owner = "owner";
    let operator = "operator";
    let operator_addr = deps.api.addr_make(operator);
    let info = get_message_info(&deps.api, owner, &[]);

    // Proper contract initialization
    let msg = get_default_instantiate_msg(&deps.api);
    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok(), "Failed to instantiate contract: {:?}", res);

    // Setup ST_ATOM token info provider
    let token_info_provider_addr = deps.api.addr_make("token_info_provider_1");
    setup_st_atom_token_info_provider_mock(&mut deps, token_info_provider_addr, Decimal::one());

    // Lock tokens to create a lock entry
    let lock_info = get_message_info(
        &deps.api,
        owner,
        &[Coin::new(1000u64, ST_ATOM_ON_NEUTRON.to_string())],
    );
    let lock_msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };
    let lock_res = execute(deps.as_mut(), env.clone(), lock_info, lock_msg);
    assert!(lock_res.is_ok(), "Failed to lock tokens: {:?}", lock_res);

    // Create an approval message for spender over token_id
    let spender = get_address_as_str(&deps.api, "spender");
    let token_id = "0".to_string(); // First lock ID is 0
    let approve_msg = ExecuteMsg::Approve {
        spender: spender.clone(),
        token_id: token_id.clone(),
        expires: None,
    };

    let approve_res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        approve_msg.clone(),
    );
    assert!(approve_res.is_ok());

    // Verify that operator cannot revoke an Approval on the token (not yet operator)
    let operator_info = get_message_info(&deps.api, operator, &[]);
    let revoke_msg = ExecuteMsg::Revoke {
        spender: spender.clone(),
        token_id: token_id.clone(),
    };
    let revoke_res = execute(
        deps.as_mut(),
        env.clone(),
        operator_info.clone(),
        revoke_msg.clone(),
    );
    assert!(revoke_res.is_err());

    // Approve operator for all tokens
    let approve_all_msg = ExecuteMsg::ApproveAll {
        operator: operator_addr.to_string(),
        expires: None,
    };
    let approve_res = execute(deps.as_mut(), env.clone(), info.clone(), approve_all_msg);
    assert!(
        approve_res.is_ok(),
        "Failed to approve operator: {:?}",
        approve_res
    );

    // Verify that operator can now revoke an Approval on the token
    let revoke_res = execute(
        deps.as_mut(),
        env.clone(),
        operator_info.clone(),
        revoke_msg.clone(),
    );
    assert!(revoke_res.is_ok());

    let revoke_res = revoke_res.unwrap();
    assert!(revoke_res
        .attributes
        .iter()
        .any(|attr| attr.key == "action" && attr.value == "revoke"));
    assert!(revoke_res
        .attributes
        .iter()
        .any(|attr| attr.key == "spender" && attr.value == spender));
    assert!(revoke_res
        .attributes
        .iter()
        .any(|attr| attr.key == "token_id" && attr.value == token_id));
}

// Unlocking a token should remove any Approval on that token
#[test]
fn test_handle_execute_approve_then_unlock() {
    // Setup mock for grpc queries
    let grpc_query = denom_trace_grpc_query_mock(
        "transfer/channel-0".to_string(),
        HashMap::from([(
            ST_ATOM_ON_NEUTRON.to_string(),
            ST_ATOM_ON_STRIDE.to_string(),
        )]),
    );

    // Setup initial state
    let (mut deps, mut env) = (mock_dependencies(grpc_query), mock_env());

    let owner = "owner";
    let info = get_message_info(&deps.api, owner, &[]);

    // Proper contract initialization
    let msg = get_default_instantiate_msg(&deps.api);
    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg);
    assert!(res.is_ok(), "Failed to instantiate contract: {:?}", res);

    // Setup ST_ATOM token info provider
    let token_info_provider_addr = deps.api.addr_make("token_info_provider_1");
    setup_st_atom_token_info_provider_mock(&mut deps, token_info_provider_addr, Decimal::one());

    // Lock tokens first to create a lock entry
    let lock_info = get_message_info(
        &deps.api,
        owner,
        &[Coin::new(1000u64, ST_ATOM_ON_NEUTRON.to_string())],
    );
    let lock_msg = ExecuteMsg::LockTokens {
        lock_duration: ONE_MONTH_IN_NANO_SECONDS,
        proof: None,
    };
    let lock_res = execute(deps.as_mut(), env.clone(), lock_info, lock_msg);
    assert!(lock_res.is_ok(), "Failed to lock tokens: {:?}", lock_res);

    // Try to create an approval for spender
    let spender_addr = deps.api.addr_make("spender");
    let spender = spender_addr.to_string();
    let token_id = "0".to_string(); // First lock ID is 0
    let execute_msg = ExecuteMsg::Approve {
        spender: spender.clone(),
        token_id: token_id.clone(),
        expires: None,
    };

    // Execute the message using the contract's execute function
    let res = execute(deps.as_mut(), env.clone(), info.clone(), execute_msg);

    // Verify the response
    assert!(res.is_ok());

    // Advance the chain by one month + 1 nano second and check that user can unlock tokens
    env.block.time = env.block.time.plus_nanos(ONE_MONTH_IN_NANO_SECONDS + 1);

    // Unlock the token
    let res = execute(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        ExecuteMsg::UnlockTokens { lock_ids: None },
    );
    assert!(res.is_ok());

    // We need to directly retrieve from the store, as both query_owner_of and query_approval queries
    // first check that the lockup exists, and error out if not
    let approval = NFT_APPROVALS.may_load(&deps.storage, (0, spender_addr.clone()));
    assert_eq!(approval.unwrap(), None);
}
