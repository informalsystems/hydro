use std::collections::HashSet;

use cosmwasm_std::{
    entry_point, to_json_binary, Addr, BankMsg, Binary, Coin, Deps, DepsMut, Env, MessageInfo,
    Order, Reply, Response, StdResult, Uint128,
};
use cw2::set_contract_version;
use interface::token_info_provider::ValidatorsInfoResponse;
use neutron_sdk::bindings::{msg::NeutronMsg, query::NeutronQuery};
use neutron_sdk::interchain_queries::v047::register_queries::new_register_staking_validators_query_msg;
use neutron_sdk::sudo::msg::SudoMsg;

use crate::error::{new_generic_error, ContractError};
use crate::msg::{ExecuteContext, ExecuteMsg, InstantiateMsg};
use crate::query::{
    AdminsResponse, ConfigResponse, ICQManagersResponse, QueryMsg,
    RegisteredValidatorQueriesResponse,
};
use crate::state::{
    Config, ADMINS, CONFIG, ICQ_MANAGERS, VALIDATORS_INFO, VALIDATORS_PER_ROUND,
    VALIDATORS_STORE_INITIALIZED, VALIDATOR_TO_QUERY_ID,
};
use crate::utils::{
    get_nearest_store_initialized_round, query_current_round_id, run_on_each_transaction,
    COSMOS_VALIDATOR_PREFIX,
};
use crate::validators_icqs::{
    build_create_interchain_query_submsg, handle_delivered_interchain_query_result,
    handle_submsg_reply, query_min_interchain_query_deposit,
};
use cw_utils::must_pay;

/// Contract name that is used for migration.
pub const CONTRACT_NAME: &str = env!("CARGO_PKG_NAME");
/// Contract version that is used for migration.
pub const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

pub const NATIVE_TOKEN_DENOM: &str = "untrn";

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut<NeutronQuery>,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    let hydro_contract_address = match msg.hydro_contract_address {
        Some(hydro_contract_address) => deps.api.addr_validate(&hydro_contract_address)?,
        None => info.sender.clone(),
    };

    let config = Config {
        hydro_contract_address: hydro_contract_address.clone(),
        max_validator_shares_participating: msg.max_validator_shares_participating,
        hub_connection_id: msg.hub_connection_id.clone(),
        hub_transfer_channel_id: msg.hub_transfer_channel_id.clone(),
        icq_update_period: msg.icq_update_period,
    };

    CONFIG.save(deps.storage, &config)?;

    // the store for the first round is already initialized, since there is no previous round to copy information over from.
    VALIDATORS_STORE_INITIALIZED.save(deps.storage, 0, &true)?;

    for admin in msg.admins {
        let admin_addr = deps.api.addr_validate(&admin)?;
        ADMINS.save(deps.storage, admin_addr, &true)?;
    }

    for manager in msg.icq_managers {
        let manager_addr = deps.api.addr_validate(&manager)?;
        ICQ_MANAGERS.save(deps.storage, manager_addr, &true)?;
    }

    Ok(Response::new()
        .add_attribute("action", "initialisation")
        .add_attribute("sender", info.sender)
        .add_attribute("hydro_contract_address", hydro_contract_address)
        .add_attribute(
            "max_validator_shares_participating",
            msg.max_validator_shares_participating.to_string(),
        )
        .add_attribute("hub_connection_id", msg.hub_connection_id)
        .add_attribute("hub_transfer_channel_id", msg.hub_transfer_channel_id)
        .add_attribute("icq_update_period", msg.icq_update_period.to_string()))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    mut deps: DepsMut<NeutronQuery>,
    _env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response<NeutronMsg>, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let current_round_id = query_current_round_id(&deps.as_ref(), &config.hydro_contract_address)?;

    run_on_each_transaction(&mut deps, current_round_id)?;

    let context = ExecuteContext {
        current_round_id,
        config,
    };

    match msg {
        ExecuteMsg::CreateICQsForValidators { validators } => {
            create_icqs_for_validators(deps, info, validators, context)
        }
        ExecuteMsg::AddICQManager { address } => add_icq_manager(deps, info, address),
        ExecuteMsg::RemoveICQManager { address } => remove_icq_manager(deps, info, address),
        ExecuteMsg::WithdrawICQFunds { amount } => withdraw_icq_funds(deps, info, amount),
    }
}

