use cosmwasm_std::{Coin, Deps, Env, QueryRequest, StdResult};
use neutron_sdk::bindings::msg::{IbcFee, NeutronMsg};
use neutron_sdk::bindings::query::NeutronQuery;
use neutron_sdk::query::min_ibc_fee::MinIbcFeeResponse;
use neutron_sdk::sudo::msg::RequestPacketTimeoutHeight;

use crate::state::{ChainConfig, Config, TransferFundsInstructions};

/// Local denom on Neutron
pub const LOCAL_DENOM: &str = "untrn";

/// Queries the minimum IBC fee from Neutron
pub fn query_ibc_fee(deps: Deps<NeutronQuery>, denom: &str) -> StdResult<IbcFee> {
    let min_fee: MinIbcFeeResponse = deps
        .querier
        .query(&QueryRequest::Custom(NeutronQuery::MinIbcFee {}))?;

    let mut fee = min_fee.min_fee;
    fee.ack_fee.retain(|fee| fee.denom == denom);
    fee.timeout_fee.retain(|fee| fee.denom == denom);

    Ok(fee)
}

/// Creates an IBC transfer message
pub fn create_ibc_transfer_msg(
    deps: Deps<NeutronQuery>,
    env: &Env,
    chain_config: &ChainConfig,
    coin: Coin,
    recipient: String,
    timeout_timestamp: u64,
    memo: Option<String>,
) -> StdResult<NeutronMsg> {
    let fee = query_ibc_fee(deps, LOCAL_DENOM)?;

    Ok(NeutronMsg::IbcTransfer {
        source_port: "transfer".to_string(),
        source_channel: chain_config.channel_from_neutron.clone(),
        token: coin,
        sender: env.contract.address.to_string(),
        receiver: recipient,
        timeout_height: RequestPacketTimeoutHeight {
            revision_number: None,
            revision_height: None,
        },
        timeout_timestamp,
        memo: memo.unwrap_or_default(),
        fee,
    })
}

/// Calculates IBC timeout timestamp in nanoseconds
pub fn calculate_timeout(
    env: &Env,
    config: &Config,
    instructions: &TransferFundsInstructions,
) -> u64 {
    let timeout_seconds = instructions
        .timeout_seconds
        .unwrap_or(config.default_timeout_seconds);

    // Convert to nanoseconds and add to current block time
    env.block.time.plus_seconds(timeout_seconds).nanos()
}
