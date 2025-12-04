use cosmwasm_std::{
    entry_point, from_json, to_json_binary, to_json_vec, Addr, BankMsg, Binary, Coin, Deps,
    DepsMut, Env, MessageInfo, Reply, Response, StdError, StdResult, SubMsg, SubMsgResult, WasmMsg,
};
use cw2::set_contract_version;

use interface::{
    inflow::{
        Config as InflowConfig, ConfigResponse as InflowConfigResponse,
        ExecuteMsg as InflowExecuteMsg, QueryMsg as InflowQueryMsg,
    },
    inflow_control_center::{QueryMsg as ControlCenterQueryMsg, SubvaultsResponse},
    utils::UNUSED_MSG_ID,
};

use crate::{
    error::{new_generic_error, ContractError},
    msg::{ConfigResponse, ExecuteMsg, InstantiateMsg, QueryMsg, ReplyPayload},
    state::{load_config, Config, CONFIG},
};

const CONTRACT_NAME: &str = env!("CARGO_PKG_NAME");
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let admins = msg
        .admins
        .iter()
        .map(|addr| deps.api.addr_validate(addr))
        .collect::<StdResult<Vec<Addr>>>()?;

    if admins.is_empty() {
        return Err(new_generic_error("no admins provided"));
    }

    let control_centers = msg
        .control_centers
        .iter()
        .map(|addr| deps.api.addr_validate(addr))
        .collect::<StdResult<Vec<Addr>>>()?;

    if control_centers.is_empty() {
        return Err(new_generic_error("no control centers provided"));
    }

    let config = Config {
        admins,
        control_centers,
    };
    CONFIG.save(deps.storage, &config)?;

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    Ok(Response::new().add_attribute("action", "initialization"))
}

#[entry_point]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    let config = load_config(deps.storage)?;

    match msg {
        ExecuteMsg::ForwardToInflow {} => forward_to_inflow(deps, env, &config),
        ExecuteMsg::WithdrawReceiptTokens { address, coin } => {
            withdraw_receipt_tokens(deps, env, info, &config, address, coin)
        }
        ExecuteMsg::WithdrawFunds { address, coin } => {
            withdraw_funds(deps, env, info, &config, address, coin)
        }
    }
}

fn forward_to_inflow(deps: DepsMut, env: Env, config: &Config) -> Result<Response, ContractError> {
    // Collect information about all Inflow vaults from all Control Centers
    let vault_infos = get_all_inflow_vaults_configs(&deps.as_ref(), config)?;

    // Get contract's deposit tokens balances and prepare submessages to deposit tokens into Inflow vaults
    let submsgs = vault_infos
        .iter()
        .filter_map(|vault_info| {
            let deposit_token_balance = match deps.querier.query_balance(
                env.contract.address.to_string(),
                &vault_info.config.deposit_denom,
            ) {
                Ok(balance) => {
                    if balance.amount.is_zero() {
                        return None;
                    }

                    balance.amount
                }
                Err(_) => return None,
            };

            let execute_deposit_msg =
                match to_json_binary(&InflowExecuteMsg::Deposit { on_behalf_of: None }) {
                    Ok(msg) => msg,
                    Err(_) => return None,
                };

            // Create payload in order to be able to identify the deposit in the reply handler
            let reply_payload = match to_json_vec(&ReplyPayload::DepositToInflow {
                vault_address: vault_info.address.to_string(),
                deposit: Coin {
                    denom: vault_info.config.deposit_denom.clone(),
                    amount: deposit_token_balance,
                },
            }) {
                Ok(payload) => payload,
                Err(_) => return None,
            };

            // Prepare submessage to execute the deposit. Use reply_always() to ensure we get
            // a reply even if the deposit fails. This allows only some deposits to succeed.
            Some(
                SubMsg::reply_always(
                    WasmMsg::Execute {
                        contract_addr: vault_info.address.to_string(),
                        msg: execute_deposit_msg,
                        funds: vec![Coin {
                            denom: vault_info.config.deposit_denom.clone(),
                            amount: deposit_token_balance,
                        }],
                    },
                    UNUSED_MSG_ID,
                )
                .with_payload(reply_payload),
            )
        })
        .collect::<Vec<SubMsg>>();

    Ok(Response::new()
        .add_submessages(submsgs)
        .add_attribute("action", "forward_to_inflow"))
}

fn withdraw_receipt_tokens(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    config: &Config,
    recipient: String,
    requested_for_withdrawal: Coin,
) -> Result<Response, ContractError> {
    ensure_admin(config, &info.sender)?;

    let recipient = deps.api.addr_validate(&recipient)?;

    let vault_shares_token_balance = deps
        .querier
        .query_balance(
            env.contract.address.to_string(),
            requested_for_withdrawal.denom.clone(),
        )?
        .amount;

    let amount_to_withdraw = vault_shares_token_balance.min(requested_for_withdrawal.amount);

    if amount_to_withdraw.is_zero() {
        return Err(ContractError::Std(StdError::generic_err(format!(
            "failed to withdraw receipt tokens; zero balance of {}",
            requested_for_withdrawal.denom
        ))));
    }

    let bank_msg = BankMsg::Send {
        to_address: recipient.to_string(),
        amount: vec![Coin {
            denom: requested_for_withdrawal.denom.clone(),
            amount: amount_to_withdraw,
        }],
    };

    Ok(Response::new()
        .add_message(bank_msg)
        .add_attribute("action", "withdraw_receipt_tokens")
        .add_attribute("recipient", recipient)
        .add_attribute("denom", requested_for_withdrawal.denom)
        .add_attribute("amount_requested", requested_for_withdrawal.amount)
        .add_attribute("amount_withdrawn", amount_to_withdraw))
}

