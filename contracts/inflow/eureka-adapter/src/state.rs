use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Uint128};
use cw_storage_plus::{Item, Map};

/// Contract configuration
#[cw_serde]
pub struct Config {
    /// Skip swap entry point contract on Cosmos Hub
    pub skip_entry_point: String,
    /// Skip swap IBC adapter contract on Cosmos Hub (also used as outer IBC receiver and dest_callback address)
    pub skip_ibc_adapter: String,
    /// IBC channel from Neutron to Cosmos Hub (e.g. "channel-1")
    pub neutron_to_hub_channel: String,
    /// Default IBC timeout in seconds for the Neutron→Hub transfer
    pub ibc_default_timeout_seconds: u64,
}

/// Chain configuration for a target EVM chain reachable via IBC Eureka
#[cw_serde]
pub struct ChainConfig {
    /// Internal chain identifier (e.g. "ethereum-1")
    pub chain_id: String,
    /// IBC Eureka source channel on Cosmos Hub to this EVM chain (e.g. "08-wasm-1369")
    pub eureka_source_channel: String,
    /// Cosmos Hub address that receives the Eureka relay fee
    pub eureka_fee_receiver: String,
    /// Minimum allowed Eureka fee amount (in the bridged token denom)
    pub min_eureka_fee: Uint128,
    /// Maximum allowed Eureka fee amount (in the bridged token denom)
    pub max_eureka_fee: Uint128,
}

/// Token configuration for a supported bridgeable token
#[cw_serde]
pub struct TokenConfig {
    /// Token denom on Neutron (e.g. "ibc/0E293...")
    pub denom: String,
    /// Same token's denom as seen on Cosmos Hub (used in eureka_fee.coin.denom)
    pub hub_denom: String,
}

/// Depositor capabilities
#[cw_serde]
pub struct DepositorCapabilities {
    /// Whether this depositor can withdraw funds from the adapter
    pub can_withdraw: bool,
}

/// Depositor information
#[cw_serde]
pub struct Depositor {
    /// Whether this depositor is currently enabled
    pub enabled: bool,
    /// Depositor-specific capabilities
    pub capabilities: DepositorCapabilities,
}

/// Instructions for an Eureka bridge transfer
#[cw_serde]
pub struct TransferFundsInstructions {
    /// Target EVM chain ID from CHAIN_REGISTRY (e.g. "ethereum-1")
    pub chain_id: String,
    /// Recipient EVM address in hex format — validated against ALLOWED_DESTINATION_ADDRESSES
    pub recipient: String,
    /// Cosmos Hub address to receive funds if the Eureka transfer fails — validated against ALLOWED_RECOVER_ADDRESSES
    pub recover_address: String,
    /// Token denom to bridge — must be registered in TOKEN_REGISTRY
    pub denom: String,
    /// Amount to bridge (excluding Eureka fee; fee is provided via info.funds)
    pub amount: Uint128,
}

// Storage items

pub const CONFIG: Item<Config> = Item::new("config");
pub const ADMINS: Item<Vec<Addr>> = Item::new("admins");
/// Maps executor address to empty tuple (existence = registered)
pub const EXECUTORS: Map<Addr, ()> = Map::new("executors");
/// Maps depositor address to their info
pub const WHITELISTED_DEPOSITORS: Map<Addr, Depositor> = Map::new("whitelisted_depositors");
/// Maps chain_id to chain configuration
pub const CHAIN_REGISTRY: Map<String, ChainConfig> = Map::new("chain_registry");
/// Maps token denom (on Neutron) to token configuration
pub const TOKEN_REGISTRY: Map<String, TokenConfig> = Map::new("token_registry");
/// Maps (chain_id, hex_evm_address) to empty tuple — the EVM address allowlist
pub const ALLOWED_DESTINATION_ADDRESSES: Map<(String, String), ()> =
    Map::new("allowed_destination_addresses");
/// Maps cosmos bech32 address to empty tuple — allowed recover addresses
pub const ALLOWED_RECOVER_ADDRESSES: Map<String, ()> = Map::new("allowed_recover_addresses");
