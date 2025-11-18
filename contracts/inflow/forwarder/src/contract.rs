use cosmwasm_std::{
    entry_point, to_json_binary, CosmosMsg, Deps, DepsMut, Env, IbcMsg, IbcTimeout, MessageInfo,
    QueryResponse, Response, StdError, StdResult,
};

use cw2::set_contract_version;

use interface::inflow::ExecuteMsg as InflowExecuteMsg;

use serde::Serialize;

use crate::{
    error::ContractError,
    msg::{ConfigResponse, ExecuteMsg, InstantiateMsg, QueryMsg},
    state::{Config, CONFIG},
};

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
    if msg.ibc_timeout_seconds == 0 {
        return Err(ContractError::InvalidIbcTimeout {});
    }

    let config = Config {
        target_address: msg.target_address,
        denom: msg.denom,
        inflow_contract: msg.inflow_contract,
        channel_id: msg.channel_id,
        ibc_timeout_seconds: msg.ibc_timeout_seconds,
    };

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new()
        .add_attribute("action", "instantiate")
        .add_attribute("sender", info.sender)
        .add_attribute("denom", config.denom.clone())
        .add_attribute("channel_id", config.channel_id.clone()))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::ForwardToInflow {} => forward_to_inflow(deps, env, info),
    }
}

fn forward_to_inflow(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let amount = deps
        .querier
        .query_balance(env.contract.address, &config.denom)?;

    if amount.amount.is_zero() {
        return Err(ContractError::NothingToForward {
            denom: config.denom,
        });
    }

    let timeout_timestamp = env.block.time.plus_seconds(config.ibc_timeout_seconds);
    let deposit_msg = InflowExecuteMsg::Deposit {
        on_behalf_of: Some(config.target_address.clone()),
    };
    let memo = build_ibc_hook_memo(&config.inflow_contract, &deposit_msg)?;

    let ibc_transfer = CosmosMsg::Ibc(IbcMsg::Transfer {
        channel_id: config.channel_id.clone(),
        to_address: config.inflow_contract.clone(),
        amount: amount.clone(),
        timeout: IbcTimeout::with_timestamp(timeout_timestamp),
        memo: Some(memo),
    });

    Ok(Response::new()
        .add_message(ibc_transfer)
        .add_attribute("action", "forward_to_inflow")
        .add_attribute("caller", info.sender)
        .add_attribute("amount", amount.amount)
        .add_attribute("denom", amount.denom))
}

fn build_ibc_hook_memo(
    inflow_contract: &str,
    deposit_msg: &InflowExecuteMsg,
) -> Result<String, ContractError> {
    let memo = IbcHookMessage {
        wasm: WasmHook {
            contract: inflow_contract,
            msg: to_json_binary(deposit_msg)?,
        },
    };

    serde_json_wasm::to_string(&memo)
        .map_err(|err| StdError::serialize_err("IbcHookMessage", err).into())
}

#[derive(Serialize)]
struct IbcHookMessage<'a> {
    wasm: WasmHook<'a>,
}

#[derive(Serialize)]
struct WasmHook<'a> {
    contract: &'a str,
    msg: cosmwasm_std::Binary,
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<QueryResponse> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&query_config(deps)?),
    }
}

fn query_config(deps: Deps) -> StdResult<ConfigResponse> {
    let config = CONFIG.load(deps.storage)?;
    Ok(ConfigResponse {
        target_address: config.target_address,
        denom: config.denom,
        inflow_contract: config.inflow_contract,
        channel_id: config.channel_id,
        ibc_timeout_seconds: config.ibc_timeout_seconds,
    })
}
