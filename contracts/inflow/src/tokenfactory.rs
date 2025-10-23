use cosmwasm_std::{AnyMsg, Binary, CosmosMsg, Uint128};
use prost::Message;

pub const CREATE_DENOM_URL: &str = "/osmosis.tokenfactory.v1beta1.MsgCreateDenom";
pub const MINT_URL: &str = "/osmosis.tokenfactory.v1beta1.MsgMint";
pub const BURN_URL: &str = "/osmosis.tokenfactory.v1beta1.MsgBurn";
pub const SET_METADATA_URL: &str = "/osmosis.tokenfactory.v1beta1.MsgSetDenomMetadata";

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct MsgCreateDenom {
    #[prost(string, tag = "1")]
    pub sender: String,
    #[prost(string, tag = "2")]
    pub subdenom: String,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct MsgMint {
    #[prost(string, tag = "1")]
    pub sender: String,
    #[prost(message, optional, tag = "2")]
    pub amount: Option<Coin>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct MsgBurn {
    #[prost(string, tag = "1")]
    pub sender: String,
    #[prost(message, optional, tag = "2")]
    pub amount: Option<Coin>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Coin {
    #[prost(string, tag = "1")]
    pub denom: String,
    #[prost(string, tag = "2")]
    pub amount: String,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct MsgSetDenomMetadata {
    #[prost(string, tag = "1")]
    pub sender: String,
    #[prost(message, optional, tag = "2")]
    pub metadata: Option<Metadata>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct DenomUnit {
    #[prost(string, tag = "1")]
    pub denom: String,
    #[prost(uint32, tag = "2")]
    pub exponent: u32,
    #[prost(string, repeated, tag = "3")]
    pub aliases: ::prost::alloc::vec::Vec<String>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Metadata {
    #[prost(string, tag = "1")]
    pub description: String,
    #[prost(message, repeated, tag = "2")]
    pub denom_units: ::prost::alloc::vec::Vec<DenomUnit>,
    #[prost(string, tag = "3")]
    pub base: String,
    #[prost(string, tag = "4")]
    pub display: String,
    #[prost(string, tag = "5")]
    pub name: String,
    #[prost(string, tag = "6")]
    pub symbol: String,
    #[prost(string, tag = "7")]
    pub uri: String,
    #[prost(string, tag = "8")]
    pub uri_hash: String,
}

pub fn msg_create_denom(sender: String, subdenom: String) -> CosmosMsg {
    let msg = MsgCreateDenom { sender, subdenom };
    CosmosMsg::Any(AnyMsg {
        type_url: CREATE_DENOM_URL.to_owned(),
        value: Binary::from(msg.encode_to_vec()),
    })
}

pub fn msg_mint(sender: String, denom: String, amount: Uint128) -> CosmosMsg {
    let coin = Coin {
        denom,
        amount: amount.to_string(),
    };
    let msg = MsgMint {
        sender,
        amount: Some(coin),
    };
    CosmosMsg::Any(AnyMsg {
        type_url: MINT_URL.to_owned(),
        value: Binary::from(msg.encode_to_vec()),
    })
}

pub fn msg_burn(sender: String, denom: String, amount: Uint128) -> CosmosMsg {
    let coin = Coin {
        denom,
        amount: amount.to_string(),
    };
    let msg = MsgBurn {
        sender,
        amount: Some(coin),
    };
    CosmosMsg::Any(AnyMsg {
        type_url: BURN_URL.to_owned(),
        value: Binary::from(msg.encode_to_vec()),
    })
}

pub fn msg_set_denom_metadata(
    sender: String,
    base: String,
    display: String,
    display_exponent: u32,
    name: String,
    description: String,
    symbol: String,
    uri: String,
    uri_hash: String,
) -> CosmosMsg {
    let metadata = Metadata {
        description,
        denom_units: vec![
            DenomUnit {
                denom: base.clone(),
                exponent: 0,
                aliases: vec![],
            },
            DenomUnit {
                denom: display.clone(),
                exponent: display_exponent,
                aliases: vec![],
            },
        ],
        base,
        display,
        name,
        symbol,
        uri,
        uri_hash,
    };
    let msg = MsgSetDenomMetadata {
        sender,
        metadata: Some(metadata),
    };
    CosmosMsg::Any(AnyMsg {
        type_url: SET_METADATA_URL.to_owned(),
        value: Binary::from(msg.encode_to_vec()),
    })
}

pub fn full_denom(contract_addr: &str, subdenom: &str) -> String {
    format!("factory/{contract_addr}/{subdenom}")
}
