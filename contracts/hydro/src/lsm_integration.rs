use cosmwasm_std::{Binary, Deps, StdError, StdResult};

use ibc_proto::ibc::apps::transfer::v1::DenomTrace;

use prost::Message;

pub const IBC_TOKEN_PREFIX: &str = "ibc/";
// ibc-go v10 (shipped in Neutron v10) removed the DenomTrace gRPC endpoint and replaced it
// with a new Denom endpoint. The new endpoint is available at the following path.
pub const DENOM_GRPC: &str = "/ibc.applications.transfer.v1.Query/Denom";
pub const TRANSFER_PORT: &str = "transfer";
pub const COSMOS_VALIDATOR_PREFIX: &str = "cosmosvaloper";
pub const COSMOS_VALIDATOR_ADDR_LENGTH: usize = 52; // e.g. cosmosvaloper15w6ra6m68c63t0sv2hzmkngwr9t88e23r8vtg5

// Proto types for the ibc-go v10 Denom query endpoint.
// These types are not yet available in the ibc-proto crate.
#[derive(Clone, PartialEq, Message)]
pub(crate) struct QueryDenomRequest {
    #[prost(string, tag = "1")]
    pub hash: String,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct DenomHop {
    #[prost(string, tag = "1")]
    pub port_id: String,
    #[prost(string, tag = "2")]
    pub channel_id: String,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct IbcDenom {
    #[prost(string, tag = "1")]
    pub base: String,
    #[prost(message, repeated, tag = "3")]
    pub trace: Vec<DenomHop>,
}

#[derive(Clone, PartialEq, Message)]
pub(crate) struct QueryDenomResponse {
    #[prost(message, optional, tag = "1")]
    pub denom: Option<IbcDenom>,
}

pub fn resolve_validator_from_denom(
    deps: &Deps,
    hub_transfer_channel_id: &str,
    denom: String,
) -> StdResult<String> {
    if !denom.starts_with(IBC_TOKEN_PREFIX) {
        return Err(StdError::generic_err("IBC token expected"));
    }

    let denom_trace = query_ibc_denom_trace(deps, denom)?;

    // valid path example: transfer/channel-1
    let path_parts: Vec<&str> = denom_trace.path.split("/").collect();
    if path_parts.len() != 2
        || path_parts[0] != TRANSFER_PORT
        || path_parts[1] != hub_transfer_channel_id
    {
        return Err(StdError::generic_err(
            "Only LSTs transferred directly from the Cosmos Hub can be locked.",
        ));
    }

    // valid base_denom example: cosmosvaloper16k579jk6yt2cwmqx9dz5xvq9fug2tekvlu9qdv/22409
    let base_denom_parts: Vec<&str> = denom_trace.base_denom.split("/").collect();
    if base_denom_parts.len() != 2
        || base_denom_parts[0].len() != COSMOS_VALIDATOR_ADDR_LENGTH
        || !base_denom_parts[0].starts_with(COSMOS_VALIDATOR_PREFIX)
        || base_denom_parts[1].parse::<u64>().is_err()
    {
        return Err(StdError::generic_err(
            "Only LSTs from the Cosmos Hub can be locked.",
        ));
    }

    Ok(base_denom_parts[0].to_string())
}

pub fn query_ibc_denom_trace(deps: &Deps, denom: String) -> StdResult<DenomTrace> {
    let result = deps
        .querier
        .query_grpc(
            DENOM_GRPC.to_owned(),
            Binary::new(QueryDenomRequest { hash: denom }.encode_to_vec()),
        )
        .map_err(|err| StdError::generic_err(format!("Failed to obtain IBC denom trace: {err}")))?;

    let ibc_denom = QueryDenomResponse::decode(result.as_slice())
        .map_err(|_| StdError::generic_err("Failed to obtain IBC denom trace"))?
        .denom
        .ok_or(StdError::generic_err("Failed to obtain IBC denom trace"))?;

    // Convert the new ibc-go v10 Denom format to the legacy DenomTrace format used throughout
    // this contract. The new trace is a slice of (port, channel) hops; the old path is those
    // pairs joined with "/", e.g. "transfer/channel-1".
    let path = ibc_denom
        .trace
        .iter()
        .map(|hop| format!("{}/{}", hop.port_id, hop.channel_id))
        .collect::<Vec<_>>()
        .join("/");

    Ok(DenomTrace {
        path,
        base_denom: ibc_denom.base,
    })
}
