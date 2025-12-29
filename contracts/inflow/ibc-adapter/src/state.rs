use cosmwasm_schema::cw_serde;
use cosmwasm_std::Addr;
use cw_storage_plus::{Item, Map};

/// Contract configuration
#[cw_serde]
pub struct Config {
    /// Default IBC timeout in seconds
    pub default_timeout_seconds: u64,
}

/// Chain configuration for IBC transfers
#[cw_serde]
pub struct ChainConfig {
    /// Chain ID (e.g., "osmosis-1")
    pub chain_id: String,
    /// IBC channel from Neutron to this chain (e.g., "channel-0")
    pub channel_from_neutron: String,
    /// List of allowed recipient addresses on this chain
    /// Empty vector means all recipients are allowed
    pub allowed_recipients: Vec<String>,
}

/// Token configuration
#[cw_serde]
pub struct TokenConfig {
    /// Token denom (e.g., IBC denom like "ibc/27394FB...")
    pub denom: String,
    /// Source chain ID where the token originates (e.g., "osmosis-1")
    pub source_chain_id: String,
}

/// Depositor capabilities for the IBC adapter
#[cw_serde]
pub struct DepositorCapabilities {
    /// Whether this depositor can withdraw funds
    pub can_withdraw: bool,
    /// Whether this depositor can set custom memo in IBC transfers
    /// This should be restricted to trusted contracts (e.g., skip-adapter)
    pub can_set_memo: bool,
}

/// Depositor information
#[cw_serde]
pub struct Depositor {
    /// Whether this depositor is currently enabled
    pub enabled: bool,
    /// Depositor-specific capabilities
    pub capabilities: DepositorCapabilities,
}

/// Instructions for IBC transfer routing (used with TransferFunds message)
#[cw_serde]
pub struct TransferFundsInstructions {
    /// Destination chain ID (e.g., "osmosis-1")
    pub destination_chain: String,
    /// Recipient address on the destination chain
    pub recipient: String,
    /// Optional timeout override in seconds
    /// If None, uses the default from Config
    pub timeout_seconds: Option<u64>,
    /// Optional memo for IBC transfer (used for PFM, wasm hooks, etc.)
    /// If None, defaults to empty string
    pub memo: Option<String>,
}

// Storage items

/// Configuration storage
pub const CONFIG: Item<Config> = Item::new("config");

/// List of admin addresses who can manage the adapter (config admins)
pub const ADMINS: Item<Vec<Addr>> = Item::new("admins");

/// List of executor addresses who can call TransferFunds
pub const EXECUTORS: Item<Vec<Addr>> = Item::new("executors");

/// Maps depositor address to their info (enabled status + capabilities)
pub const WHITELISTED_DEPOSITORS: Map<Addr, Depositor> = Map::new("whitelisted_depositors");

/// Maps chain_id to chain configuration (channel + allowed recipients)
pub const CHAIN_REGISTRY: Map<String, ChainConfig> = Map::new("chain_registry");

/// Maps token denom to token configuration (denom + source chain)
/// This registry defines which tokens are supported by the adapter
pub const TOKEN_REGISTRY: Map<String, TokenConfig> = Map::new("token_registry");
