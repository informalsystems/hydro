
use cosmwasm_std::{DepsMut, Response};
use neutron_sdk::bindings::{msg::NeutronMsg, query::NeutronQuery};

use crate::{
    error::{new_generic_error, ContractError},
    state::TOKEN_INFO_PROVIDERS,
    token_manager::{TokenInfoProvider, TokenInfoProviderLSM},
};

// Until the LSM token info provider becommes a separate smart contract, we use this string
// as its identifier in the store, instead of the smart contract address.
pub const LSM_TOKEN_INFO_PROVIDER_ID: &str = "lsm_token_info_provider";

// Hard-coded transfer channel ID, so that we don't have to keep the old TokenInfoProvider
// structures until the migration is completed. Only info that we need from the old struct
// is this one, and we know it always has this value.
const HUB_TRANSFER_CHANNEL_ID: &str = "channel-1";

// Replaces old LSM token info provider with the new one implemented as a smart contract.
pub fn migrate_lsm_token_info_provider(
    deps: &mut DepsMut<NeutronQuery>,
    lsm_token_info_provider: Option<String>,
) -> Result<Response<NeutronMsg>, ContractError> {
    // If there is a need to migrate some other contract instance to this version
    // except from ATOM one (e.g. NTRN instance) make sure it will be possible.
    let Some(lsm_token_info_provider) = lsm_token_info_provider else {
        return Ok(Response::new()
            .add_attribute("action", "migrate_lsm_token_info_provider")
            .add_attribute("lsm_token_info_provider_addr", "None"));
    };

    if !TOKEN_INFO_PROVIDERS.has(deps.storage, LSM_TOKEN_INFO_PROVIDER_ID.to_string()) {
        return Err(new_generic_error(
            "failed to migrate LSM token info provider- old provider not found in store",
        ));
    }

    TOKEN_INFO_PROVIDERS.remove(deps.storage, LSM_TOKEN_INFO_PROVIDER_ID.to_string());

    let lsm_token_info_provider_addr = deps.api.addr_validate(&lsm_token_info_provider)?;

    TOKEN_INFO_PROVIDERS.save(
        deps.storage,
        lsm_token_info_provider_addr.to_string(),
        &TokenInfoProvider::LSM(TokenInfoProviderLSM {
            contract: lsm_token_info_provider_addr.to_string(),
            cache: HashMap::new(),
            hub_transfer_channel_id: HUB_TRANSFER_CHANNEL_ID.to_string(),
        }),
    )?;

    Ok(Response::new()
        .add_attribute("action", "migrate_lsm_token_info_provider")
        .add_attribute(
            "lsm_token_info_provider_addr",
            lsm_token_info_provider_addr.to_string(),
        ))
}
