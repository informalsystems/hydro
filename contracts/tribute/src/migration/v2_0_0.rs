use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Coin, DepsMut, Order, StdResult, Storage, Timestamp};
use cw_storage_plus::{Item, Map};
use hydro::{
    contract::compute_round_end,
    query::{ConstantsResponse, QueryMsg as HydroQueryMsg},
    state::Constants,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    error::ContractError,
    migration::v1_1_1::{ConfigV1_1_1, TributeV1_1_1},
};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct MigrateMsgV2_0_0 {}

#[cw_serde]
pub struct ConfigV2_0_0 {
    pub hydro_contract: Addr,
}

#[cw_serde]
pub struct TributeV2_0_0 {
    pub round_id: u64,
    pub tranche_id: u64,
    pub proposal_id: u64,
    pub tribute_id: u64,
    pub depositor: Addr,
    pub funds: Coin,
    pub refunded: bool,
    pub creation_time: Timestamp,
    pub creation_round: u64,
}

impl TributeV2_0_0 {
    fn from(old_tribute: TributeV1_1_1, constants: &Constants) -> StdResult<Self> {
        let round_start =
            compute_round_end(constants, old_tribute.round_id)?.nanos() - constants.round_length;

        Ok(Self {
            round_id: old_tribute.round_id,
            tranche_id: old_tribute.tranche_id,
            proposal_id: old_tribute.proposal_id,
            tribute_id: old_tribute.tribute_id,
            depositor: old_tribute.depositor,
            funds: old_tribute.funds,
            refunded: old_tribute.refunded,
            // Set the tribute creation time to the start of the round in which the tribute was created
            creation_time: Timestamp::from_nanos(round_start),
            // In V1 tributes could only be added in current round
            creation_round: old_tribute.round_id,
        })
    }
}

// Migrating from 1.1.1 to 2.0.0 will:
// - Migrate the existing Config to remove "top_n_props_count" and "min_prop_percent_for_claimable_tributes" fields.
// - Migrate the existing Tributes to add "creation_round" and "creation_time" fields. This fields will be populated
//   with the round in which the tribute was created and with timestamp when the given round started.
pub fn migrate_v1_1_1_to_v2_0_0(deps: &mut DepsMut) -> Result<(), ContractError> {
    let new_config = migrate_config(deps.storage)?;
    migrate_tributes(deps, new_config)?;

    Ok(())
}

fn migrate_config(storage: &mut dyn Storage) -> Result<ConfigV2_0_0, ContractError> {
    const OLD_CONFIG: Item<ConfigV1_1_1> = Item::new("config");
    const NEW_CONFIG: Item<ConfigV2_0_0> = Item::new("config");

    let old_config = OLD_CONFIG.load(storage)?;
    let new_config = ConfigV2_0_0 {
        hydro_contract: old_config.hydro_contract,
    };

    NEW_CONFIG.save(storage, &new_config)?;

    Ok(new_config)
}

fn migrate_tributes(deps: &mut DepsMut, config: ConfigV2_0_0) -> Result<(), ContractError> {
    const OLD_ID_TO_TRIBUTE_MAP: Map<u64, TributeV1_1_1> = Map::new("id_to_tribute_map");
    const NEW_ID_TO_TRIBUTE_MAP: Map<u64, TributeV2_0_0> = Map::new("id_to_tribute_map");

    let constants_resp: ConstantsResponse = deps
        .querier
        .query_wasm_smart(config.hydro_contract, &HydroQueryMsg::Constants {})?;

    let mut new_tributes = vec![];

    for old_tribute in OLD_ID_TO_TRIBUTE_MAP.range(deps.storage, None, None, Order::Ascending) {
        let (_, old_tribute) = old_tribute?;

        let new_tribute = TributeV2_0_0::from(old_tribute, &constants_resp.constants)?;
        new_tributes.push(new_tribute);
    }

    for new_tribute in new_tributes {
        NEW_ID_TO_TRIBUTE_MAP.save(deps.storage, new_tribute.tribute_id, &new_tribute)?;
    }

    Ok(())
}
