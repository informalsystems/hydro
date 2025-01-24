use std::collections::HashMap;

use cosmwasm_std::{
    entry_point, to_json_binary, Binary, Coin, Deps, DepsMut, Env, MessageInfo, Response, StdError,
    StdResult,
};
use cw2::set_contract_version;

use crate::error::ContractError;
use crate::msg::{ExecuteMsg, InstantiateMsg};
use crate::query::{ConfigResponse, QueryMsg};
use crate::state::{
    Config, GlobalConfig, Proposal, ProposalAllocation, Venue, VenueConfig, VenueType, CONFIG,
};

/// Contract name that is used for migration.
pub const CONTRACT_NAME: &str = env!("CARGO_PKG_NAME");
/// Contract version that is used for migration.
pub const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    let config = Config {
        hydro_contract: deps.api.addr_validate(&msg.hydro_contract)?,
        tribute_contract: deps.api.addr_validate(&msg.tribute_contract)?,
        venue_configs: HashMap::new(),
        global_config: msg.global_config,
        venue_groups: HashMap::new(),
    };

    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new().add_attribute("action", "initialisation"))
}

// Execute

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    unimplemented!()
}

// Queries

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&query_config(deps)?),
    }
}

fn query_config(deps: Deps) -> StdResult<ConfigResponse> {
    Ok(ConfigResponse {
        config: CONFIG.load(deps.storage)?,
    })
}

impl Config {
    pub fn get_venue_config(&self, venue_id: &u64) -> Option<&VenueConfig> {
        self.venue_configs.get(venue_id)
    }

    pub fn get_tvl_factor_for_venue_type(&self, venue_type: &VenueType) -> Option<f64> {
        self.global_config
            .venue_type_to_existing_tvl_factor
            .iter()
            .find(|(vt, _)| vt == venue_type)
            .map(|(_, factor)| *factor)
    }

    pub fn compute_deployment_limit(
        &self,
        venue: &Venue,
        current_tvl: u64,
    ) -> Result<u64, StdError> {
        let venue_config = self
            .get_venue_config(&venue.id)
            .ok_or_else(|| StdError::generic_err("No venue config found for venue"))?;

        // Get TVL factor for this venue type
        let tvl_factor = self
            .get_tvl_factor_for_venue_type(&venue_config.venue_type)
            .ok_or_else(|| {
                StdError::generic_err(format!(
                    "No TVL factor configured for venue type {:?}",
                    venue_config.venue_type
                ))
            })?;

        // Calculate max deployment based on TVL factor
        let max_by_tvl = (current_tvl as f64 * tvl_factor) as u64;

        // Use venue-specific bootstrap limit if set, otherwise use global
        let bootstrap_limit = venue_config
            .bootstrap_limit_override
            .unwrap_or(self.global_config.bootstrap_limit);

        // Return max of TVL-based limit and bootstrap limit
        Ok(std::cmp::max(max_by_tvl, bootstrap_limit))
    }

    pub fn compute_liquidity_allocations(
        config: Config,
        proposals: Vec<Proposal>,
        // an oracle that is given denoms to be queried and returns their price in the base denom
        price_oracle: &dyn Fn(String) -> u64,
        current_holdings_oracle: &dyn Fn(u64) -> Vec<Coin>,
        current_total_venue_liquidity_oracle: &dyn Fn(u64) -> Vec<Coin>,
    ) -> Vec<ProposalAllocation> {
        // Calculate total power
        let total_power: u64 = proposals.iter().map(|p| p.power).sum();
        if total_power == 0 {
            return Ok((vec![], self.global_config.total_allocated));
        }

        // Initialize allocation tracking
        let mut proposal_allocations = Vec::new();
        let mut venue_group_used = HashMap::new();
        for group in self.venue_groups.values() {
            venue_group_used.insert(group.id, 0u64);
        }
    }
}
