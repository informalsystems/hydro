use cosmwasm_schema::cw_serde;
use cosmwasm_std::Addr;
use cw_storage_plus::Item;

pub const CONFIG: Item<Config> = Item::new("config");

#[cw_serde]
pub struct Config {
    pub hydro_contract: Addr,
}

pub struct Proposal {
    pub id: u64,
    pub venues: Vec<Venue>,
}

// A venue is a target liquidity allocation for a specific proposal
// to deploy funds into one specific liquidity location, i.e. a DEX pool or a lending protocol.
// Any parameters in this struct are set by bidders autonomously.
pub struct Venue {
    pub id: u64,
    // the target liquidity allocation in ATOM
    pub target_allocation: u64,
    // the weight by which extra funds, after all target allocations are fulfilled, are distributed
    pub surplus_allocation_weight: u64,
    pub deployment_params: String,
}

// A global configuration for a venue.
// These are configuration parameters that are set by
// the governance contract.
pub struct VenueConfig {
    pub id: u64,
    pub name: String,
    pub venue_type: VenueType,
    pub bootstrap_limit_override: u64,
}

pub struct GlobalConfig {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VenueType {
    // Liquidity provision for pools in an exchange, e.g. Osmosis, Astroport, etc.
    Exchange,
    // Lending out principal assets on money markets, e.g. Nolus, Mars.
    Lending,
}

pub struct GlobalConfig {
    // For each venue type, the existing TVL factor is multiplied by this factor
    // to determine the maximum amount of funds that can be deployed into a venue.
    pub venue_type_to_existing_tvl_factor: HashMap<VenueType, u64>,

    // The minimal amount of funds we want to deploy into venues, even if the
    // existing TVL factor would allow for less, to "bootstrap" venues.
    pub bootstrap_limit: u64,
}
