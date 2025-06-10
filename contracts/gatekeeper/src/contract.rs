#![allow(unused_imports)]
use cosmwasm_std::{
    entry_point, from_json, to_json_binary, Binary, Deps, DepsMut, Env, MessageInfo, Order,
    Response, StdError, StdResult, Timestamp,
};
use cw2::set_contract_version;
use cw_storage_plus::Bound;
use interface::gatekeeper::ExecuteLockTokensMsg;
use sha2::Digest;
use std::collections::HashSet;

use crate::error::{new_generic_error, ContractError};
use crate::msg::{ExecuteMsg, InstantiateMsg};
use crate::query::{
    AdminsResponse, ConfigResponse, CurrentEpochUserLockedResponse, CurrentStageResponse, QueryMsg,
};
use crate::state::{
    Config, StageData, ADMINS, CONFIG, EPOCHS, STAGES, STAGE_ID, USER_LOCK_AMOUNTS,
};
use crate::utils::{CosmosSignature, SignatureInfo};

/// Contract name that is used for migration.
pub const CONTRACT_NAME: &str = env!("CARGO_PKG_NAME");
/// Contract version that is used for migration.
pub const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    let config = Config {
        hydro_contract: info.sender.clone(),
    };

    CONFIG.save(deps.storage, &config)?;
    STAGE_ID.save(deps.storage, &0)?;

    let mut admins: HashSet<String> = HashSet::new();

    for admin in msg.admins {
        let admin_addr = deps.api.addr_validate(&admin)?;
        admins.insert(admin_addr.to_string());

        ADMINS.save(deps.storage, admin_addr, &())?;
    }

    if admins.is_empty() {
        return Err(new_generic_error("At least one admin must be specified."));
    }

    Ok(Response::new()
        .add_attribute("action", "initialisation")
        .add_attribute("sender", info.sender)
        .add_attribute(
            "admins",
            admins.into_iter().collect::<Vec<String>>().join(","),
        ))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::RegisterStage {
            activate_at,
            merkle_root,
            start_new_epoch,
            hrp,
        } => execute_register_stage(
            deps,
            env,
            info,
            merkle_root,
            activate_at,
            start_new_epoch,
            hrp,
        ),
        ExecuteMsg::LockTokens(lock_tokens_msg) => {
            execute_lock_tokens(deps, env, info, lock_tokens_msg)
        }
        ExecuteMsg::AddAdmin { admin } => execute_add_admin(deps, info, admin),
        ExecuteMsg::RemoveAdmin { admin } => execute_remove_admin(deps, info, admin),
    }
}

fn execute_register_stage(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    merkle_root: String,
    activate_at: Timestamp,
    start_new_epoch: bool,
    hrp: Option<String>,
) -> Result<Response, ContractError> {
    validate_sender_is_admin(&deps, &info)?;

    if activate_at < env.block.time {
        return Err(new_generic_error(
            "Cannot register new stage for the timestamp in past.",
        ));
    }

    // Using exclusive range start allows us to replace the latest stage with a new one
    // starting at the same time in case if we need to do some corrections.
    if STAGES
        .range(
            deps.storage,
            Some(Bound::exclusive(activate_at.nanos())),
            None,
            Order::Ascending,
        )
        .any(|_| true)
    {
        return Err(new_generic_error(
            "Cannot register new stage at a timestamp earlier than already existing stage.",
        ));
    }

    let stage_id = STAGE_ID.load(deps.storage)?;
    let stage_data = StageData {
        stage_id,
        activate_at,
        merkle_root: merkle_root.clone(),
        hrp: hrp.clone(),
    };

    STAGES.save(deps.storage, activate_at.nanos(), &stage_data)?;
    STAGE_ID.save(deps.storage, &(stage_id + 1))?;

    // The first stage must be marked as the begining of the (first) epoch in order to initiate the tracking of
    // which stages belong to which epoch. For later stages, admins can choose when they want to start a new epoch.
    if stage_id == 0 || start_new_epoch {
        EPOCHS.save(deps.storage, stage_id, &())?;
    }

    Ok(Response::new()
        .add_attribute("action", "register_stage")
        .add_attribute("sender", info.sender)
        .add_attribute("stage_id", stage_id.to_string())
        .add_attribute("merkle_root", merkle_root)
        .add_attribute("activate_at", activate_at.to_string())
        .add_attribute("start_new_epoch", start_new_epoch.to_string())
        .add_attribute("hrp", hrp.unwrap_or_default()))
}

