use crate::contract::{CONTRACT_NAME, CONTRACT_VERSION};
use crate::error::{new_generic_error, ContractError};
use crate::state::{BLOCKED_LOCK_DENOMS, CONSTANTS};
use crate::utils::load_constants_active_at_timestamp;
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{DepsMut, Env, IbcMsg, IbcTimeout, Response, StdError};
use cw2::{get_contract_version, set_contract_version};
// entry_point is being used but for some reason clippy doesn't see that, hence the allow attribute here
#[allow(unused_imports)]
use cosmwasm_std::entry_point;

/// Default timeout of the ICS-20 transfer sending the stATOM to Stride: 14 days.
pub const DEFAULT_STRIDE_TRANSFER_TIMEOUT_SECONDS: u64 = 14 * 24 * 60 * 60;

pub const STRIDE_ADDRESS_PREFIX: &str = "stride1";

#[cw_serde]
pub struct MigrateMsg {
    /// Denom of stATOM on the Cosmos Hub (an IBC denom). The contract's entire balance of this
    /// denom is sent to Stride, and the denom is blocked from being put into new lockups.
    pub statom_denom: String,
    /// Cosmos Hub side of the ICS-20 transfer channel to Stride used to send the stATOM.
    pub stride_channel_id: String,
    /// Address on Stride that receives the stATOM.
    pub stride_receiver: String,
    /// Number of seconds after the migration block time after which the transfer times out.
    /// On timeout, the stATOM is refunded to this contract. Defaults to 14 days.
    pub transfer_timeout_seconds: Option<u64>,
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, env: Env, msg: MigrateMsg) -> Result<Response, ContractError> {
    check_contract_version(deps.storage)?;

    let timeout_seconds = validate_migrate_msg(&msg)?;

    // Block the stATOM denom from being put into any new lockup.
    let mut blocked_lock_denoms = BLOCKED_LOCK_DENOMS
        .may_load(deps.storage)?
        .unwrap_or_default();
    if !blocked_lock_denoms.contains(&msg.statom_denom) {
        blocked_lock_denoms.push(msg.statom_denom.clone());
    }
    BLOCKED_LOCK_DENOMS.save(deps.storage, &blocked_lock_denoms)?;

    // Send the entire stATOM balance of the contract to Stride.
    let statom_balance = deps
        .querier
        .query_balance(env.contract.address.clone(), msg.statom_denom.clone())?;

    let timeout_timestamp = env.block.time.plus_seconds(timeout_seconds);

    let mut response = Response::new()
        .add_attribute("action", "migrate")
        .add_attribute("blocked_lock_denoms", blocked_lock_denoms.join(","))
        .add_attribute("statom_sent_to_stride", statom_balance.to_string());

    if !statom_balance.amount.is_zero() {
        response = response
            .add_message(IbcMsg::Transfer {
                channel_id: msg.stride_channel_id.clone(),
                to_address: msg.stride_receiver.clone(),
                amount: statom_balance,
                timeout: IbcTimeout::with_timestamp(timeout_timestamp),
                memo: None,
            })
            .add_attribute("stride_channel_id", msg.stride_channel_id)
            .add_attribute("stride_receiver", msg.stride_receiver)
            .add_attribute("transfer_timeout_timestamp", timeout_timestamp.to_string());
    }

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    Ok(response)
}

/// Validates the migrate message and returns the transfer timeout in seconds.
fn validate_migrate_msg(msg: &MigrateMsg) -> Result<u64, ContractError> {
    if msg.statom_denom.trim().is_empty() {
        return Err(new_generic_error("statom_denom must not be empty"));
    }

    if msg.stride_channel_id.trim().is_empty() {
        return Err(new_generic_error("stride_channel_id must not be empty"));
    }

    if !msg.stride_receiver.starts_with(STRIDE_ADDRESS_PREFIX)
        || msg.stride_receiver.len() <= STRIDE_ADDRESS_PREFIX.len()
    {
        return Err(new_generic_error(format!(
            "stride_receiver must be a Stride address starting with '{STRIDE_ADDRESS_PREFIX}'"
        )));
    }

    let timeout_seconds = msg
        .transfer_timeout_seconds
        .unwrap_or(DEFAULT_STRIDE_TRANSFER_TIMEOUT_SECONDS);
    if timeout_seconds == 0 {
        return Err(new_generic_error(
            "transfer_timeout_seconds must be greater than zero",
        ));
    }

    Ok(timeout_seconds)
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

// Keep in mind that this function will store the new version of the Constants into the store.
// If there were some changes to the Constants structure, make sure to first migrate the constants,
// and then do the migration that requires pausing/unpausing the contract. Otherwise, the contract
// store will be broken.
fn _pause_contract_before_migration(deps: &mut DepsMut, env: &Env) -> Result<(), ContractError> {
    let (timestamp, mut constants) =
        load_constants_active_at_timestamp(&deps.as_ref(), env.block.time)?;

    if !constants.paused {
        constants.paused = true;
        CONSTANTS.save(deps.storage, timestamp, &constants)?;
    }

    Ok(())
}

fn _unpause_contract_after_migration(deps: &mut DepsMut, env: Env) -> Result<(), ContractError> {
    let (timestamp, mut constants) =
        load_constants_active_at_timestamp(&deps.as_ref(), env.block.time)?;

    if !constants.paused {
        return Err(new_generic_error(
            "Contract is already unpaused. Error in migration process",
        ));
    }

    constants.paused = false;
    CONSTANTS.save(deps.storage, timestamp, &constants)?;

    Ok(())
}
