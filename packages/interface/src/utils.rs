use cosmwasm_std::{Reply, StdError, StdResult};

// This function extracts the response message bytes from the Reply message instance.
// Note that it only works if the Reply message contains a single response.
pub fn extract_response_msg_bytes_from_reply_msg(msg: &Reply) -> StdResult<Vec<u8>> {
    msg.result
        .clone()
        .into_result()
        .map_err(StdError::generic_err)?
        .msg_responses
        .first()
        .map(|response| response.value.to_vec())
        .ok_or_else(|| StdError::not_found("msg_responses"))
}