fn execute_lock_tokens(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    lock_tokens_msg: ExecuteLockTokensMsg,
) -> Result<Response, ContractError> {
    validate_sender_is_hydro_contract(&deps, &info)?;

    let user_address = deps.api.addr_validate(&lock_tokens_msg.user_address)?;
    let current_stage = get_current_stage(&deps.as_ref(), &env)?;
    let epoch_start_stage_id =
        get_epoch_start_for_stage_id(&deps.as_ref(), current_stage.stage_id)?;

    let proof_addr = match lock_tokens_msg.sig_info {
        None => user_address.to_string(),
        Some(sig) => {
            // Convert into internal representation
            let sig = SignatureInfo::convert(sig);

            // Verify signature
            let cosmos_signature: CosmosSignature = from_json(&sig.signature)?;
            cosmos_signature.verify(deps.as_ref(), &sig.claim_msg)?;

            // Get stage bech32 prefix and derive proof address from public key
            let hrp = match current_stage.hrp {
                Some(hrp) => hrp,
                None => {
                    return Err(new_generic_error(
                        "Signature info provided but wasn't expected.",
                    ));
                }
            };

            let proof_addr = cosmos_signature.derive_addr_from_pubkey(hrp.as_str())?;

            let signed_address = sig.extract_addr_from_claim_msg()?;
            if signed_address != user_address.to_string() {
                return Err(new_generic_error(
                    format!("Signature verification failed. Signed address ({}) doesn't match the expected one ({}).",
                    signed_address, user_address)));
            }

            proof_addr
        }
    };

    let user_input = format!("{}{}", proof_addr, lock_tokens_msg.maximum_amount);
    let hash = sha2::Sha256::digest(user_input.as_bytes())
        .as_slice()
        .try_into()
        .map_err(|_| ContractError::WrongLength {})?;

    let hash = lock_tokens_msg
        .proof
        .into_iter()
        .try_fold(hash, |hash, p| {
            let mut proof_buf = [0; 32];
            hex::decode_to_slice(p, &mut proof_buf)?;
            let mut hashes = [hash, proof_buf];
            hashes.sort_unstable();
            sha2::Sha256::digest(hashes.concat())
                .as_slice()
                .try_into()
                .map_err(|_| ContractError::WrongLength {})
        })?;

    let mut root_buf: [u8; 32] = [0; 32];
    hex::decode_to_slice(current_stage.merkle_root, &mut root_buf)?;

    if root_buf != hash {
        return Err(new_generic_error(
            "Failed to verify provided proofs against the current stage root.",
        ));
    }

    let already_locked = USER_LOCK_AMOUNTS
        .may_load(deps.storage, (user_address.clone(), epoch_start_stage_id))?
        .unwrap_or_default();

    if already_locked + lock_tokens_msg.amount_to_lock > lock_tokens_msg.maximum_amount {
        return Err(new_generic_error(format!(
            "User cannot lock {} tokens. Currently locked: {}. Maximum allowed to lock: {}.",
            lock_tokens_msg.amount_to_lock, already_locked, lock_tokens_msg.maximum_amount
        )));
    }

    USER_LOCK_AMOUNTS.save(
        deps.storage,
        (user_address.clone(), epoch_start_stage_id),
        &(already_locked + lock_tokens_msg.amount_to_lock),
    )?;

    Ok(Response::new()
        .add_attribute("action", "lock_tokens")
        .add_attribute("sender", info.sender)
        .add_attribute("user_address", user_address.clone())
        .add_attribute("amount_to_lock", lock_tokens_msg.amount_to_lock.to_string()))
}

fn execute_add_admin(
    deps: DepsMut,
    info: MessageInfo,
    admin: String,
) -> Result<Response, ContractError> {
    validate_sender_is_admin(&deps, &info)?;
    let admin_address = deps.api.addr_validate(&admin)?;

    if ADMINS.has(deps.storage, admin_address.clone()) {
        return Err(new_generic_error("Address is already an admin"));
    }

    ADMINS.save(deps.storage, admin_address.clone(), &())?;

    Ok(Response::new()
        .add_attribute("action", "add_admin")
        .add_attribute("sender", info.sender)
        .add_attribute("added_admin", admin_address))
}

