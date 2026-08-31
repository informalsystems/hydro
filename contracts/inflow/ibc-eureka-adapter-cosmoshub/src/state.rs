use cosmwasm_schema::cw_serde;
use cosmwasm_std::Addr;
use cw_storage_plus::{Item, Map};

/// One adapter instance targets a single destination chain (one IBC Eureka
/// channel + one Eureka entry contract), but can bridge multiple token denoms
/// over that same channel.
#[cw_serde]
pub struct Config {
    /// Address of the Skip swap entry-point contract on Cosmos Hub
    pub skip_swap_entry_point_contract: String,
    /// IBC Eureka source channel ID used for bridging to the destination chain
    pub source_channel: String,
    /// Address that receives the IBC Eureka relayer fee
    pub eureka_fee_receiver: String,
    /// Payload encoding expected by the destination chain ("application/x-solidity-abi" for EVM)
    pub encoding: String,
    /// Timeout, in seconds, to be used when building IBC Eureka transfer messages
    pub ibc_transfer_timeout_seconds: u64,
    /// Timeout, in seconds, for the Eureka relayer fees to be received (converted to nanoseconds when building the message)
    pub eureka_fee_timeout_seconds: u64,
}

/// Depositor capabilities for the IBC Eureka adapter
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

/// List of admin addresses who can manage the adapter
pub const ADMINS: Item<Vec<Addr>> = Item::new("admins");

/// Maps executor address to empty tuple (just tracks existence)
pub const EXECUTORS: Map<Addr, ()> = Map::new("executors");

/// Maps depositor address to their info (enabled status + capabilities)
pub const WHITELISTED_DEPOSITORS: Map<Addr, Depositor> = Map::new("whitelisted_depositors");

/// Maps denom to empty tuple (just tracks existence of allowed denoms).
/// Allows a single adapter instance to bridge multiple token denoms (e.g.
/// wBTC, wETH, USDT), which all share the same Eureka channel.
pub const ALLOWED_DENOMS: Map<String, ()> = Map::new("allowed_denoms");

/// Maps normalized (i.e. lowercase, no "0x" prefix) EVM destination
/// address to empty tuple (just tracks existence of allowed addresses).
pub const ALLOWED_DESTINATION_ADDRESSES: Map<String, ()> =
    Map::new("allowed_destination_addresses");