fn withdraw_funds(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    config: &Config,
    recipient: String,
    requested_for_withdrawal: Coin,
) -> Result<Response, ContractError> {
    ensure_admin(config, &info.sender)?;

    let recipient = deps.api.addr_validate(&recipient)?;

    let vault_shares_token_balance = deps
        .querier
        .query_balance(
            env.contract.address.to_string(),
            requested_for_withdrawal.denom.clone(),
        )?
        .amount;

    let amount_to_withdraw = vault_shares_token_balance.min(requested_for_withdrawal.amount);

    if amount_to_withdraw.is_zero() {
        return Err(ContractError::Std(StdError::generic_err(format!(
            "failed to withdraw funds; zero balance of {}",
            requested_for_withdrawal.denom
        ))));
    }

    let inflow_vault =
        get_inflow_vault_for_shares_denom(&deps.as_ref(), &requested_for_withdrawal.denom, config)?;

    let execute_withdraw_msg = to_json_binary(&InflowExecuteMsg::Withdraw {
        on_behalf_of: Some(recipient.to_string()),
    })?;

    let wasm_msg = WasmMsg::Execute {
        contract_addr: inflow_vault.to_string(),
        msg: execute_withdraw_msg,
        funds: vec![Coin {
            denom: requested_for_withdrawal.denom.clone(),
            amount: amount_to_withdraw,
        }],
    };

    Ok(Response::new()
        .add_message(wasm_msg)
        .add_attribute("action", "withdraw_funds")
        .add_attribute("recipient", recipient)
        .add_attribute("denom", requested_for_withdrawal.denom)
        .add_attribute("amount_requested", requested_for_withdrawal.amount)
        .add_attribute("amount_withdrawn", amount_to_withdraw))
}

fn ensure_admin(config: &Config, sender: &Addr) -> Result<(), ContractError> {
    if config.admins.iter().any(|addr| addr == sender) {
        Ok(())
    } else {
        Err(ContractError::Unauthorized {})
    }
}

#[entry_point]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&ConfigResponse {
            config: load_config(deps.storage)?,
        }),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(_deps: DepsMut, _env: Env, msg: Reply) -> Result<Response, ContractError> {
    match from_json::<ReplyPayload>(&msg.payload)? {
        ReplyPayload::DepositToInflow {
            vault_address,
            deposit,
        } => handle_deposit_to_inflow_reply(vault_address, deposit, msg),
    }
}

fn handle_deposit_to_inflow_reply(
    vault_address: String,
    deposit: Coin,
    msg: Reply,
) -> Result<Response, ContractError> {
    let mut response = Response::new()
        .add_attribute("action", "deposit_to_inflow_reply")
        .add_attribute("vault_address", vault_address)
        .add_attribute("deposit_denom", deposit.denom)
        .add_attribute("deposit_amount", deposit.amount);

    match msg.result {
        SubMsgResult::Ok(_) => {
            response = response.add_attribute("status", "success");
        }
        SubMsgResult::Err(err) => {
            response = response
                .add_attribute("status", "failed")
                .add_attribute("error", err);
        }
    }

    Ok(response)
}

pub fn get_all_inflow_vaults_configs(
    deps: &Deps,
    config: &Config,
) -> Result<Vec<InflowVaultConfig>, ContractError> {
    let mut vaults_configs = vec![];

    for control_center in &config.control_centers {
        let subvaults = deps
            .querier
            .query_wasm_smart::<SubvaultsResponse>(
                control_center,
                &ControlCenterQueryMsg::Subvaults {},
            )?
            .subvaults;

        for subvault in &subvaults {
            let vault_config = deps
                .querier
                .query_wasm_smart::<InflowConfigResponse>(subvault, &InflowQueryMsg::Config {})?
                .config;

            vaults_configs.push(InflowVaultConfig {
                address: subvault.clone(),
                config: vault_config,
            });
        }
    }

    Ok(vaults_configs)
}

pub fn get_inflow_vault_for_shares_denom(
    deps: &Deps,
    vault_shares_denom: &str,
    config: &Config,
) -> Result<Addr, ContractError> {
    let vaults_configs = get_all_inflow_vaults_configs(deps, config)?;

    for vault_config in vaults_configs {
        if vault_config.config.vault_shares_denom == vault_shares_denom {
            return Ok(vault_config.address);
        }
    }

    Err(ContractError::Std(StdError::generic_err(format!(
        "no Inflow vault found for shares denom {vault_shares_denom}"
    ))))
}

/// Holds the address and configuration of an Inflow vault contract.
pub struct InflowVaultConfig {
    pub address: Addr,
    pub config: InflowConfig,
}
