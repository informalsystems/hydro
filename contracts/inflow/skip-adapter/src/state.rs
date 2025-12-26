use cosmwasm_schema::cw_serde;
use cosmwasm_std::Addr;
use cw_storage_plus::{Item, Map};

/// Contract configuration
#[cw_serde]
pub struct Config {
    /// Skip contract address on Neutron
    pub skip_contract: Addr,
    /// Default timeout for swap operations (nanoseconds)
    pub default_timeout_nanos: u64,
    /// Maximum allowed slippage in basis points (e.g., 100 = 1%)
    pub max_slippage_bps: u64,
}

/// Route configuration for a swap path
#[cw_serde]
pub struct RouteConfig {
    /// Starting denom
    pub denom_in: String,
    /// Final output denom
    pub denom_out: String,
    /// Full ordered path of denoms for validation
    /// Example: ["untrn", "ibc/B559A80D62249C8AA07A380E2A2BEA6E5CA9A6F079C912C3A9E9B494105E4F81", "ibc/0E293A7622DC9A6439DB60E6D234B5AF446962E27CA3AB44D0590603DFF6968E"]
    pub denoms_path: Vec<String>,
    /// Whether this route is currently enabled
    pub enabled: bool,
}

/// Recipient configuration for post-swap transfers
#[cw_serde]
pub struct RecipientConfig {
    /// Recipient address
    pub address: String,
    /// Optional description/label
    pub description: Option<String>,
    /// Whether this recipient is currently enabled
    pub enabled: bool,
}

/// Depositor information
#[cw_serde]
pub struct Depositor {
    /// Whether this depositor is currently enabled
    pub enabled: bool,
}

/// Denom to Slinky symbol mapping for oracle price queries
#[cw_serde]
pub struct DenomSymbolMapping {
    /// Slinky currency symbol (e.g., "NTRN", "USDT", "ATOM")
    pub symbol: String,
    /// Optional description for human readability
    pub description: Option<String>,
}

// Storage Items

/// Configuration storage
pub const CONFIG: Item<Config> = Item::new("config");

/// List of admin addresses (config management)
pub const ADMINS: Item<Vec<Addr>> = Item::new("admins");

/// List of executor addresses (can execute swaps)
pub const EXECUTORS: Item<Vec<Addr>> = Item::new("executors");

/// Maps depositor address to their info
pub const WHITELISTED_DEPOSITORS: Map<Addr, Depositor> = Map::new("whitelisted_depositors");

/// Maps route identifier to route configuration
/// Key: route_id (e.g., "untrn_to_usdt")
pub const ROUTE_REGISTRY: Map<String, RouteConfig> = Map::new("route_registry");

/// Maps recipient address to recipient configuration
pub const RECIPIENT_REGISTRY: Map<Addr, RecipientConfig> = Map::new("recipient_registry");

/// Maps denom to Slinky symbol for oracle price queries
pub const DENOM_SYMBOL_REGISTRY: Map<String, DenomSymbolMapping> =
    Map::new("denom_symbol_registry");
