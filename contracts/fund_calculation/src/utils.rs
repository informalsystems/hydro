use cosmwasm_std::Coin;

pub struct CurrentHoldingsResponse {
    pub current_holings: Vec<Coin>,
}
