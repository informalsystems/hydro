use cosmos_sdk_proto::cosmos::bank::v1beta1::{DenomUnit, Metadata};
use cosmwasm_std::{
    entry_point, from_json, to_json_binary, to_json_vec, Addr, AnyMsg, BankMsg, Binary, Coin,
    CosmosMsg, Decimal, Deps, DepsMut, Env, MessageInfo, Reply, Response, StdError, StdResult,
    SubMsg, Uint128,
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
    state::{load_config, Config, CONFIG, DEPLOYED_AMOUNT, VAULT_SHARES_DENOM, WHITELIST},
};

/// Contract name that is used for migration.
pub const CONTRACT_NAME: &str = env!("CARGO_PKG_NAME");
/// Contract version that is used for migration.
pub const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

pub const UNUSED_MSG_ID: u64 = 0;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut<NeutronQuery>,
    env: Env,
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

    DEPLOYED_AMOUNT.save(deps.storage, &Uint128::zero(), env.block.height)?;

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
        .add_attribute("deposit_token_denom", msg.deposit_denom)
        .add_attribute("subdenom", msg.subdenom)
        .add_attribute(
            "whitelist",
            whitelist_addresses
                .iter()
                .map(|addr| addr.to_string())
                .collect::<Vec<String>>()
                .join(", "),
        ))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response<NeutronMsg>, ContractError> {
    let config = load_config(deps.storage)?;

    match msg {
        ExecuteMsg::Deposit {} => deposit(deps, env, info, &config),
        ExecuteMsg::WithdrawForDeployment { amount } => {
            withdraw_for_deployment(deps, env, info, &config, amount)
        }
        ExecuteMsg::AddToWhitelist { address } => add_to_whitelist(deps, env, info, address),
        ExecuteMsg::RemoveFromWhitelist { address } => {
            remove_from_whitelist(deps, env, info, address)
        }
        ExecuteMsg::SubmitDeployedAmount { amount } => {
            submit_deployed_amount(deps, env, info, amount)
        }
    }
}

// Deposits tokens accepted by the vault and issues certain amount of vault shares tokens in return.
fn deposit(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    config: &Config,
) -> Result<Response<NeutronMsg>, ContractError> {
    let deposit_amount = cw_utils::must_pay(&info, &config.deposit_denom)?;

    let vault_shares_to_mint =
        calculate_number_of_shares_to_mint(&deps.as_ref(), &env, config, deposit_amount)?;

    let mint_vault_shares_msg = NeutronMsg::submit_mint_tokens(
        &VAULT_SHARES_DENOM.load(deps.storage)?,
        vault_shares_to_mint,
        &info.sender,
    );

    Ok(Response::new()
        .add_message(mint_vault_shares_msg)
        .add_attribute("action", "deposit")
        .add_attribute("sender", info.sender)
        .add_attribute("deposit_amount", deposit_amount)
        .add_attribute("vault_shares_minted", vault_shares_to_mint))
}

// Withdraws the specified amount for deployment.
fn withdraw_for_deployment(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    config: &Config,
    amount: Uint128,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_address_is_whitelisted(&deps, info.sender.clone())?;

    // We can update the deployed amount immediately, since we know it is now transferred to the multisig.
    DEPLOYED_AMOUNT.update(deps.storage, env.block.height, |current_value| {
        current_value
            .unwrap_or_default()
            .checked_add(amount)
            .map_err(|e| StdError::generic_err(format!("overflow error: {e}")))
    })?;

    // There is no need to check if the smart contract balance has the required
    // amount of tokens deposited, since the transaction will fail if it doesn't.
    let send_tokens_msg = BankMsg::Send {
        to_address: info.sender.to_string(),
        amount: vec![Coin {
            amount,
            denom: config.deposit_denom.clone(),
        }],
    };

    Ok(Response::new()
        .add_message(send_tokens_msg)
        .add_attribute("action", "withdraw_for_deployment")
        .add_attribute("sender", info.sender)
        .add_attribute("amount", amount))
}

