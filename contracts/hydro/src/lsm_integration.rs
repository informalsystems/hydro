use cosmwasm_std::{Binary, Deps, StdError, StdResult};

use ibc_proto::ibc::apps::transfer::v1::{
    DenomTrace, QueryDenomTraceRequest, QueryDenomTraceResponse,
};

use prost::Message;

pub const IBC_TOKEN_PREFIX: &str = "ibc/";
pub const DENOM_TRACE_GRPC: &str = "/ibc.applications.transfer.v1.Query/DenomTrace";
pub const TRANSFER_PORT: &str = "transfer";
pub const COSMOS_VALIDATOR_PREFIX: &str = "cosmosvaloper";
pub const COSMOS_VALIDATOR_ADDR_LENGTH: usize = 52; // e.g. cosmosvaloper15w6ra6m68c63t0sv2hzmkngwr9t88e23r8vtg5

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
    let denom_trace_query_result = deps
        .querier
        .query_grpc(
            DENOM_TRACE_GRPC.to_owned(),
            Binary::new(QueryDenomTraceRequest { hash: denom }.encode_to_vec()),
        )
        .map_err(|err| {
            StdError::generic_err(format!("Failed to obtain IBC denom trace: {}", err))
        })?;

    let denom_trace = QueryDenomTraceResponse::decode(denom_trace_query_result.as_slice())
        .map_err(|_| StdError::generic_err("Failed to obtain IBC denom trace"))?
        .denom_trace
        .ok_or(StdError::generic_err("Failed to obtain IBC denom trace"))?;

    Ok(denom_trace)
}
