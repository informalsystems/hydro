use cosmwasm_std::{DepsMut, MessageInfo, Response};
use cw_storage_plus::Item;
use neutron_sdk::bindings::{msg::NeutronMsg, query::NeutronQuery};

use crate::{
    error::{new_generic_error, ContractError},
    migration::{migrate::MigrateMsg, v_3_6_1::ConfigV3_6_1},
    state::{Config, CONFIG},
};

pub fn migrate_config(
    deps: &mut DepsMut<NeutronQuery>,
    info: MessageInfo,
    msg: MigrateMsg,
) -> Result<Response<NeutronMsg>, ContractError> {
    const OLD_CONFIG: Item<ConfigV3_6_1> = Item::new("config");

    let old_config = OLD_CONFIG
        .load(deps.storage)
        .map_err(|e| new_generic_error(format!("failed to load old configuration: {e}")))?;

    let control_center_contract = deps.api.addr_validate(&msg.control_center_addr)?;

    let token_info_provider_contract = match msg.token_info_provider_addr {
        None => None,
        Some(token_info_provider_contract) => {
            Some(deps.api.addr_validate(&token_info_provider_contract)?)
        }
    };

    let new_config = Config {
        deposit_denom: old_config.deposit_denom,
        vault_shares_denom: old_config.vault_shares_denom,
        max_withdrawals_per_user: old_config.max_withdrawals_per_user,
        control_center_contract: control_center_contract.clone(),
        token_info_provider_contract: token_info_provider_contract.clone(),
    };

    CONFIG.save(deps.storage, &new_config)?;

    Ok(Response::new()
        .add_attribute("action", "migrate_config")
        .add_attribute("sender", info.sender)
        .add_attribute(
            "control_center_contract",
            new_config.control_center_contract.to_string(),
        )
        .add_attribute(
            "token_info_provider_contract",
            new_config
                .token_info_provider_contract
                .map(|addr| addr.to_string())
                .unwrap_or_default(),
        ))
}
