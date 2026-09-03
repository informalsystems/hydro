// IBC Eureka Integration Module
//
// This module contains message types and helper functions for interacting with the
// IBC Eureka entry-point contract on Cosmos Hub, which forwards tokens to EVM chains.

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{to_json_binary, Coin, StdResult, Uint128, WasmMsg};

/// Top-level execute message expected by the IBC Eureka entry-point contract
#[cw_serde]
pub struct EurekaExecuteMsg {
    pub action: EurekaActionPayload,
}

#[cw_serde]
pub struct EurekaActionPayload {
    /// Timeout timestamp for the overall IBC Eureka transfer action, in seconds
    pub timeout_timestamp: u64,
    pub action: EurekaAction,
    pub exact_out: bool,
}

#[cw_serde]
pub enum EurekaAction {
    IbcTransfer { ibc_info: IbcInfo },
}

#[cw_serde]
pub struct IbcInfo {
    pub source_channel: String,
    pub receiver: String,
    pub memo: String,
    pub recover_address: String,
    pub encoding: String,
    pub eureka_fee: EurekaFeeInfo,
}

#[cw_serde]
pub struct EurekaFeeInfo {
    pub coin: Coin,
    pub receiver: String,
    /// Timeout timestamp for the Eureka relayer fee, in nanoseconds
    pub timeout_timestamp: u64,
}

/// Parameters needed to build an IBC Eureka transfer message.
///
/// The fields `action_timeout_timestamp` and `fee_timeout_timestamp` are deliberately in different
/// units (seconds vs. nanoseconds). This matches what Skip:Go's entry-point contract expects for each field.
pub struct EurekaTransferParams {
    pub skip_swap_entry_point_contract: String,
    pub source_channel: String,
    pub receiver: String,
    pub recover_address: String,
    pub encoding: String,
    pub fee_receiver: String,
    pub denom: String,
    pub amount: Uint128,
    pub fee_amount: Uint128,
    /// Timeout timestamp for the overall action, in seconds
    pub action_timeout_timestamp: u64,
    /// Timeout timestamp for the Eureka relayer fee, in nanoseconds
    pub fee_timeout_timestamp: u64,
}

/// Build the WasmMsg::Execute that triggers an IBC Eureka transfer to an EVM chain.
///
/// The total funds attached to the message is `amount + fee_amount`, both in the
/// same denom - the IBC Eureka entry contract splits out the fee itself.
pub fn build_eureka_transfer_msg(params: EurekaTransferParams) -> StdResult<WasmMsg> {
    let total_amount = params.amount + params.fee_amount;

    let msg = EurekaExecuteMsg {
        action: EurekaActionPayload {
            timeout_timestamp: params.action_timeout_timestamp,
            action: EurekaAction::IbcTransfer {
                ibc_info: IbcInfo {
                    source_channel: params.source_channel,
                    receiver: params.receiver,
                    memo: String::new(),
                    recover_address: params.recover_address,
                    encoding: params.encoding,
                    eureka_fee: EurekaFeeInfo {
                        coin: Coin {
                            denom: params.denom.clone(),
                            amount: params.fee_amount,
                        },
                        receiver: params.fee_receiver,
                        timeout_timestamp: params.fee_timeout_timestamp,
                    },
                },
            },
            exact_out: false,
        },
    };

    Ok(WasmMsg::Execute {
        contract_addr: params.skip_swap_entry_point_contract,
        msg: to_json_binary(&msg)?,
        funds: vec![Coin {
            denom: params.denom,
            amount: total_amount,
        }],
    })
}