// CreateICQsForValidators:
//     Validate that the first round has started
//     Validate received validator addresses
//     Validate that the sender paid enough deposit for ICQs creation
//     Create ICQ for each of the valid addresses
fn create_icqs_for_validators(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    validators: Vec<String>,
    context: ExecuteContext,
) -> Result<Response<NeutronMsg>, ContractError> {
    let mut valid_addresses = HashSet::new();
    for validator in validators
        .iter()
        .map(|validator| validator.trim().to_owned())
    {
        let hrp = match bech32::decode(&validator) {
            Ok((hrp, _)) => hrp,
            Err(_) => {
                continue;
            }
        };

        if !valid_addresses.contains(&validator)
            && !VALIDATOR_TO_QUERY_ID.has(deps.storage, validator.clone())
            && hrp.as_str().to_owned().eq(COSMOS_VALIDATOR_PREFIX)
        {
            valid_addresses.insert(validator);
        }
    }

    // icq_manager can create ICQs without paying for them; in this case, the
    // funds are implicitly provided by the contract, and these can thus either be funds
    // sent to the contract beforehand, or they could be escrowed funds
    // that were returned to the contract when previous Interchain Queries were removed
    // amd the escrowed funds were removed
    let is_icq_manager = validate_address_is_icq_manager(&deps, info.sender.clone()).is_ok();
    if !is_icq_manager {
        validate_icq_deposit_funds_sent(deps, &info, valid_addresses.len() as u64)?;
    }

    let mut register_icqs_submsgs = vec![];
    for validator_address in valid_addresses.clone() {
        let msg = new_register_staking_validators_query_msg(
            context.config.hub_connection_id.clone(),
            vec![validator_address.clone()],
            context.config.icq_update_period,
        )
        .map_err(|err| {
            new_generic_error(format!(
                "Failed to create staking validators interchain query. Error: {err}"
            ))
        })?;

        register_icqs_submsgs.push(build_create_interchain_query_submsg(
            msg,
            validator_address,
        )?);
    }

    Ok(Response::new()
        .add_attribute("action", "create_icqs_for_validators")
        .add_attribute("sender", info.sender)
        .add_attribute(
            "validator_addresses",
            valid_addresses
                .into_iter()
                .collect::<Vec<String>>()
                .join(", "),
        )
        .add_submessages(register_icqs_submsgs))
}

fn add_icq_manager(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    address: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_sender_is_admin(&deps, &info)?;

    let user_addr = deps.api.addr_validate(&address)?;

    let is_icq_manager = validate_address_is_icq_manager(&deps, user_addr.clone());
    if is_icq_manager.is_ok() {
        return Err(new_generic_error("Address is already an ICQ manager"));
    }

    ICQ_MANAGERS.save(deps.storage, user_addr.clone(), &true)?;

    Ok(Response::new()
        .add_attribute("action", "add_icq_manager")
        .add_attribute("address", user_addr)
        .add_attribute("sender", info.sender))
}

fn remove_icq_manager(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    address: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_sender_is_admin(&deps, &info)?;

    let user_addr = deps.api.addr_validate(&address)?;

    let free_icq_creators = validate_address_is_icq_manager(&deps, user_addr.clone());
    if free_icq_creators.is_err() {
        return Err(new_generic_error("Address is not an ICQ manager"));
    }

    ICQ_MANAGERS.remove(deps.storage, user_addr.clone());

    Ok(Response::new()
        .add_attribute("action", "remove_icq_manager")
        .add_attribute("address", user_addr)
        .add_attribute("sender", info.sender))
}

// Tries to withdraw the given amount of the NATIVE_TOKEN_DENOM from the contract.
// These will in practice be funds that were returned to the contract when the
// Interchain Queries were removed because a validator fell out of the top validators.
fn withdraw_icq_funds(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    amount: Uint128,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_address_is_icq_manager(&deps, info.sender.clone())?;

    // send the amount of native tokens to the sender
    let send = Coin {
        denom: NATIVE_TOKEN_DENOM.to_string(),
        amount,
    };

    Ok(Response::new()
        .add_attribute("action", "withdraw_icq_escrows")
        .add_attribute("sender", info.sender.clone())
        .add_message(BankMsg::Send {
            to_address: info.sender.to_string(),
            amount: vec![send],
        }))
}

fn validate_sender_is_admin(
    deps: &DepsMut<NeutronQuery>,
    info: &MessageInfo,
) -> Result<(), ContractError> {
    let is_admin = ADMINS.may_load(deps.storage, info.sender.clone())?;
    if is_admin.is_none() {
        return Err(ContractError::Unauthorized);
    }

    Ok(())
}

fn validate_address_is_icq_manager(
    deps: &DepsMut<NeutronQuery>,
    address: Addr,
) -> Result<(), ContractError> {
    let is_manager = ICQ_MANAGERS.may_load(deps.storage, address)?;
    if is_manager.is_none() {
        return Err(ContractError::Unauthorized);
    }

    Ok(())
}

