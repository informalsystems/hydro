use cosmwasm_std::{
    entry_point, to_json_binary, Addr, BankMsg, Binary, Coin, Deps, DepsMut, Env, MessageInfo,
    Response, StdResult, Uint128,
};
use cw2::set_contract_version;
use cw_utils::one_coin;

use crate::error::ContractError;
use crate::msg::{
    AdapterInterfaceMsg, AdapterInterfaceQueryMsg, AdminsResponse, AllPositionsResponse,
    AvailableAmountResponse, ConfigResponse, DepositorPositionResponse, DepositorPositionsResponse,
    ExecuteMsg, InstantiateMsg, QueryMsg, RegisteredDepositorInfo, RegisteredDepositorsResponse,
    TimeEstimateResponse,
};
use crate::state::{Depositor, ADMINS, WHITELISTED_DEPOSITORS};
use crate::validation::{validate_admin_caller, validate_depositor_caller};

pub const CONTRACT_NAME: &str = env!("CARGO_PKG_NAME");
pub const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

// ========== INSTANTIATE ==========

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    if msg.admins.is_empty() {
        return Err(ContractError::AtLeastOneAdmin {});
    }

    let mut validated_admins: Vec<Addr> = Vec::new();
    for admin_str in &msg.admins {
        let addr = deps.api.addr_validate(admin_str)?;
        if validated_admins.contains(&addr) {
            return Err(ContractError::AdminAlreadyExists {
                admin: admin_str.clone(),
            });
        }
        validated_admins.push(addr);
    }
    ADMINS.save(deps.storage, &validated_admins)?;

    let mut depositors_count = 0u32;
    for address in msg.initial_depositors {
        let addr = deps.api.addr_validate(&address)?;
        if WHITELISTED_DEPOSITORS.has(deps.storage, addr.clone()) {
            return Err(ContractError::DepositorAlreadyRegistered {
                depositor_address: address,
            });
        }
        WHITELISTED_DEPOSITORS.save(deps.storage, addr, &Depositor { enabled: true })?;
        depositors_count += 1;
    }

    Ok(Response::new()
        .add_attribute("action", "instantiate")
        .add_attribute("contract_name", CONTRACT_NAME)
        .add_attribute("contract_version", CONTRACT_VERSION)
        .add_attribute("admins_count", validated_admins.len().to_string())
        .add_attribute("depositors_count", depositors_count.to_string()))
}

// ========== EXECUTE ==========

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    let ExecuteMsg::StandardAction(interface_msg) = msg;
    match interface_msg {
        AdapterInterfaceMsg::Deposit {} => execute_deposit(deps, info),
        AdapterInterfaceMsg::Withdraw { coin } => execute_withdraw(deps, _env, info, coin),
        AdapterInterfaceMsg::RegisterDepositor {
            depositor_address,
            metadata: _,
        } => execute_register_depositor(deps, info, depositor_address),
        AdapterInterfaceMsg::UnregisterDepositor { depositor_address } => {
            execute_unregister_depositor(deps, info, depositor_address)
        }
        AdapterInterfaceMsg::SetDepositorEnabled {
            depositor_address,
            enabled,
        } => execute_set_depositor_enabled(deps, info, depositor_address, enabled),
        AdapterInterfaceMsg::AddAdmin { admin_address } => {
            execute_add_admin(deps, info, admin_address)
        }
        AdapterInterfaceMsg::RemoveAdmin { admin_address } => {
            execute_remove_admin(deps, info, admin_address)
        }
    }
}

fn execute_deposit(deps: DepsMut, info: MessageInfo) -> Result<Response, ContractError> {
    validate_depositor_caller(&deps, &info)?;

    let coin = one_coin(&info).map_err(|_| ContractError::InvalidFunds {
        count: info.funds.len(),
    })?;

    Ok(Response::new()
        .add_attribute("action", "deposit")
        .add_attribute("depositor", info.sender)
        .add_attribute("amount", coin.amount)
        .add_attribute("denom", &coin.denom))
}

fn execute_withdraw(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    coin: Coin,
) -> Result<Response, ContractError> {
    validate_depositor_caller(&deps, &info)?;

    if coin.amount.is_zero() {
        return Err(ContractError::ZeroAmount {});
    }

    let balance = deps
        .querier
        .query_balance(env.contract.address, coin.denom.clone())?;
    if balance.amount < coin.amount {
        return Err(ContractError::InsufficientBalance {});
    }

    let bank_msg = BankMsg::Send {
        to_address: info.sender.to_string(),
        amount: vec![coin.clone()],
    };

    Ok(Response::new()
        .add_message(bank_msg)
        .add_attribute("action", "withdraw")
        .add_attribute("depositor", info.sender)
        .add_attribute("amount", coin.amount)
        .add_attribute("denom", coin.denom))
}

fn execute_register_depositor(
    deps: DepsMut,
    info: MessageInfo,
    depositor_address: String,
) -> Result<Response, ContractError> {
    validate_admin_caller(&deps.as_ref(), &info)?;

    let addr = deps.api.addr_validate(&depositor_address)?;
    if WHITELISTED_DEPOSITORS.has(deps.storage, addr.clone()) {
        return Err(ContractError::DepositorAlreadyRegistered {
            depositor_address: depositor_address.clone(),
        });
    }

    WHITELISTED_DEPOSITORS.save(deps.storage, addr, &Depositor { enabled: true })?;

    Ok(Response::new()
        .add_attribute("action", "register_depositor")
        .add_attribute("depositor_address", depositor_address))
}

