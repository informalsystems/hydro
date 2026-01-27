use cosmwasm_schema::cw_serde;
use cosmwasm_std::{to_json_binary, Addr, Coin, StdResult, WasmMsg};

#[cw_serde]
pub enum DropStakingExecuteMsg {
    Unbond {},
}

#[cw_serde]
pub enum DropVoucherExecuteMsg {
    SendNft {
        contract: String,
        token_id: String,
        msg: String,
    },
}

#[cw_serde]
pub enum WithdrawMsg {
    Withdraw {},
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
    let withdraw_msg = to_json_binary(&WithdrawMsg::Withdraw {})?;

    let msg = DropVoucherExecuteMsg::SendNft {
        contract: withdrawal_manager.to_string(),
        token_id,
        msg: withdraw_msg.to_base64(),
    };

    Ok(WasmMsg::Execute {
        contract_addr: drop_voucher.to_string(),
        msg: to_json_binary(&msg)?,
        funds: vec![],
    })
}
