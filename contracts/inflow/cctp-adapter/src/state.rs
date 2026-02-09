use cosmwasm_schema::cw_serde;
use cosmwasm_std::Addr;
use cw_storage_plus::{Item, Map};

/// Contract configuration
#[cw_serde]
pub struct Config {
    /// The token denom this adapter handles (i.e. USDC denom on Neutron)
    pub denom: String,
    /// Neutron-Noble transfer channel ID
    pub noble_transfer_channel_id: String,
    /// Default IBC timeout in seconds for IBC transfers
    pub ibc_default_timeout_seconds: u64,
}

/// Chain configuration for EVM chains
#[cw_serde]
pub struct ChainConfig {
    /// Chain ID; Our internal identifier for the destination EVM chain. Not used for CCTP bridging.
    pub chain_id: String,
    /// Bridging configuration for this chain
    pub bridging_config: BridgingConfig,
}

/// Bridging configuration for CCTP
#[cw_serde]
pub struct BridgingConfig {
    /// Noble receiver address for CCTP transfers (i.e. Noble Orbiter module address)
    pub noble_receiver: String,
    /// Noble fee recipient address (i.e. Skip's relayer address on Noble)
    pub noble_fee_recipient: String,
    /// CCTP destination domain identifier. See the full list here: https://developers.circle.com/cctp/v1/supported-domains
    pub destination_domain: u64,
    /// EVM destination caller address (i.e. Skip's relayer address on destination EVM chain)
    pub evm_destination_caller: String,
}

/// Depositor capabilities for the CCTP adapter
#[cw_serde]
pub struct DepositorCapabilities {
    /// Whether this depositor can withdraw funds
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

/// Configuration storage
pub const CONFIG: Item<Config> = Item::new("config");

/// List of admin addresses who can manage the adapter (config admins)
pub const ADMINS: Item<Vec<Addr>> = Item::new("admins");

/// Maps executor address to empty tuple (just tracks existence)
pub const EXECUTORS: Map<Addr, ()> = Map::new("executors");

/// Maps depositor address to their info (enabled status + capabilities)
pub const WHITELISTED_DEPOSITORS: Map<Addr, Depositor> = Map::new("whitelisted_depositors");

/// Maps chain_id to chain configuration. Allows support for multiple EVM chains.
pub const CHAIN_REGISTRY: Map<String, ChainConfig> = Map::new("chain_registry");

/// Maps (chain_id, hex_evm_address) to destination address info
pub const ALLOWED_DESTINATION_ADDRESSES: Map<(String, String), DestinationAddress> =
    Map::new("allowed_destination_addresses");

/// Destination address information for EVM chains
#[cw_serde]
pub struct DestinationAddress {
    /// EVM address
    pub address: String,
    /// Optional protocol identifier (e.g. "uniswap-v3", "aave-v3", etc.)
    pub protocol: String,
}

/// Instructions for CCTP transfer via Noble to EVM chain
#[cw_serde]
pub struct TransferFundsInstructions {
    /// Target EVM chain ID from CHAIN_REGISTRY
    pub chain_id: String,
    /// Recipient address on destination chain (hex format, will be looked up in ALLOWED_DESTINATION_ADDRESSES)
    pub recipient: String,
}
