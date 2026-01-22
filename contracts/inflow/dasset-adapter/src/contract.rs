use cosmwasm_std::{
    entry_point, to_json_binary, Addr, BankMsg, Binary, Coin, Deps, DepsMut, Env, MessageInfo,
    Reply, ReplyOn, Response, StdError, SubMsg, SubMsgResult,
};
use cw2::set_contract_version;

use crate::{
    drop,
    error::ContractError,
    msg::{ConfigResponse, DAssetAdapterMsg, ExecuteMsg, InstantiateMsg, QueryMsg},
    state::{Config, ADMINS, CONFIG, EXECUTORS},
};

const CONTRACT_NAME: &str = env!("CARGO_PKG_NAME");
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

const WITHDRAW_REPLY_ID: u64 = 1;

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

    if msg.executors.is_empty() {
        return Err(ContractError::AtLeastOneExecutor {});
    }

    let admins: Vec<Addr> = msg
        .admins
        .into_iter()
        .map(|a| deps.api.addr_validate(&a))
        .collect::<Result<_, _>>()?;

    let executors: Vec<Addr> = msg
        .executors
        .into_iter()
        .map(|a| deps.api.addr_validate(&a))
        .collect::<Result<_, _>>()?;

    ADMINS.save(deps.storage, &admins)?;
    EXECUTORS.save(deps.storage, &executors)?;

    let config = Config {
        drop_staking_core: deps.api.addr_validate(&msg.drop_staking_core)?,
        drop_voucher: deps.api.addr_validate(&msg.drop_voucher)?,
        drop_withdrawal_manager: deps.api.addr_validate(&msg.drop_withdrawal_manager)?,
        vault_contract: deps.api.addr_validate(&msg.vault_contract)?,
        liquid_asset_denom: msg.liquid_asset_denom,
        base_asset_denom: msg.base_asset_denom,
    };

    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new().add_attribute("action", "instantiate"))
}

/// NOTE: This adapter does not implement the standard AdapterInterfaceMsg
/// because it handles Drop Protocol's asynchronous unbonding flow:
///
/// 1. Receives dAsset tokens (from any source)
/// 2. Executor calls Unbond() to initiate unbonding
/// 3. Drop Protocol returns NFT voucher with token_id
/// 4. After the unbonding period, executor calls Withdraw(token_id)
/// 5. Redeemed base assets are forwarded to the vault
///
/// This flow is incompatible with the synchronous deposit/withdraw pattern
/// of standard adapters, and does not require depositor tracking.
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::StandardAction(_) => Err(ContractError::Std(StdError::generic_err(
            "StandardAction not supported - use CustomAction for dAsset redemption flow",
        ))),

        ExecuteMsg::CustomAction(custom_msg) => {
            dispatch_custom_execute(deps, env, info, custom_msg)
        }
    }
}

fn dispatch_custom_execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: DAssetAdapterMsg,
) -> Result<Response, ContractError> {
    match msg {
        DAssetAdapterMsg::Unbond {} => {
            validate_executor(&deps, &info)?;
            execute_unbond(deps, env)
        }

        DAssetAdapterMsg::Withdraw { token_id } => {
            validate_executor(&deps, &info)?;
            execute_withdraw(deps, token_id)
        }

        DAssetAdapterMsg::UpdateConfig { .. } | DAssetAdapterMsg::UpdateExecutors { .. } => {
            validate_admin(&deps, &info)?;
            dispatch_admin_execute(deps, msg)
        }
    }
}

fn execute_unbond(deps: DepsMut, env: Env) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    let balance = deps
        .querier
        .query_balance(env.contract.address, config.liquid_asset_denom.clone())?;

    if balance.amount.is_zero() {
        return Err(ContractError::NoFundsToUnbond {});
    }

    let msg = drop::unbond_msg(config.drop_staking_core, vec![balance])?;

    Ok(Response::new()
        .add_message(msg)
        .add_attribute("action", "unbond"))
}

