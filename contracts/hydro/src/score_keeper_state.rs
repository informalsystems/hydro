// TOTAL_VOTED_POWER: key(round_id, tranche_id) -> score_keeper
pub const TOTAL_VOTED_POWER_KEY: &[u8] = b"total_voted_power";

pub fn get_total_voted_power_key(round_id: u64, tranche_id: u64) -> String {
    let mut key = String::from_utf8_lossy(TOTAL_VOTED_POWER_KEY).to_string();
    key.push_str(
        &round_id
            .to_be_bytes()
            .to_vec()
            .iter()
            .map(|b| format!("{:02X}", b))
            .collect::<String>(),
    );
    key.push_str(
        &tranche_id
            .to_be_bytes()
            .to_vec()
            .iter()
            .map(|b| format!("{:02X}", b))
            .collect::<String>(),
    );
    key
}

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
