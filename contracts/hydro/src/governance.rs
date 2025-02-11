use cosmwasm_std::{Deps, Env, StdResult};
use neutron_sdk::bindings::query::NeutronQuery;

use crate::{
    contract::compute_current_round_id,
    lsm_integration::get_total_power_for_round,
    query::{TotalPowerAtHeightResponse, VotingPowerAtHeightResponse},
    state::TOTAL_VOTING_POWER_PER_ROUND,
    utils::{
        get_current_user_voting_power, get_round_id_for_height,
        get_user_voting_power_for_past_height, load_current_constants,
    },
};

pub fn query_total_power_at_height(
    deps: &Deps<NeutronQuery>,
    env: &Env,
    height: Option<u64>,
) -> StdResult<TotalPowerAtHeightResponse> {
    let (power, height) = match height {
        // If no height is provided, we should return the latest known total voting power
        // for the current round, paired with the current block height.
        None => {
            let constants = load_current_constants(deps, env)?;
            let current_round_id = compute_current_round_id(env, &constants)?;

            (
                get_total_power_for_round(deps, current_round_id)?.to_uint_ceil(),
                env.block.height,
            )
        }
        Some(height) => {
            let round_id = get_round_id_for_height(deps.storage, height)?;

            let power = TOTAL_VOTING_POWER_PER_ROUND
                .may_load_at_height(deps.storage, round_id, height)?
                .unwrap_or_default();

            (power, height)
        }
    };

    Ok(TotalPowerAtHeightResponse { power, height })
}

pub fn query_voting_power_at_height(
    deps: &Deps<NeutronQuery>,
    env: &Env,
    address: String,
    height: Option<u64>,
) -> StdResult<VotingPowerAtHeightResponse> {
    let address = deps.api.addr_validate(&address)?;

    let (power, height) = match height {
        // If no height is provided, we should return current user voting power, paired with the current block height.
        None => (
            get_current_user_voting_power(deps, env, address)?.into(),
            env.block.height,
        ),
        Some(height) => {
            let constants = load_current_constants(deps, env)?;
            let power = get_user_voting_power_for_past_height(deps, &constants, address, height)?;

            (power.into(), height)
        }
    };

    Ok(VotingPowerAtHeightResponse { power, height })
}
