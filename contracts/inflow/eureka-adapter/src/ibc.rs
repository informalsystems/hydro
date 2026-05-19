use cosmwasm_std::{Coin, Deps, Env, StdResult};
use neutron_sdk::bindings::msg::{IbcFee, NeutronMsg};
use neutron_sdk::bindings::query::NeutronQuery;
use neutron_sdk::query::min_ibc_fee::MinIbcFeeResponse;
use neutron_sdk::sudo::msg::RequestPacketTimeoutHeight;

use crate::state::Config;

pub const LOCAL_DENOM: &str = "untrn";
pub const TRANSFER_PORT: &str = "transfer";

/// Query the minimum IBC fee from Neutron
pub fn query_ibc_fee(deps: Deps<NeutronQuery>, denom: &str) -> StdResult<IbcFee> {
    let min_fee: MinIbcFeeResponse = deps.querier.query(&cosmwasm_std::QueryRequest::Custom(
        NeutronQuery::MinIbcFee {},
    ))?;

    let mut fee = min_fee.min_fee;
    fee.ack_fee.retain(|f| f.denom == denom);
    fee.timeout_fee.retain(|f| f.denom == denom);

    Ok(fee)
}

/// Create the IBC transfer message from Neutron to Cosmos Hub
pub fn create_ibc_transfer_msg(
    deps: Deps<NeutronQuery>,
    env: &Env,
    config: &Config,
    token: Coin,
    receiver: String,
    memo: String,
    timeout_seconds: u64,
) -> StdResult<NeutronMsg> {
    let fee = query_ibc_fee(deps, LOCAL_DENOM)?;
    let timeout_timestamp = env.block.time.plus_seconds(timeout_seconds).nanos();

    Ok(NeutronMsg::IbcTransfer {
        source_port: TRANSFER_PORT.to_string(),
        source_channel: config.neutron_to_hub_channel.clone(),
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