fn execute_withdraw(deps: DepsMut, token_id: String) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    let submsg = SubMsg {
        id: WITHDRAW_REPLY_ID,
        msg: drop::withdraw_voucher_msg(
            config.drop_voucher,
            config.drop_withdrawal_manager,
            token_id,
        )?
        .into(),
        gas_limit: None,
        reply_on: ReplyOn::Success,
        payload: Binary::default(),
    };

    Ok(Response::new()
        .add_submessage(submsg)
        .add_attribute("action", "withdraw"))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, env: Env, reply: Reply) -> Result<Response, ContractError> {
    if reply.id != WITHDRAW_REPLY_ID {
        return Err(ContractError::Std(StdError::generic_err(
            "Unknown reply id",
        )));
    }

    // Verify the withdrawal was successful
    match reply.result {
        SubMsgResult::Ok(_) => on_withdraw_reply(deps, env),
        SubMsgResult::Err(err) => Err(ContractError::WithdrawalFailed { reason: err }),
    }
}

fn on_withdraw_reply(deps: DepsMut, env: Env) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    let base_balance = deps
        .querier
        .query_balance(env.contract.address, config.base_asset_denom.clone())?;

    if base_balance.amount.is_zero() {
        return Err(ContractError::NoFundsReceived {});
    }

    Ok(Response::new()
        .add_message(BankMsg::Send {
            to_address: config.vault_contract.to_string(),
            amount: vec![Coin {
                denom: config.base_asset_denom,
                amount: base_balance.amount,
            }],
        })
        .add_attribute("action", "withdraw_reply")
        .add_attribute("forwarded", base_balance.amount.to_string()))
}

fn dispatch_admin_execute(deps: DepsMut, msg: DAssetAdapterMsg) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;

    match msg {
        DAssetAdapterMsg::UpdateConfig {
            drop_staking_core,
            drop_voucher,
            drop_withdrawal_manager,
            vault_contract,
        } => {
            if let Some(v) = drop_staking_core {
                config.drop_staking_core = deps.api.addr_validate(&v)?;
            }
            if let Some(v) = drop_voucher {
                config.drop_voucher = deps.api.addr_validate(&v)?;
            }
            if let Some(v) = drop_withdrawal_manager {
                config.drop_withdrawal_manager = deps.api.addr_validate(&v)?;
            }
            if let Some(v) = vault_contract {
                config.vault_contract = deps.api.addr_validate(&v)?;
            }

            CONFIG.save(deps.storage, &config)?;
            Ok(Response::new().add_attribute("action", "update_config"))
        }

        DAssetAdapterMsg::UpdateExecutors { executors } => {
            let execs: Vec<Addr> = executors
                .into_iter()
                .map(|a| deps.api.addr_validate(&a))
                .collect::<Result<_, _>>()?;

            EXECUTORS.save(deps.storage, &execs)?;
            Ok(Response::new().add_attribute("action", "update_executors"))
        }

        _ => Err(ContractError::UnauthorizedAdmin {}),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> Result<Binary, ContractError> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&query_config(deps)?).map_err(ContractError::Std),
        QueryMsg::StandardQuery(_) => Err(ContractError::Std(StdError::generic_err(
            "Standard queries not supported - use Config query",
        ))),
    }
}

fn query_config(deps: Deps) -> Result<ConfigResponse, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let admins = ADMINS.load(deps.storage)?;
    let executors = EXECUTORS.load(deps.storage)?;

    Ok(ConfigResponse {
        admins: admins.iter().map(|a| a.to_string()).collect(),
        executors: executors.iter().map(|a| a.to_string()).collect(),
        drop_staking_core: config.drop_staking_core.to_string(),
        drop_voucher: config.drop_voucher.to_string(),
        drop_withdrawal_manager: config.drop_withdrawal_manager.to_string(),
        vault_contract: config.vault_contract.to_string(),
        liquid_asset_denom: config.liquid_asset_denom,
        base_asset_denom: config.base_asset_denom,
    })
}

fn validate_executor(deps: &DepsMut, info: &MessageInfo) -> Result<(), ContractError> {
    let executors = EXECUTORS.load(deps.storage)?;
    if !executors.contains(&info.sender) {
        return Err(ContractError::UnauthorizedExecutor {});
    }
    Ok(())
}

fn validate_admin(deps: &DepsMut, info: &MessageInfo) -> Result<(), ContractError> {
    let admins = ADMINS.load(deps.storage)?;
    if !admins.contains(&info.sender) {
        return Err(ContractError::UnauthorizedAdmin {});
    }
    Ok(())
}
