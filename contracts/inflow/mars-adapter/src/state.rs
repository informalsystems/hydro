use cosmwasm_schema::cw_serde;
use cosmwasm_std::Addr;
use cw_storage_plus::{Item, Map};

/// Contract configuration
#[cw_serde]
pub struct Config {
    /// Mars credit manager contract address
    pub mars_contract: Addr,
    /// List of supported token denoms (e.g., USDC IBC denom)
    pub supported_denoms: Vec<String>,
}

#[cw_serde]
pub struct Depositor {
    /// Mars credit account ID for this depositor
    pub mars_account_id: String,
    /// Whether this depositor is currently enabled
    pub enabled: bool,
}

// Every address in this list can change the configuration, register/unregister depositors.
pub const ADMINS: Item<Vec<Addr>> = Item::new("admins");

/// Maps depositor address to their Mars account ID
/// Key: Addr (depositor address), Value: Depositor (mars_account_id + enabled)
pub const WHITELISTED_DEPOSITORS: Map<Addr, Depositor> = Map::new("whitelisted_depositors");

/// Temporary storage for tracking which depositor address is being set up during replies
/// This is needed because reply handlers don't have access to the original message context
pub const PENDING_DEPOSITOR_SETUP: Item<Addr> = Item::new("pending_depositor_setup");

/// Configuration storage
pub const CONFIG: Item<Config> = Item::new("config");
