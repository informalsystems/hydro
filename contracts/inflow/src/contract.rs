use cosmos_sdk_proto::cosmos::bank::v1beta1::{DenomUnit, Metadata};
use cosmwasm_std::{
    entry_point, from_json, to_json_binary, to_json_vec, AnyMsg, Binary, Coin, CosmosMsg, Deps,
    DepsMut, Env, MessageInfo, Reply, Response, StdError, StdResult, SubMsg, Uint128,
};
use cw2::set_contract_version;
use neutron_sdk::{
    bindings::{msg::NeutronMsg, query::NeutronQuery},
    proto_types::osmosis::tokenfactory::v1beta1::MsgSetDenomMetadata,
    query::token_factory::query_full_denom,
};

use prost::Message;

use crate::{
    error::ContractError,
    msg::{DenomMetadata, ExecuteMsg, InstantiateMsg, ReplyPayload},
    query::{ConfigResponse, QueryMsg, TotalPoolValueResponse},
    state::{
        load_config, Config, CONFIG, DEPLOYED_AMOUNT, TOKENS_DEPOSITED, VAULT_SHARES_DENOM,
        WHITELIST,
    },
};

/// Contract name that is used for migration.
pub const CONTRACT_NAME: &str = env!("CARGO_PKG_NAME");
/// Contract version that is used for migration.
pub const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

pub const UNUSED_MSG_ID: u64 = 0;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut<NeutronQuery>,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response<NeutronMsg>, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    CONFIG.save(
        deps.storage,
        &Config {
            deposit_denom: msg.deposit_denom.clone(),
        },
    )?;

    TOKENS_DEPOSITED.save(deps.storage, &Uint128::zero())?;

    let whitelist_addresses = msg
        .whitelist
        .iter()
        .filter_map(|addr| deps.api.addr_validate(addr).ok())
        .collect::<Vec<_>>();

    if whitelist_addresses.is_empty() {
        return Err(ContractError::Std(StdError::generic_err(
            "at least one whitelist address must be provided",
        )));
    }

    for whitelist_address in &whitelist_addresses {
        WHITELIST.save(deps.storage, whitelist_address.clone(), &())?;
    }

    // Send SubMsg to the TokenFactory module to create a new denom
    let create_denom_msg = SubMsg::reply_on_success(
        NeutronMsg::submit_create_denom(msg.subdenom.clone()),
        UNUSED_MSG_ID,
    )
    .with_payload(to_json_vec(&ReplyPayload::CreateDenom {
        subdenom: msg.subdenom.clone(),
        metadata: msg.token_metadata,
    })?);

    Ok(Response::new()
        .add_submessage(create_denom_msg)
        .add_attribute("action", "initialisation")
        .add_attribute("sender", info.sender)
        .add_attribute("deposit_token", msg.deposit_denom))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response<NeutronMsg>, ContractError> {
    match msg {
        ExecuteMsg::Deposit {} => deposit(deps, info),
        // TODO: add/remove addresses from whitelist
        ExecuteMsg::SubmitDeployedAmount { amount } => {
            submit_deployed_amount(deps, env, info, amount)
        }
    }
}

// TODO: Desc
fn deposit(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
) -> Result<Response<NeutronMsg>, ContractError> {
    let config = load_config(deps.storage)?;
    let deposit_amount = cw_utils::must_pay(&info, &config.deposit_denom)?;

    TOKENS_DEPOSITED.update(deps.storage, |current_value| {
        current_value
            .checked_add(deposit_amount)
            .map_err(|e| StdError::generic_err(format!("overflow error: {e}")))
    })?;

    // TODO: mint vault shares, save info about the tokens deposited by the user, etc.
    // let vault_token = VAULT_SHARES_DENOM.load(deps.storage)?;
    // let mint_vault_shares_msg = NeutronMsg::submit_mint_tokens(&vault_token, shares_amount, &info.sender);

    Ok(Response::new()
        // .add_message(mint_vault_shares_msg)
        .add_attribute("action", "deposit")
        .add_attribute("sender", info.sender)
        .add_attribute("deposit_amount", deposit_amount))
}

