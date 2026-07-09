use cosmwasm_std::{AnyMsg, Coin, CosmosMsg};
use ibc_proto::{cosmos::base::v1beta1::Coin as ProtoCoin, ibc::core::client::v1::Height};
use prost::Message;

/// Type URL used to route this message via the standard Cosmos SDK Msg service router
/// (Neutron's `x/transfer` module, which wraps ibc-go's transfer module with relayer fees).
pub const NEUTRON_TRANSFER_TYPE_URL: &str = "/neutron.transfer.MsgTransfer";

// Mirrors `neutron.feerefunder.Fee`. Hand-written (instead of depending on neutron-sdk) so this
// contract can remain chain-agnostic and compile/deploy on both Neutron and the Cosmos Hub.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct NeutronFee {
    #[prost(message, repeated, tag = "1")]
    pub recv_fee: Vec<ProtoCoin>,
    #[prost(message, repeated, tag = "2")]
    pub ack_fee: Vec<ProtoCoin>,
    #[prost(message, repeated, tag = "3")]
    pub timeout_fee: Vec<ProtoCoin>,
}

// Mirrors `neutron.transfer.v1.MsgTransfer` (ibc-go's `MsgTransfer` plus an embedded `Fee`).
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct NeutronMsgTransfer {
    #[prost(string, tag = "1")]
    pub source_port: String,
    #[prost(string, tag = "2")]
    pub source_channel: String,
    #[prost(message, optional, tag = "3")]
    pub token: Option<ProtoCoin>,
    #[prost(string, tag = "4")]
    pub sender: String,
    #[prost(string, tag = "5")]
    pub receiver: String,
    #[prost(message, optional, tag = "6")]
    pub timeout_height: Option<Height>,
    #[prost(uint64, tag = "7")]
    pub timeout_timestamp: u64,
    #[prost(string, tag = "8")]
    pub memo: String,
    #[prost(message, optional, tag = "9")]
    pub fee: Option<NeutronFee>,
}

fn to_proto_coin(coin: &Coin) -> ProtoCoin {
    ProtoCoin {
        denom: coin.denom.clone(),
        amount: coin.amount.to_string(),
    }
}

/// Builds a `CosmosMsg::Any`-wrapped Neutron ibc-transfer-with-fee message. `recv_fee` is always
/// empty, as required by Neutron; `ibc_fee` is used for both `ack_fee` and `timeout_fee`.
/// Timeout height is disabled (only `timeout_timestamp` is used), matching plain IBC transfers.
#[allow(clippy::too_many_arguments)]
pub fn build_neutron_ibc_transfer_msg(
    source_port: &str,
    source_channel: &str,
    sender: String,
    receiver: String,
    token: Coin,
    timeout_timestamp_nanos: u64,
    memo: Option<String>,
    ibc_fee: Coin,
) -> CosmosMsg {
    let proto_fee = to_proto_coin(&ibc_fee);

    let msg = NeutronMsgTransfer {
        source_port: source_port.to_string(),
        source_channel: source_channel.to_string(),
        token: Some(to_proto_coin(&token)),
        sender,
        receiver,
        timeout_height: Some(Height {
            revision_number: 0,
            revision_height: 0,
        }),
        timeout_timestamp: timeout_timestamp_nanos,
        memo: memo.unwrap_or_default(),
        fee: Some(NeutronFee {
            recv_fee: vec![],
            ack_fee: vec![proto_fee.clone()],
            timeout_fee: vec![proto_fee],
        }),
    };

    CosmosMsg::Any(AnyMsg {
        type_url: NEUTRON_TRANSFER_TYPE_URL.to_string(),
        value: msg.encode_to_vec().into(),
    })
}
