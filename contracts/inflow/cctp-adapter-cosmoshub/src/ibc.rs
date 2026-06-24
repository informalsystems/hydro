use cosmwasm_std::{Coin, CosmosMsg, Env, IbcMsg, IbcTimeout, StdResult};

use crate::state::Config;

/// Create IBC transfer message to Noble.
/// On Cosmos Hub, contracts issue standard IbcMsg::Transfer directly —
/// no Neutron relayer fee (IbcFee) is required.
pub fn create_ibc_transfer_msg(
    env: &Env,
    config: &Config,
    token: Coin,
    receiver: String,
    memo: String,
    timeout_seconds: u64,
) -> StdResult<CosmosMsg> {
    let timeout = IbcTimeout::with_timestamp(env.block.time.plus_seconds(timeout_seconds));

    Ok(CosmosMsg::Ibc(IbcMsg::Transfer {
        channel_id: config.noble_transfer_channel_id.clone(),
        to_address: receiver,
        amount: token,
        timeout,
        memo: Some(memo),
    }))
}
