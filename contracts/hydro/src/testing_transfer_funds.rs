use cosmwasm_std::{testing::mock_env, AnyMsg, Coin, CosmosMsg};
use prost::Message;

use crate::{
    contract::{execute, instantiate},
    ibc_transfer::{NeutronMsgTransfer, NEUTRON_TRANSFER_TYPE_URL},
    msg::ExecuteMsg,
    testing::{
        get_default_instantiate_msg, get_message_info, DEFAULT_WHITELIST_ADMIN, IBC_DENOM_1,
        ST_ATOM_ON_NEUTRON,
    },
    testing_mocks::{mock_dependencies, no_op_grpc_query_mock},
};

const HUB_RECIPIENT: &str = "cosmos1srjdd7y6duuukmaenghucasqlycddcc65qdj34k6spq8pwk4h6ms7j4w4j";

fn ibc_fee() -> Coin {
    Coin::new(1000u128, "untrn".to_string())
}

fn decode_transfer_msg(msg: &CosmosMsg) -> NeutronMsgTransfer {
    match msg {
        CosmosMsg::Any(AnyMsg { type_url, value }) => {
            assert_eq!(type_url, NEUTRON_TRANSFER_TYPE_URL);
            NeutronMsgTransfer::decode(value.as_slice())
                .expect("failed to decode NeutronMsgTransfer")
        }
        _ => panic!("expected a CosmosMsg::Any transfer message"),
    }
}

#[test]
fn transfer_funds_unauthorized_test() {
    let (mut deps, env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());
    let admin_info = get_message_info(&deps.api, DEFAULT_WHITELIST_ADMIN, &[]);
    let msg = get_default_instantiate_msg(&deps.api);
    let res = instantiate(deps.as_mut(), env.clone(), admin_info, msg);
    assert!(res.is_ok());

    let non_admin_info = get_message_info(&deps.api, "addr0000", &[]);
    let res = execute(
        deps.as_mut(),
        env,
        non_admin_info,
        ExecuteMsg::TransferFundsToHub {
            denoms: vec![IBC_DENOM_1.to_string()],
            recipient: HUB_RECIPIENT.to_string(),
            ibc_fee: ibc_fee(),
        },
    );

    assert!(res.unwrap_err().to_string().contains("Unauthorized"));
}

#[test]
fn transfer_funds_invalid_recipient_test() {
    let (mut deps, env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());
    let admin_info = get_message_info(&deps.api, DEFAULT_WHITELIST_ADMIN, &[]);
    let msg = get_default_instantiate_msg(&deps.api);
    let res = instantiate(deps.as_mut(), env.clone(), admin_info.clone(), msg);
    assert!(res.is_ok());

    let res = execute(
        deps.as_mut(),
        env,
        admin_info,
        ExecuteMsg::TransferFundsToHub {
            denoms: vec![IBC_DENOM_1.to_string()],
            recipient: "not-a-valid-address".to_string(),
            ibc_fee: ibc_fee(),
        },
    );

    assert!(res.is_err());
}

#[test]
fn transfer_funds_regular_denom_test() {
    let (mut deps, env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());
    let admin_info = get_message_info(&deps.api, DEFAULT_WHITELIST_ADMIN, &[]);
    let msg = get_default_instantiate_msg(&deps.api);
    let res = instantiate(deps.as_mut(), env.clone(), admin_info.clone(), msg);
    assert!(res.is_ok());

    deps.querier.update_balance(
        env.contract.address.clone(),
        vec![Coin::new(1000u128, IBC_DENOM_1.to_string())],
    );

    let res = execute(
        deps.as_mut(),
        env,
        admin_info,
        ExecuteMsg::TransferFundsToHub {
            denoms: vec![IBC_DENOM_1.to_string()],
            recipient: HUB_RECIPIENT.to_string(),
            ibc_fee: ibc_fee(),
        },
    )
    .unwrap();

    assert_eq!(res.messages.len(), 1);
    let transfer = decode_transfer_msg(&res.messages[0].clone().msg);
    assert_eq!(transfer.source_channel, "channel-1");
    assert_eq!(transfer.receiver, HUB_RECIPIENT);
    assert_eq!(transfer.token.unwrap().amount, "1000");
    assert_eq!(transfer.memo, "");

    let fee = transfer.fee.expect("fee must be set");
    assert!(fee.recv_fee.is_empty());
    assert_eq!(fee.ack_fee.len(), 1);
    assert_eq!(fee.ack_fee[0].amount, "1000");
    assert_eq!(fee.ack_fee[0].denom, "untrn");
    assert_eq!(fee.timeout_fee, fee.ack_fee);
}

