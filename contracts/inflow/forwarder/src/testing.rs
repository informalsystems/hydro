use cosmwasm_std::{
    from_json,
    testing::{message_info, mock_dependencies, mock_env},
    Addr, Binary, CosmosMsg, IbcMsg, Timestamp,
};
use interface::inflow::ExecuteMsg as InflowExecuteMsg;
use serde::Deserialize;

use crate::{
    contract::{execute, instantiate},
    msg::{ExecuteMsg, InstantiateMsg},
    ContractError,
};

#[test]
fn instantiate_rejects_zero_timeout() {
    let mut deps = mock_dependencies();
    let msg = InstantiateMsg {
        target_address: "target".to_string(),
        denom: "uatom".to_string(),
        inflow_contract: "neutron1contract".to_string(),
        channel_id: "channel-0".to_string(),
        ibc_timeout_seconds: 0,
    };

    let err = instantiate(deps.as_mut(), mock_env(), info("sender"), msg)
        .unwrap_err();
    assert!(matches!(err, ContractError::InvalidIbcTimeout {}));
}

#[test]
fn forward_errors_without_balance() {
    let mut deps = mock_dependencies();
    let msg = InstantiateMsg {
        target_address: "target".to_string(),
        denom: "uatom".to_string(),
        inflow_contract: "neutron1contract".to_string(),
        channel_id: "channel-0".to_string(),
        ibc_timeout_seconds: 10,
    };

    instantiate(deps.as_mut(), mock_env(), info("sender"), msg).unwrap();

    let err = execute(
        deps.as_mut(),
        mock_env(),
        info("caller"),
        ExecuteMsg::ForwardToInflow {},
    )
    .unwrap_err();

    assert!(matches!(err, ContractError::NothingToForward { .. }));
}

#[test]
fn forward_transfers_full_balance() {
    let mut deps = mock_dependencies();
    let instantiate_msg = InstantiateMsg {
        target_address: "neutron1target".to_string(),
        denom: "uatom".to_string(),
        inflow_contract: "neutron1inflow".to_string(),
        channel_id: "channel-23".to_string(),
        ibc_timeout_seconds: 42,
    };

    instantiate(deps.as_mut(), mock_env(), info("creator"), instantiate_msg).unwrap();

    let mut env = mock_env();
    env.block.time = Timestamp::from_seconds(12345);
    let contract_addr: Addr = env.contract.address.clone();
    deps.querier.bank.update_balance(
        contract_addr,
        vec![cosmwasm_std::coin(999, "uatom")],
    );

    let resp = execute(
        deps.as_mut(),
        env.clone(),
        info("caller"),
        ExecuteMsg::ForwardToInflow {},
    )
    .unwrap();

    assert_eq!(resp.messages.len(), 1);

    let msg = resp.messages[0].msg.clone();
    match msg {
        CosmosMsg::Ibc(IbcMsg::Transfer {
            channel_id,
            to_address,
            amount,
            timeout,
            memo,
        }) => {
            assert_eq!(channel_id, "channel-23");
            assert_eq!(to_address, "neutron1inflow");
            assert_eq!(amount.amount.u128(), 999);
            assert_eq!(amount.denom, "uatom");
            assert_eq!(timeout.timestamp(), Some(env.block.time.plus_seconds(42)));

            let memo = memo.expect("memo must be set");
            let memo: HookMemo = serde_json_wasm::from_str(&memo).unwrap();
            assert_eq!(memo.wasm.contract, "neutron1inflow");

            let deposit: InflowExecuteMsg = from_json(&memo.wasm.msg).unwrap();
            match deposit {
                InflowExecuteMsg::Deposit { on_behalf_of } => {
                    assert_eq!(on_behalf_of.unwrap(), "neutron1target");
                }
            }
        }
        _ => panic!("unexpected message type"),
    }
}

#[derive(Deserialize)]
struct HookMemo {
    wasm: HookMemoData,
}

#[derive(Deserialize)]
struct HookMemoData {
    contract: String,
    msg: Binary,
}

fn info(sender: &str) -> cosmwasm_std::MessageInfo {
    let addr = Addr::unchecked(sender);
    message_info(&addr, &[])
}