fn submit_deployed_amount(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    amount: Uint128,
) -> Result<Response<NeutronMsg>, ContractError> {
    // Check if the sender is in the whitelist
    let is_whitelisted = WHITELIST.may_load(deps.storage, info.sender.clone())?;

    if is_whitelisted.is_none() {
        return Err(ContractError::Unauthorized);
    }

    // Save the deployed amount snapshot at current height
    DEPLOYED_AMOUNT.save(deps.storage, &amount, env.block.height)?;

    Ok(Response::new()
        .add_attribute("action", "submit_deployed_amount")
        .add_attribute("sender", info.sender)
        .add_attribute("amount", amount))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&query_config(deps)?),
        QueryMsg::TotalPoolValue {} => to_json_binary(&query_total_pool_value(deps, env)?),
    }
}

fn query_config(deps: Deps) -> StdResult<ConfigResponse> {
    Ok(ConfigResponse {
        config: CONFIG.load(deps.storage)?,
    })
}

fn query_total_pool_value(deps: Deps, env: Env) -> StdResult<TotalPoolValueResponse> {
    let config = CONFIG.load(deps.storage)?;
    let denom = config.deposit_denom;

    // Get the current balance of this contract in the deposit denom
    let balance: Coin = deps
        .querier
        .query_balance(env.contract.address, denom.clone())?;

    // Get the total deployed amount (from snapshot storage)
    let deployed_amount = DEPLOYED_AMOUNT.may_load(deps.storage)?;

    let total = balance
        .amount
        .checked_add(deployed_amount.unwrap_or_default())?;
    Ok(TotalPoolValueResponse { total })
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    msg: Reply,
) -> Result<Response<NeutronMsg>, ContractError> {
    let reply_paylod = from_json::<ReplyPayload>(&msg.payload)?;
    match reply_paylod {
        ReplyPayload::CreateDenom { subdenom, metadata } => {
            // Full denom name, e.g. "factory/{inflow_contract_address}/hydro_inflow_atom"
            let full_denom = query_full_denom(deps.as_ref(), &env.contract.address, subdenom)?;

            VAULT_SHARES_DENOM.save(deps.storage, &full_denom.denom)?;

            let msg = create_set_denom_metadata_msg(
                env.contract.address.into_string(),
                full_denom.denom.clone(),
                metadata,
            );

            Ok(Response::new()
                .add_message(msg)
                .add_attribute("action", "reply_create_denom")
                .add_attribute("full_denom", full_denom.denom))
        }
    }
}

fn create_set_denom_metadata_msg(
    contract_address: String,
    full_denom: String,
    token_metadata: DenomMetadata,
) -> CosmosMsg<NeutronMsg> {
    CosmosMsg::Any(AnyMsg {
        type_url: "/osmosis.tokenfactory.v1beta1.MsgSetDenomMetadata".to_owned(),
        value: Binary::from(
            MsgSetDenomMetadata {
                sender: contract_address,
                metadata: Some(Metadata {
                    denom_units: vec![
                        DenomUnit {
                            denom: full_denom.clone(),
                            exponent: 0,
                            aliases: vec![],
                        },
                        DenomUnit {
                            denom: token_metadata.display.clone(),
                            exponent: token_metadata.exponent,
                            aliases: vec![],
                        },
                    ],
                    base: full_denom,
                    display: token_metadata.display,
                    name: token_metadata.name,
                    description: token_metadata.description,
                    symbol: token_metadata.symbol,
                    uri: token_metadata.uri.unwrap_or_default(),
                    uri_hash: token_metadata.uri_hash.unwrap_or_default(),
                }),
            }
            .encode_to_vec(),
        ),
    })
}