#[test]
fn transfer_funds_st_atom_uses_pfm_test() {
    let (mut deps, env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());
    let admin_info = get_message_info(&deps.api, DEFAULT_WHITELIST_ADMIN, &[]);
    let msg = get_default_instantiate_msg(&deps.api);
    let res = instantiate(deps.as_mut(), env.clone(), admin_info.clone(), msg);
    assert!(res.is_ok());

    deps.querier.update_balance(
        env.contract.address.clone(),
        vec![Coin::new(500u128, ST_ATOM_ON_NEUTRON.to_string())],
    );

    let res = execute(
        deps.as_mut(),
        env,
        admin_info,
        ExecuteMsg::TransferFundsToHub {
            denoms: vec![ST_ATOM_ON_NEUTRON.to_string()],
            recipient: HUB_RECIPIENT.to_string(),
            ibc_fee: ibc_fee(),
        },
    )
    .unwrap();

    assert_eq!(res.messages.len(), 1);
    let transfer = decode_transfer_msg(&res.messages[0].clone().msg);
    assert_eq!(transfer.source_channel, "channel-8");
    assert_eq!(
        transfer.receiver,
        "stride1srjdd7y6duuukmaenghucasqlycddcc65qdj34k6spq8pwk4h6msfjsukj"
    );
    assert_eq!(transfer.token.unwrap().amount, "500");

    let memo_json: serde_json::Value = serde_json::from_str(&transfer.memo).unwrap();
    let forward = &memo_json["forward"];
    assert_eq!(forward["channel"], "channel-0");
    assert_eq!(forward["port"], "transfer");
    assert_eq!(forward["receiver"], HUB_RECIPIENT);
}

#[test]
fn transfer_funds_skips_zero_balance_denoms_test() {
    let (mut deps, env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());
    let admin_info = get_message_info(&deps.api, DEFAULT_WHITELIST_ADMIN, &[]);
    let msg = get_default_instantiate_msg(&deps.api);
    let res = instantiate(deps.as_mut(), env.clone(), admin_info.clone(), msg);
    assert!(res.is_ok());

    // Only IBC_DENOM_1 has a nonzero balance; the other two should be skipped without error.
    deps.querier.update_balance(
        env.contract.address.clone(),
        vec![Coin::new(1000u128, IBC_DENOM_1.to_string())],
    );

    let res = execute(
        deps.as_mut(),
        env,
        admin_info,
        ExecuteMsg::TransferFundsToHub {
            denoms: vec![
                IBC_DENOM_1.to_string(),
                ST_ATOM_ON_NEUTRON.to_string(),
                "ibc/unrelated-empty-denom".to_string(),
            ],
            recipient: HUB_RECIPIENT.to_string(),
            ibc_fee: ibc_fee(),
        },
    )
    .unwrap();

    assert_eq!(res.messages.len(), 1);
    let transfer = decode_transfer_msg(&res.messages[0].clone().msg);
    assert_eq!(transfer.token.unwrap().amount, "1000");
}

#[test]
fn transfer_funds_all_zero_balances_produces_no_messages_test() {
    let (mut deps, env) = (mock_dependencies(no_op_grpc_query_mock()), mock_env());
    let admin_info = get_message_info(&deps.api, DEFAULT_WHITELIST_ADMIN, &[]);
    let msg = get_default_instantiate_msg(&deps.api);
    let res = instantiate(deps.as_mut(), env.clone(), admin_info.clone(), msg);
    assert!(res.is_ok());

    let res = execute(
        deps.as_mut(),
        env,
        admin_info,
        ExecuteMsg::TransferFundsToHub {
            denoms: vec![IBC_DENOM_1.to_string()],
            recipient: HUB_RECIPIENT.to_string(),
            ibc_fee: ibc_fee(),
        },
    )
    .unwrap();

    assert_eq!(res.messages.len(), 0);
}