fn execute_remove_admin(
    deps: DepsMut,
    info: MessageInfo,
    admin: String,
) -> Result<Response, ContractError> {
    validate_sender_is_admin(&deps, &info)?;
    let admin_address = deps.api.addr_validate(&admin)?;

    if !ADMINS.has(deps.storage, admin_address.clone()) {
        return Err(new_generic_error("Address is not an admin"));
    }

    // if there is only one admin left, we cannot remove it
    let admins_count = ADMINS
        .keys(deps.storage, None, None, Order::Ascending)
        .count();
    if admins_count == 1 {
        return Err(new_generic_error("Cannot remove the last admin"));
    }

    ADMINS.remove(deps.storage, admin_address.clone());

    Ok(Response::new()
        .add_attribute("action", "remove_admin")
        .add_attribute("sender", info.sender)
        .add_attribute("removed_admin", admin_address))
}

fn validate_sender_is_admin(deps: &DepsMut, info: &MessageInfo) -> Result<(), ContractError> {
    let is_admin = ADMINS.may_load(deps.storage, info.sender.clone())?;
    if is_admin.is_none() {
        return Err(ContractError::Unauthorized);
    }

    Ok(())
}

fn validate_sender_is_hydro_contract(
    deps: &DepsMut,
    info: &MessageInfo,
) -> Result<(), ContractError> {
    let hydro_contract = CONFIG.load(deps.storage)?.hydro_contract;
    if info.sender != hydro_contract {
        return Err(ContractError::Unauthorized);
    }

    Ok(())
}

fn get_current_stage(deps: &Deps, env: &Env) -> Result<StageData, ContractError> {
    let current_stage: Vec<StageData> = STAGES
        .range(
            deps.storage,
            None,
            Some(Bound::inclusive(env.block.time.nanos())),
            Order::Descending,
        )
        .take(1)
        .filter_map(|stage| match stage {
            Ok(stage) => Some(stage.1),
            Err(_) => None,
        })
        .collect();

    match current_stage.len() {
        1 => Ok(current_stage[0].clone()),
        _ => Err(new_generic_error(format!(
            "Failed to load current stage for timestamp: {}",
            env.block.time.nanos()
        ))),
    }
}

pub fn get_epoch_start_for_stage_id(deps: &Deps, stage_id: u64) -> Result<u64, ContractError> {
    let epoch_start_stage_id: Vec<u64> = EPOCHS
        .range(
            deps.storage,
            None,
            // Use inclusive upper bound to get the correct stage ID in case of epoch's starting stage
            Some(Bound::inclusive(stage_id)),
            Order::Descending,
        )
        .take(1)
        .filter_map(|stage| match stage {
            Ok(stage) => Some(stage.0),
            Err(_) => None,
        })
        .collect();

    match epoch_start_stage_id.len() {
        1 => Ok(epoch_start_stage_id[0]),
        _ => Err(new_generic_error(format!(
            "Failed to load epoch start stage id for provided stage id: {}",
            stage_id
        ))),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&query_config(&deps)?),
        QueryMsg::CurrentStage {} => to_json_binary(&query_current_stage(&deps, &env)?),
        QueryMsg::CurrentEpochUserLocked { user_address } => {
            to_json_binary(&query_current_epoch_user_locked(&deps, &env, user_address)?)
        }
        QueryMsg::Admins {} => to_json_binary(&query_admins(&deps)?),
    }
}

pub fn query_config(deps: &Deps) -> StdResult<ConfigResponse> {
    Ok(ConfigResponse {
        config: CONFIG.load(deps.storage)?,
    })
}

pub fn query_current_stage(deps: &Deps, env: &Env) -> StdResult<CurrentStageResponse> {
    Ok(CurrentStageResponse {
        stage: get_current_stage(deps, env)
            .map_err(|_| StdError::generic_err("Failed to get current stage."))?,
    })
}

pub fn query_current_epoch_user_locked(
    deps: &Deps,
    env: &Env,
    user_address: String,
) -> StdResult<CurrentEpochUserLockedResponse> {
    let user_address = deps.api.addr_validate(&user_address)?;

    let current_stage = get_current_stage(deps, env)
        .map_err(|_| StdError::generic_err("failed to load current stage"))?;

    let epoch_start_stage_id = get_epoch_start_for_stage_id(deps, current_stage.stage_id)
        .map_err(|_| StdError::generic_err("failed to load epoch start for current stage"))?;

    let currently_locked = USER_LOCK_AMOUNTS
        .may_load(deps.storage, (user_address, epoch_start_stage_id))?
        .unwrap_or_default();

    Ok(CurrentEpochUserLockedResponse { currently_locked })
}

pub fn query_admins(deps: &Deps) -> StdResult<AdminsResponse> {
    Ok(AdminsResponse {
        admins: ADMINS
            .keys(deps.storage, None, None, Order::Ascending)
            .filter_map(|admin| admin.ok())
            .collect(),
    })
}
