use cosmwasm_schema::cw_serde;
use cosmwasm_std::{to_json_binary, Addr, Coin, StdResult, WasmMsg};

#[cw_serde]
pub enum DropStakingExecuteMsg {
    Unbond {},
}

#[cw_serde]
pub enum VoucherExecuteMsg {
    SendNft {
        contract: String,
        token_id: String,
        msg: String,
    },
}

pub fn unbond_msg(drop_staking_core: Addr, funds: Vec<Coin>) -> StdResult<WasmMsg> {
    Ok(WasmMsg::Execute {
        contract_addr: drop_staking_core.to_string(),
        msg: to_json_binary(&DropStakingExecuteMsg::Unbond {})?,
        funds,
    })
}

pub fn withdraw_voucher_msg(
    drop_voucher: Addr,
    withdrawal_manager: Addr,
    token_id: String,
) -> StdResult<WasmMsg> {
    let msg = VoucherExecuteMsg::SendNft {
        contract: withdrawal_manager.to_string(),
        token_id,
        // base64({"withdraw":{}})
        msg: "eyJ3aXRoZHJhdyI6e319".to_string(),
    };

    Ok(WasmMsg::Execute {
        contract_addr: drop_voucher.to_string(),
        msg: to_json_binary(&msg)?,
        funds: vec![],
    })
}
