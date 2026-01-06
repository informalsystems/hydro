use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Coin, StdResult, Storage};
use cw_storage_plus::Item;

#[cw_serde]
pub struct Config {
    pub admins: Vec<Addr>,
    pub control_centers: Vec<Addr>,
}

pub const CONFIG: Item<Config> = Item::new("config");

pub fn load_config(storage: &dyn Storage) -> StdResult<Config> {
    CONFIG.load(storage)
}

#[cw_serde]
#[derive(Default)]
pub enum ActionState {
    #[default]
    Idle,
    Forwarded,
    WithdrawReceiptTokens {
        recipient: Addr,
        coin: Coin,
    },
    WithdrawFunds {
        recipient: Addr,
        coin: Coin,
    },
}

#[cw_serde]
#[derive(Default)]
pub struct State {
    pub last_action: ActionState,
}

pub const STATE: Item<State> = Item::new("state");