// Adds a new account address to the whitelist.
fn add_to_whitelist(
    deps: DepsMut<NeutronQuery>,
    _env: Env,
    info: MessageInfo,
    address: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_address_is_whitelisted(&deps, info.sender.clone())?;
    let whitelist_address = deps.api.addr_validate(&address)?;

    // Return an error if the account address is already in the whitelist
    if WHITELIST
        .may_load(deps.storage, whitelist_address.clone())?
        .is_some()
    {
        return Err(ContractError::Std(StdError::generic_err(format!(
            "address {whitelist_address} is already in the whitelist"
        ))));
    }

    // Add address to whitelist
    WHITELIST.save(deps.storage, whitelist_address.clone(), &())?;

    Ok(Response::new()
        .add_attribute("action", "add_to_whitelist")
        .add_attribute("sender", info.sender)
        .add_attribute("added_whitelist_address", whitelist_address))
}

// Removes an account address from the whitelist.
fn remove_from_whitelist(
    deps: DepsMut<NeutronQuery>,
    _env: Env,
    info: MessageInfo,
    address: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_address_is_whitelisted(&deps, info.sender.clone())?;
    let whitelist_address = deps.api.addr_validate(&address)?;

    // Return an error if the account address is not in the whitelist
    if WHITELIST
        .may_load(deps.storage, whitelist_address.clone())?
        .is_none()
    {
        return Err(ContractError::Std(StdError::generic_err(format!(
            "address {whitelist_address} is not in the whitelist"
        ))));
    }

    // Remove address from the whitelist
    WHITELIST.remove(deps.storage, whitelist_address.clone());

    Ok(Response::new()
        .add_attribute("action", "remove_from_whitelist")
        .add_attribute("sender", info.sender)
        .add_attribute("removed_whitelist_address", whitelist_address))
}

fn validate_address_is_whitelisted(
    deps: &DepsMut<NeutronQuery>,
    address: Addr,
) -> Result<(), ContractError> {
    let is_whitelisted = WHITELIST.may_load(deps.storage, address)?;
    if is_whitelisted.is_none() {
        return Err(ContractError::Unauthorized);
    }

    Ok(())
}

/// Given the `deposit amount`, this function will calculate how many vault shares tokens should be minted in return.
pub fn calculate_number_of_shares_to_mint(
    deps: &Deps<NeutronQuery>,
    env: &Env,
    config: &Config,
    deposit_amount: Uint128,
) -> Result<Uint128, ContractError> {
    let contract_deposit_token_balance = deps
        .querier
        .query_balance(
            env.contract.address.to_string(),
            config.deposit_denom.clone(),
        )?
        .amount;

    let deployed_amount = DEPLOYED_AMOUNT.load(deps.storage)?;

    // `deposit_amount` has already been added to the smart contract balance even before `execute()` is called,
    // so we need to subtract it here in order to accurately calculate number of vault shares to mint.
    let deposit_token_current_balance = Decimal::from_ratio(
        contract_deposit_token_balance
            .checked_sub(deposit_amount)?
            .checked_add(deployed_amount)?,
        Uint128::one(),
    );

    let total_shares_issued = Decimal::from_ratio(query_total_shares_issued(deps)?, Uint128::one());

    // If it is the first deposit, vault shares have 1:1 ratio with the deposit token.
    if deposit_token_current_balance.is_zero() || total_shares_issued.is_zero() {
        return Ok(deposit_amount);
    }

    let ratio = total_shares_issued.checked_div(deposit_token_current_balance)?;

    Ok(Decimal::from_ratio(deposit_amount, Uint128::one())
        .checked_mul(ratio)?
        .to_uint_floor())
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
pub fn query(deps: Deps<NeutronQuery>, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&query_config(&deps)?),
        QueryMsg::TotalSharesIssued {} => to_json_binary(&query_total_shares_issued(&deps)?),
        QueryMsg::TotalPoolValue {} => to_json_binary(&query_total_pool_value(&deps, env)?),
    }
}

fn query_config(deps: &Deps<NeutronQuery>) -> StdResult<ConfigResponse> {
    Ok(ConfigResponse {
        config: CONFIG.load(deps.storage)?,
    })
}

fn query_total_shares_issued(deps: &Deps<NeutronQuery>) -> StdResult<Uint128> {
    Ok(deps
        .querier
        .query_supply(VAULT_SHARES_DENOM.load(deps.storage)?)?
        .amount)
}

fn query_total_pool_value(
    deps: &Deps<NeutronQuery>,
    env: Env,
) -> StdResult<TotalPoolValueResponse> {
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

/// Creates MsgSetDenomMetadata that will set the metadata for the previously created `full_denom` token.
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
