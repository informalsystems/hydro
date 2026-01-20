#[cfg(feature = "cosmwasm_compat")]
use crate::compat::StdErrExt;
use cosmwasm_std::{Reply, StdError, StdResult};

// SubMsg ID is used so that we can differentiate submessages sent by the smart contract when the Wasm SDK module
// calls back the reply() function on the smart contract. Since we are using the payload to populate all the data
// that we need when reply() is called, we don't need to set a unique SubMsg ID and can use 0 for all SubMsgs.
pub const UNUSED_MSG_ID: u64 = 0;

/// Default limit for pagination queries
pub const DEFAULT_PAGINATION_LIMIT: u32 = 30;
/// Maximum limit for pagination queries
pub const MAX_PAGINATION_LIMIT: u32 = 100;

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
