use cosmwasm_std::{Decimal, Deps, StdResult};

use crate::score_keeper::get_total_power;

// TOTAL_ROUND_POWER: key(round_id) -> score_keeper
pub const TOTAL_ROUND_POWER_KEY: &[u8] = b"total_round_power";

pub fn get_total_round_power_key(round_id: u64) -> String {
    let mut key = String::from_utf8_lossy(TOTAL_ROUND_POWER_KEY).to_string();
    key.push_str(
        &round_id
            .to_be_bytes()
            .to_vec()
            .iter()
            .map(|b| format!("{:02X}", b))
            .collect::<String>(),
    );
    key
}

pub fn get_total_round_power_total(deps: Deps, round_id: u64) -> StdResult<Decimal> {
    // get the key for the round
    let key = get_total_round_power_key(round_id);

    // return the total power for that round
    get_total_power(deps.storage, key.as_str())
}

// PROP_POWER: key(prop_id) -> score_keeper
pub const PROP_POWER_KEY: &[u8] = b"prop_power";

pub fn get_prop_power_key(prop_id: u64) -> String {
    let mut key = String::from_utf8_lossy(PROP_POWER_KEY).to_string();
    key.push_str(
        &prop_id
            .to_be_bytes()
            .to_vec()
            .iter()
            .map(|b| format!("{:02X}", b))
            .collect::<String>(),
    );
    key
}

pub fn get_prop_power_total(deps: Deps, prop_id: u64) -> StdResult<Decimal> {
    // get the key for the proposal
    let key = get_prop_power_key(prop_id);

    // return the total power for that proposal
    get_total_power(deps.storage, key.as_str())
}
