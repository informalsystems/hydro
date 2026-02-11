use cosmwasm_schema::cw_serde;
use cosmwasm_std::{entry_point, Addr, Decimal, DepsMut, Env, Response, StdError};
use cw2::{get_contract_version, set_contract_version};
use interface::inflow_control_center::{FeeConfig, FeeConfigInit};
use neutron_sdk::bindings::{msg::NeutronMsg, query::NeutronQuery};

use crate::{
    contract::{query_pool_info, CONTRACT_NAME, CONTRACT_VERSION},
    error::ContractError,
    state::{FEE_CONFIG, HIGH_WATER_MARK_PRICE},
};

#[cw_serde]
pub struct MigrateMsg {
    /// Fee configuration to initialize during migration.
    /// If None and FEE_CONFIG doesn't exist, fees are disabled by default.
    pub fee_config: Option<FeeConfigInit>,
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    msg: MigrateMsg,
) -> Result<Response<NeutronMsg>, ContractError> {
    check_contract_version(deps.storage)?;

    // Initialize fee config if not already present
    // Fees are enabled when fee_rate > 0
    if FEE_CONFIG.may_load(deps.storage)?.is_none() {
        let fee_config = match msg.fee_config {
            Some(init) => {
                if init.fee_rate > Decimal::one() {
                    return Err(ContractError::InvalidFeeRate);
                }
                let fee_recipient = deps.api.addr_validate(&init.fee_recipient)?;
                FeeConfig {
                    fee_rate: init.fee_rate,
                    fee_recipient,
                }
            }
            None => {
                // Default: fees disabled (fee_rate = 0)
                FeeConfig {
                    fee_rate: Decimal::zero(),
                    fee_recipient: Addr::unchecked(""),
                }
            }
        };

        FEE_CONFIG.save(deps.storage, &fee_config)?;

        // Set high-water mark to current share price
        let pool_info = query_pool_info(&deps.as_ref(), &env)?;
        let current_share_price = if pool_info.total_shares_issued.is_zero() {
            Decimal::one()
        } else {
            Decimal::from_ratio(pool_info.total_pool_value, pool_info.total_shares_issued)
        };
        HIGH_WATER_MARK_PRICE.save(deps.storage, &current_share_price)?;
    }

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    Ok(Response::new().add_attribute("action", "migrate"))
}

fn check_contract_version(storage: &dyn cosmwasm_std::Storage) -> Result<(), ContractError> {
    let contract_version = get_contract_version(storage)?;

    if contract_version.version == CONTRACT_VERSION {
        return Err(ContractError::Std(StdError::generic_err(
            "Contract is already migrated to the newest version.",
        )));
    }

    Ok(())
}