// Validates that enough funds were sent to create ICQs for the given validator addresses.
// This function will be used again once we lift the restriction that only ICQ managers can create ICQs.
fn validate_icq_deposit_funds_sent(
    deps: DepsMut<NeutronQuery>,
    info: &MessageInfo,
    num_created_icqs: u64,
) -> Result<(), ContractError> {
    let min_icq_deposit = query_min_interchain_query_deposit(&deps.as_ref())?;
    let sent_token = must_pay(info, &min_icq_deposit.denom)?;
    let min_icqs_deposit = min_icq_deposit.amount.u128() * (num_created_icqs as u128);

    if min_icqs_deposit > sent_token.u128() {
        return Err(new_generic_error(format!("Insufficient tokens sent to pay for {} interchain queries deposits. Sent: {}, Required: {}", num_created_icqs, Coin::new(sent_token, NATIVE_TOKEN_DENOM), Coin::new(min_icqs_deposit, NATIVE_TOKEN_DENOM))));
    }

    Ok(())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(
    deps: DepsMut<NeutronQuery>,
    _env: Env,
    msg: Reply,
) -> Result<Response<NeutronMsg>, ContractError> {
    handle_submsg_reply(deps, msg)
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn sudo(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    msg: SudoMsg,
) -> Result<Response<NeutronMsg>, ContractError> {
    match msg {
        SudoMsg::KVQueryResult { query_id } => {
            handle_delivered_interchain_query_result(deps, env, query_id)
        }
        _ => Err(new_generic_error("Unexpected sudo message received")),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps<NeutronQuery>, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&query_config(deps)?),
        QueryMsg::RegisteredValidatorQueries {} => {
            to_json_binary(&query_registered_validator_queries(deps)?)
        }
        QueryMsg::Admins {} => to_json_binary(&query_admins(deps)?),
        QueryMsg::ICQManagers {} => to_json_binary(&query_icq_managers(deps)?),
        QueryMsg::ValidatorsInfo { round_id } => {
            to_json_binary(&query_validators_info(deps, round_id)?)
        }
    }
}

fn query_config(deps: Deps<NeutronQuery>) -> StdResult<ConfigResponse> {
    Ok(ConfigResponse {
        config: CONFIG.load(deps.storage)?,
    })
}

// Returns all the validator queries that are registered
// by the contract right now, for each query returning the address of the validator it is
// associated with, plus its query id.
pub fn query_registered_validator_queries(
    deps: Deps<NeutronQuery>,
) -> StdResult<RegisteredValidatorQueriesResponse> {
    let query_ids = VALIDATOR_TO_QUERY_ID
        .range(deps.storage, None, None, Order::Ascending)
        .filter_map(|l| {
            if l.is_err() {
                deps.api
                    .debug(&format!("Error when querying validator query id: {l:?}"));
            }
            l.ok()
        })
        .collect();

    Ok(RegisteredValidatorQueriesResponse { query_ids })
}

pub fn query_admins(deps: Deps<NeutronQuery>) -> StdResult<AdminsResponse> {
    Ok(AdminsResponse {
        admins: ADMINS
            .range(deps.storage, None, None, Order::Ascending)
            .filter_map(|l| match l {
                Ok((k, _)) => Some(k),
                Err(_) => {
                    deps.api.debug("Error parsing store when iterating admins!");
                    None
                }
            })
            .collect(),
    })
}

pub fn query_icq_managers(deps: Deps<NeutronQuery>) -> StdResult<ICQManagersResponse> {
    Ok(ICQManagersResponse {
        managers: ICQ_MANAGERS
            .range(deps.storage, None, None, Order::Ascending)
            .filter_map(|l| match l {
                Ok((k, _)) => Some(k),
                Err(_) => {
                    deps.api
                        .debug("Error parsing store when iterating ICQ managers!");
                    None
                }
            })
            .collect(),
    })
}

pub fn query_validators_info(
    deps: Deps<NeutronQuery>,
    round_id: u64,
) -> StdResult<ValidatorsInfoResponse> {
    // Find the nearest store initialized round ID or use 0. Using round ID 0, even if not initialized,
    // will not cause any harm, since no results will be found in that case (round ID is used as prefix).
    let round_id = get_nearest_store_initialized_round(deps.storage, round_id).unwrap_or_default();

    Ok(ValidatorsInfoResponse {
        round_id,
        validators: VALIDATORS_INFO
            .prefix(round_id)
            .range(deps.storage, None, None, Order::Ascending)
            .filter_map(|l| l.ok())
            .collect(),
    })
}

pub fn query_validators_per_round(
    deps: Deps<NeutronQuery>,
    round_id: u64,
) -> StdResult<Vec<(u128, String)>> {
    Ok(VALIDATORS_PER_ROUND
        .sub_prefix(round_id)
        .range(deps.storage, None, None, Order::Descending)
        .map(|l| l.unwrap().0)
        .collect())
}