fn execute_unregister_depositor(
    deps: DepsMut,
    info: MessageInfo,
    depositor_address: String,
) -> Result<Response, ContractError> {
    validate_admin_caller(&deps.as_ref(), &info)?;

    let addr = deps.api.addr_validate(&depositor_address)?;
    WHITELISTED_DEPOSITORS.remove(deps.storage, addr);

    Ok(Response::new()
        .add_attribute("action", "unregister_depositor")
        .add_attribute("depositor_address", depositor_address))
}

fn execute_set_depositor_enabled(
    deps: DepsMut,
    info: MessageInfo,
    depositor_address: String,
    enabled: bool,
) -> Result<Response, ContractError> {
    validate_admin_caller(&deps.as_ref(), &info)?;

    let addr = deps.api.addr_validate(&depositor_address)?;
    let mut depositor = WHITELISTED_DEPOSITORS
        .may_load(deps.storage, addr.clone())?
        .ok_or(ContractError::DepositorNotRegistered {
            depositor_address: depositor_address.clone(),
        })?;

    depositor.enabled = enabled;
    WHITELISTED_DEPOSITORS.save(deps.storage, addr, &depositor)?;

    Ok(Response::new()
        .add_attribute("action", "set_depositor_enabled")
        .add_attribute("depositor_address", depositor_address)
        .add_attribute("enabled", enabled.to_string()))
}

fn execute_add_admin(
    deps: DepsMut,
    info: MessageInfo,
    admin_address: String,
) -> Result<Response, ContractError> {
    validate_admin_caller(&deps.as_ref(), &info)?;

    let addr = deps.api.addr_validate(&admin_address)?;
    let mut admins = ADMINS.load(deps.storage)?;
    if admins.contains(&addr) {
        return Err(ContractError::AdminAlreadyExists {
            admin: admin_address,
        });
    }
    admins.push(addr.clone());
    ADMINS.save(deps.storage, &admins)?;

    Ok(Response::new()
        .add_attribute("action", "add_admin")
        .add_attribute("admin", addr)
        .add_attribute("added_by", info.sender))
}

fn execute_remove_admin(
    deps: DepsMut,
    info: MessageInfo,
    admin_address: String,
) -> Result<Response, ContractError> {
    validate_admin_caller(&deps.as_ref(), &info)?;

    let addr = deps.api.addr_validate(&admin_address)?;
    let mut admins = ADMINS.load(deps.storage)?;
    if !admins.contains(&addr) {
        return Err(ContractError::AdminNotFound {
            admin: admin_address,
        });
    }
    admins.retain(|a| a != addr);
    if admins.is_empty() {
        return Err(ContractError::CannotRemoveLastAdmin {});
    }
    ADMINS.save(deps.storage, &admins)?;

    Ok(Response::new()
        .add_attribute("action", "remove_admin")
        .add_attribute("admin", addr)
        .add_attribute("removed_by", info.sender))
}

// ========== QUERY ==========

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    let QueryMsg::StandardQuery(interface_msg) = msg;
    match interface_msg {
        AdapterInterfaceQueryMsg::Config {} => to_json_binary(&query_config(deps)?),
        AdapterInterfaceQueryMsg::AvailableForDeposit { .. } => {
            to_json_binary(&AvailableAmountResponse {
                amount: Uint128::MAX,
            })
        }
        AdapterInterfaceQueryMsg::AvailableForWithdraw { denom, .. } => {
            let balance = deps.querier.query_balance(env.contract.address, denom)?;
            to_json_binary(&AvailableAmountResponse {
                amount: balance.amount,
            })
        }
        AdapterInterfaceQueryMsg::TimeToWithdraw { .. } => to_json_binary(&TimeEstimateResponse {
            blocks: 0,
            seconds: 0,
        }),
        AdapterInterfaceQueryMsg::AllPositions {} => {
            to_json_binary(&AllPositionsResponse { positions: vec![] })
        }
        AdapterInterfaceQueryMsg::DepositorPosition { .. } => {
            to_json_binary(&DepositorPositionResponse {
                amount: Uint128::zero(),
            })
        }
        AdapterInterfaceQueryMsg::DepositorPositions { .. } => {
            to_json_binary(&DepositorPositionsResponse { positions: vec![] })
        }
        AdapterInterfaceQueryMsg::RegisteredDepositors { enabled } => {
            to_json_binary(&query_registered_depositors(deps, enabled)?)
        }
        AdapterInterfaceQueryMsg::Admins {} => to_json_binary(&query_admins(deps)?),
    }
}

fn query_config(_deps: Deps) -> StdResult<ConfigResponse> {
    Ok(ConfigResponse {})
}

fn query_registered_depositors(
    deps: Deps,
    enabled: Option<bool>,
) -> StdResult<RegisteredDepositorsResponse> {
    let depositors: Vec<RegisteredDepositorInfo> = WHITELISTED_DEPOSITORS
        .range(deps.storage, None, None, cosmwasm_std::Order::Ascending)
        .filter_map(|item| {
            item.ok().and_then(|(addr, depositor)| {
                if let Some(filter_enabled) = enabled {
                    if depositor.enabled != filter_enabled {
                        return None;
                    }
                }
                Some(RegisteredDepositorInfo {
                    depositor_address: addr.to_string(),
                    enabled: depositor.enabled,
                })
            })
        })
        .collect();

    Ok(RegisteredDepositorsResponse { depositors })
}

fn query_admins(deps: Deps) -> StdResult<AdminsResponse> {
    let admins = ADMINS.load(deps.storage)?;
    Ok(AdminsResponse {
        admins: admins.into_iter().map(|a| a.to_string()).collect(),
    })
}
