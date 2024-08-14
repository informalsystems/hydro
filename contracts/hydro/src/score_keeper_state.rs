// TOTAL_VOTED_POWER: key(round_id, tranche_id) -> score_keeper
pub const TOTAL_VOTED_POWER_KEY: &[u8] = b"total_voted_power";

pub fn get_total_power_key(round_id: u64, tranche_id: u64) -> Vec<u8> {
    let mut key = TOTAL_VOTED_POWER_KEY.to_vec();
    key.extend_from_slice(&round_id.to_be_bytes());
    key.extend_from_slice(&tranche_id.to_be_bytes());
    key
}

// TOTAL_ROUND_POWER: key(round_id) -> score_keeper
pub const TOTAL_ROUND_POWER_KEY: &[u8] = b"total_round_power";

pub fn get_total_round_power_key(round_id: u64) -> Vec<u8> {
    let mut key = TOTAL_ROUND_POWER_KEY.to_vec();
    key.extend_from_slice(&round_id.to_be_bytes());
    key
}
