//! Chain messages and replies not in cosmwasm-std: gaia's `x/liquid` redeem as a
//! `CosmosMsg::Any` with hand-written prost types (as hATOM's `chain.rs` / `proto.rs`), and the
//! `msg_responses` the gate decodes in its replies.
//!
//! Field numbers checked against gaia v29.1.0 `proto/gaia/liquid/v1beta1/tx.proto`,
//! cosmos-sdk v0.53 `proto/cosmos/staking/v1beta1/tx.proto` and wasmd v0.60
//! `proto/cosmwasm/wasm/v1/tx.proto`.

use cosmwasm_std::{
    AnyMsg, Binary, Coin, CosmosMsg, StdError, StdResult, SubMsgResponse, Timestamp,
};
use prost::Message;

pub const REDEEM_TYPE_URL: &str = "/gaia.liquid.v1beta1.MsgRedeemTokensForShares";
pub const REDEEM_RESPONSE_TYPE_URL: &str = "/gaia.liquid.v1beta1.MsgRedeemTokensForSharesResponse";
pub const UNDELEGATE_RESPONSE_TYPE_URL: &str = "/cosmos.staking.v1beta1.MsgUndelegateResponse";
pub const EXECUTE_RESPONSE_TYPE_URL: &str = "/cosmwasm.wasm.v1.MsgExecuteContractResponse";

pub mod proto {
    /// `cosmos.base.v1beta1.Coin`
    #[derive(Clone, PartialEq, prost::Message)]
    pub struct Coin {
        #[prost(string, tag = "1")]
        pub denom: String,
        #[prost(string, tag = "2")]
        pub amount: String,
    }

    /// `google.protobuf.Timestamp`
    #[derive(Clone, PartialEq, prost::Message)]
    pub struct Timestamp {
        #[prost(int64, tag = "1")]
        pub seconds: i64,
        #[prost(int32, tag = "2")]
        pub nanos: i32,
    }

    /// `gaia.liquid.v1beta1.MsgRedeemTokensForShares`
    #[derive(Clone, PartialEq, prost::Message)]
    pub struct MsgRedeemTokensForShares {
        #[prost(string, tag = "1")]
        pub delegator_address: String,
        #[prost(message, optional, tag = "2")]
        pub amount: Option<Coin>,
    }

    /// `gaia.liquid.v1beta1.MsgRedeemTokensForSharesResponse`
    #[derive(Clone, PartialEq, prost::Message)]
    pub struct MsgRedeemTokensForSharesResponse {
        #[prost(message, optional, tag = "1")]
        pub amount: Option<Coin>,
    }

    /// `cosmos.staking.v1beta1.MsgUndelegateResponse`
    #[derive(Clone, PartialEq, prost::Message)]
    pub struct MsgUndelegateResponse {
        #[prost(message, optional, tag = "1")]
        pub completion_time: Option<Timestamp>,
        #[prost(message, optional, tag = "2")]
        pub amount: Option<Coin>,
    }

    /// `cosmwasm.wasm.v1.MsgExecuteContractResponse`
    #[derive(Clone, PartialEq, prost::Message)]
    pub struct MsgExecuteContractResponse {
        #[prost(bytes = "vec", tag = "1")]
        pub data: Vec<u8>,
    }
}

impl From<&Coin> for proto::Coin {
    fn from(c: &Coin) -> Self {
        proto::Coin {
            denom: c.denom.clone(),
            amount: c.amount.to_string(),
        }
    }
}

impl proto::Coin {
    pub fn to_std(&self) -> StdResult<Coin> {
        Ok(Coin {
            denom: self.denom.clone(),
            amount: self.amount.parse()?,
        })
    }
}

/// `MsgRedeemTokensForShares`: redeems the LSM `shares` held by `delegator` into a native
/// delegation of `delegator`.
pub fn redeem_tokens(delegator: &str, shares: &Coin) -> CosmosMsg {
    CosmosMsg::Any(AnyMsg {
        type_url: REDEEM_TYPE_URL.to_string(),
        value: Binary::new(
            proto::MsgRedeemTokensForShares {
                delegator_address: delegator.to_string(),
                amount: Some(shares.into()),
            }
            .encode_to_vec(),
        ),
    })
}

/// Decodes the first `msg_responses` entry of a submessage result, checking its type URL.
pub fn decode_msg_response<T: Message + Default>(
    res: &SubMsgResponse,
    expected_type_url: &str,
) -> StdResult<T> {
    let r = res
        .msg_responses
        .first()
        .ok_or_else(|| StdError::generic_err("no msg_responses in reply"))?;
    if r.type_url != expected_type_url {
        return Err(StdError::generic_err(format!(
            "unexpected msg response {} (wanted {expected_type_url})",
            r.type_url
        )));
    }
    T::decode(r.value.as_slice()).map_err(|e| StdError::generic_err(e.to_string()))
}

/// The `data` a wasm execute submessage returned (its `MsgExecuteContractResponse`).
pub fn execute_response_data(res: &SubMsgResponse) -> StdResult<Binary> {
    let r: proto::MsgExecuteContractResponse = decode_msg_response(res, EXECUTE_RESPONSE_TYPE_URL)?;
    Ok(Binary::new(r.data))
}

/// `completion_time` and `amount` of an undelegation reply.
pub fn undelegate_response(res: &SubMsgResponse) -> StdResult<(Timestamp, Coin)> {
    let r: proto::MsgUndelegateResponse = decode_msg_response(res, UNDELEGATE_RESPONSE_TYPE_URL)?;
    let t = r
        .completion_time
        .ok_or_else(|| StdError::generic_err("undelegation reply without completion_time"))?;
    if t.seconds < 0 || t.nanos < 0 {
        return Err(StdError::generic_err("negative completion_time"));
    }
    let completion = Timestamp::from_seconds(t.seconds as u64).plus_nanos(t.nanos as u64);
    let amount = r
        .amount
        .ok_or_else(|| StdError::generic_err("undelegation reply without amount"))?
        .to_std()?;
    Ok((completion, amount))
}
