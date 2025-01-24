use std::collections::HashMap;

use cosmwasm_schema::cw_serde;
use cosmwasm_std::Addr;
use cw_storage_plus::Item;

pub const CONFIG: Item<Config> = Item::new("config");

// The principal denom that funds are calculated and allocated in.
pub const BASE_DENOM: &str = "uatom";

#[cw_serde]
pub struct Config {
    pub hydro_contract: Addr,
    pub tribute_contract: Addr,
    // Maps venue_ids to their configs
    pub venue_configs: HashMap<u64, VenueConfig>,
    pub global_config: GlobalConfig,
    // Maps each venue_group_id to its VenueGroup struct
    pub venue_groups: HashMap<u64, VenueGroup>,
}

#[cw_serde]
pub struct Proposal {
    pub id: u64,
    pub power: u64,
    pub venues: Vec<Venue>,
}

// A venue is a target liquidity allocation for a specific proposal
// to deploy funds into one specific liquidity location, i.e. a DEX pool or a lending protocol.
// Any parameters in this struct are set by bidders autonomously.
#[cw_serde]
pub struct Venue {
    pub id: u64,
    // the target liquidity allocation in the base denom
    pub target_allocation: u64,
    // the weight by which extra funds, after all target allocations are fulfilled, are distributed
    pub surplus_allocation_weight: u64,
    pub deployment_params: String,
}

// A global configuration for a venue.
// These are configuration parameters that are set by
// the governance contract.
#[cw_serde]
pub struct VenueConfig {
    pub id: u64,
    pub name: String,
    pub venue_type: VenueType,
    pub bootstrap_limit_override: Option<u64>,
}

#[cw_serde]
pub enum VenueType {
    // Liquidity provision for pools in an exchange, e.g. Osmosis, Astroport, etc.
    Exchange,
    // Lending out principal assets on money markets, e.g. Nolus, Mars.
    Lending,
}

// The GlobalConfig contains configuration parameters that relate to
// all venues and proposals, and can be changed by the
#[cw_serde]
pub struct GlobalConfig {
    // For each venue type, the existing TVL factor is multiplied by this factor
    // to determine the maximum amount of funds that can be deployed into a venue.
    pub venue_type_to_existing_tvl_factor: Vec<(VenueType, f64)>,

    // The minimal amount of funds we want to deploy into venues, even if the
    // existing TVL factor would allow for less, to "bootstrap" venues.
    pub bootstrap_limit: u64,

    // The total amount of funds that will be distributed.
    pub total_allocated: u64,
}

// A venue group is a collection of venues that share
// a common total limit for the amount of funds that can be deployed into them.
// For example, this could be: all venues on a specific chain.
#[cw_serde]
pub struct VenueGroup {
    pub member_venue_ids: Vec<u64>,
    pub total_limit: u64,
}

// A venue allocation is a liquidity allocation for a specific venue.
#[cw_serde]
pub struct VenueAllocation {
    // the id of the venue to be allocated the liquidity
    pub venue_id: u64,
    // the amount of liquidity, measured in the base denom, to deploy to this venue
    pub amount: u64,
    // the parameters to be used for deployment
    pub deployment_params: String,
}

#[cw_serde]
pub struct ProposalAllocation {
    pub proposal: Proposal,
    pub allocations: Vec<VenueAllocation>,
}
