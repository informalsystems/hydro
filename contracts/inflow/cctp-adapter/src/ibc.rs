use cosmwasm_std::{Coin, Deps, Env, StdResult};
use neutron_sdk::bindings::msg::{IbcFee, NeutronMsg};
use neutron_sdk::bindings::query::NeutronQuery;
use neutron_sdk::query::min_ibc_fee::MinIbcFeeResponse;
use neutron_sdk::sudo::msg::RequestPacketTimeoutHeight;

use crate::state::Config;

/// Local denom on Neutron for IBC fees (Neutron relayer fees, not EVM bridging fees)
pub const LOCAL_DENOM: &str = "untrn";

/// Transfer port for IBC transfers
pub const TRANSFER_PORT: &str = "transfer";

/// Query minimum IBC fee from Neutron
/// This is the fee paid to the IBC relayer (in untrn), separate from Skip bridging fees
pub fn query_ibc_fee(deps: Deps<NeutronQuery>, denom: &str) -> StdResult<IbcFee> {
    let min_fee: MinIbcFeeResponse = deps.querier.query(&cosmwasm_std::QueryRequest::Custom(
        NeutronQuery::MinIbcFee {},
    ))?;

    let mut fee = min_fee.min_fee;
    // Filter to only include fees in the requested denom
    fee.ack_fee.retain(|f| f.denom == denom);
    fee.timeout_fee.retain(|f| f.denom == denom);

    Ok(fee)
}

/// Create IBC transfer message to Noble
pub fn create_ibc_transfer_msg(
    deps: Deps<NeutronQuery>,
    env: &Env,
    config: &Config,
    token: Coin,
    receiver: String,
    memo: String,
    timeout_seconds: u64,
) -> StdResult<NeutronMsg> {
    // Query IBC relayer fee (paid in untrn)
    let fee = query_ibc_fee(deps, LOCAL_DENOM)?;

    // Calculate timeout timestamp
    let timeout_timestamp = env.block.time.plus_seconds(timeout_seconds).nanos();

    Ok(NeutronMsg::IbcTransfer {
        source_port: TRANSFER_PORT.to_string(),
        source_channel: config.noble_transfer_channel_id.clone(),
        token,
        sender: env.contract.address.to_string(),
        receiver,
        timeout_height: RequestPacketTimeoutHeight {
            revision_number: None,
            revision_height: None,
        },
        timeout_timestamp,
        memo,
        fee,
    })
}
