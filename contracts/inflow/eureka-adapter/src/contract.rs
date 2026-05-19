use cosmwasm_std::{
    entry_point, to_json_binary, BankMsg, Binary, Coin, Deps, DepsMut, Env, MessageInfo, Order,
    Response, StdResult, Uint128,
};
use cw2::set_contract_version;
use cw_storage_plus::Bound;
use interface::utils::{DEFAULT_PAGINATION_LIMIT, MAX_PAGINATION_LIMIT};
use neutron_sdk::bindings::msg::NeutronMsg;
use neutron_sdk::bindings::query::NeutronQuery;

use crate::error::ContractError;
use crate::eureka::construct_eureka_memo;
use crate::ibc::create_ibc_transfer_msg;
use crate::msg::{
    AdapterInterfaceMsg, AdapterInterfaceQueryMsg, AdminsResponse, AllChainsResponse,
    AllPositionsResponse, AllTokensResponse, AllowedDestinationAddressesResponse,
    AllowedRecoverAddressesResponse, AvailableAmountResponse, ChainConfigResponse, ConfigResponse,
    DepositorCapabilitiesResponse, DepositorPositionResponse, DepositorPositionsResponse,
    EurekaAdapterMsg, EurekaAdapterQueryMsg, ExecuteMsg, ExecutorInfo, ExecutorsResponse,
    InstantiateMsg, QueryMsg, RegisteredDepositorInfo, RegisteredDepositorsResponse,
    TimeEstimateResponse, TokenConfigResponse, UpdateConfigData,
};
use crate::state::{
    ChainConfig, Config, Depositor, DepositorCapabilities, TokenConfig, TransferFundsInstructions,
    ADMINS, ALLOWED_DESTINATION_ADDRESSES, ALLOWED_RECOVER_ADDRESSES, CHAIN_REGISTRY, CONFIG,
    EXECUTORS, TOKEN_REGISTRY, WHITELISTED_DEPOSITORS,
};
use crate::validation::{
    get_depositor, get_destination_address, normalize_evm_address, validate_admin_caller,
    validate_cosmos_address, validate_depositor_caller, validate_executor_caller,
    validate_recover_address,
};

pub const CONTRACT_NAME: &str = env!("CARGO_PKG_NAME");
pub const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

// ========== INSTANTIATE ==========

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut<NeutronQuery>,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response<NeutronMsg>, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    if msg.admins.is_empty() {
        return Err(ContractError::AtLeastOneAdmin {});
    }

    let validated_admins = msg
        .admins
        .iter()
        .map(|a| deps.api.addr_validate(a))
        .collect::<StdResult<Vec<_>>>()?;
    let mut unique_admins = validated_admins;
    unique_admins.sort();
    unique_admins.dedup();
    ADMINS.save(deps.storage, &unique_admins)?;

    // Validate and store executors
    let validated_executors = msg
        .initial_executors
        .iter()
        .map(|e| deps.api.addr_validate(&e.address))
        .collect::<StdResult<Vec<_>>>()?;
    let mut unique_executors = validated_executors;
    unique_executors.sort();
    unique_executors.dedup();
    for executor in unique_executors {
        EXECUTORS.save(deps.storage, executor, &())?;
    }

    // Register initial depositors
    for initial_depositor in msg.initial_depositors {
        let addr = deps.api.addr_validate(&initial_depositor.address)?;
        if WHITELISTED_DEPOSITORS.has(deps.storage, addr.clone()) {
            return Err(ContractError::DepositorAlreadyRegistered {
                depositor_address: addr.to_string(),
            });
        }
        let capabilities = initial_depositor
            .capabilities
            .unwrap_or(DepositorCapabilities { can_withdraw: true });
        let depositor = Depositor {
            enabled: true,
            capabilities,
        };
        WHITELISTED_DEPOSITORS.save(deps.storage, addr, &depositor)?;
    }

    // Register initial chains and destination addresses
    for mut initial_chain in msg.initial_chains {
        initial_chain.chain_config = initial_chain.chain_config.validate_and_normalize()?;
        let chain_id = initial_chain.chain_config.chain_id.clone();

        if CHAIN_REGISTRY.has(deps.storage, chain_id.clone()) {
            return Err(ContractError::ChainAlreadyRegistered {
                chain_id: chain_id.clone(),
            });
        }
        CHAIN_REGISTRY.save(deps.storage, chain_id.clone(), &initial_chain.chain_config)?;

        for dest_addr in initial_chain.initial_allowed_destination_addresses {
            let normalized = normalize_evm_address(&dest_addr)?;
            let key = (chain_id.clone(), normalized.clone());
            if ALLOWED_DESTINATION_ADDRESSES.has(deps.storage, key.clone()) {
                return Err(ContractError::DestinationAddressAlreadyExists {
                    chain_id: chain_id.clone(),
                    address: normalized,
                });
            }
            ALLOWED_DESTINATION_ADDRESSES.save(deps.storage, key, &())?;
        }
    }

    // Register initial tokens
    for token in msg.initial_tokens {
        if TOKEN_REGISTRY.has(deps.storage, token.denom.clone()) {
            return Err(ContractError::TokenAlreadyRegistered {
                denom: token.denom.clone(),
            });
        }
        TOKEN_REGISTRY.save(deps.storage, token.denom.clone(), &token)?;
    }

    // Register initial recover addresses
    for addr in msg.initial_recover_addresses {
        let validated = validate_cosmos_address(&addr)?;
        if ALLOWED_RECOVER_ADDRESSES.has(deps.storage, validated.clone()) {
            return Err(ContractError::RecoverAddressAlreadyExists { address: validated });
        }
        ALLOWED_RECOVER_ADDRESSES.save(deps.storage, validated, &())?;
    }

    let config = Config {
        skip_entry_point: msg.skip_entry_point,
        skip_ibc_adapter: msg.skip_ibc_adapter,
        neutron_to_hub_channel: msg.neutron_to_hub_channel,
        ibc_default_timeout_seconds: msg.ibc_default_timeout_seconds,
    };
    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new()
        .add_attribute("action", "instantiate")
        .add_attribute("contract_name", CONTRACT_NAME)
        .add_attribute("contract_version", CONTRACT_VERSION))
}

// ========== EXECUTE ==========

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response<NeutronMsg>, ContractError> {
    match msg {
        ExecuteMsg::StandardAction(interface_msg) => {
            dispatch_execute_standard(deps, env, info, interface_msg)
        }
        ExecuteMsg::CustomAction(custom_msg) => {
            dispatch_execute_custom(deps, env, info, custom_msg)
        }
    }
}

fn dispatch_execute_standard(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    msg: AdapterInterfaceMsg,
) -> Result<Response<NeutronMsg>, ContractError> {
    match msg {
        AdapterInterfaceMsg::Deposit {} => execute_deposit(deps, info),
        AdapterInterfaceMsg::Withdraw { coin } => execute_withdraw(deps, env, info, coin),
        AdapterInterfaceMsg::RegisterDepositor {
            depositor_address,
            metadata,
        } => execute_register_depositor(deps, info, depositor_address, metadata),
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

fn dispatch_execute_custom(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    msg: EurekaAdapterMsg,
) -> Result<Response<NeutronMsg>, ContractError> {
    match msg {
        EurekaAdapterMsg::TransferFunds { instructions } => {
            execute_transfer_funds(deps, env, info, instructions)
        }
        EurekaAdapterMsg::UpdateConfig { update } => execute_update_config(deps, info, update),
        EurekaAdapterMsg::AddExecutor { executor_address } => {
            execute_add_executor(deps, info, executor_address)
        }
        EurekaAdapterMsg::RemoveExecutor { executor_address } => {
            execute_remove_executor(deps, info, executor_address)
        }
        EurekaAdapterMsg::RegisterChain { chain_config } => {
            execute_register_chain(deps, info, chain_config)
        }
        EurekaAdapterMsg::UpdateRegisteredChain { chain_config } => {
            execute_update_registered_chain(deps, info, chain_config)
        }
        EurekaAdapterMsg::UnregisterChain { chain_id } => {
            execute_unregister_chain(deps, info, chain_id)
        }
        EurekaAdapterMsg::RegisterToken { denom, hub_denom } => {
            execute_register_token(deps, info, denom, hub_denom)
        }
        EurekaAdapterMsg::UnregisterToken { denom } => execute_unregister_token(deps, info, denom),
        EurekaAdapterMsg::AddAllowedDestinationAddress { chain_id, address } => {
            execute_add_allowed_destination_address(deps, info, chain_id, address)
        }
        EurekaAdapterMsg::RemoveAllowedDestinationAddress { chain_id, address } => {
            execute_remove_allowed_destination_address(deps, info, chain_id, address)
        }
        EurekaAdapterMsg::AddAllowedRecoverAddress { address } => {
            execute_add_allowed_recover_address(deps, info, address)
        }
        EurekaAdapterMsg::RemoveAllowedRecoverAddress { address } => {
            execute_remove_allowed_recover_address(deps, info, address)
        }
    }
}

// ========== EXECUTE HANDLERS ==========

fn execute_deposit(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_depositor_caller(&deps, &info)?;

    if info.funds.len() != 1 {
        return Err(ContractError::Std(cosmwasm_std::StdError::generic_err(
            format!("expected exactly one coin, got {}", info.funds.len()),
        )));
    }
    let coin = &info.funds[0];

    if coin.amount.is_zero() {
        return Err(ContractError::ZeroAmount {});
    }

    TOKEN_REGISTRY
        .may_load(deps.storage, coin.denom.clone())?
        .ok_or(ContractError::TokenNotRegistered {
            denom: coin.denom.clone(),
        })?;

    Ok(Response::new()
        .add_attribute("action", "deposit")
        .add_attribute("sender", info.sender)
        .add_attribute("amount", coin.amount)
        .add_attribute("denom", &coin.denom))
}

fn execute_withdraw(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    coin: Coin,
) -> Result<Response<NeutronMsg>, ContractError> {
    let depositor = validate_depositor_caller(&deps, &info)?;
    if !depositor.capabilities.can_withdraw {
        return Err(ContractError::WithdrawalNotAllowed {});
    }

    if coin.amount.is_zero() {
        return Err(ContractError::ZeroAmount {});
    }

    TOKEN_REGISTRY
        .may_load(deps.storage, coin.denom.clone())?
        .ok_or(ContractError::TokenNotRegistered {
            denom: coin.denom.clone(),
        })?;

    let balance = deps
        .querier
        .query_balance(env.contract.address, coin.denom.clone())?;
    if balance.amount < coin.amount {
        return Err(ContractError::InsufficientBalance {
            has: balance.amount,
            needs: coin.amount,
        });
    }

    let bank_msg = BankMsg::Send {
        to_address: info.sender.to_string(),
        amount: vec![coin.clone()],
    };

    Ok(Response::new()
        .add_message(bank_msg)
        .add_attribute("action", "withdraw")
        .add_attribute("sender", info.sender)
        .add_attribute("amount", coin.amount)
        .add_attribute("denom", coin.denom))
}

fn execute_transfer_funds(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    instructions: TransferFundsInstructions,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_executor_caller(&deps, &info)?;

    // Executor sends the Eureka fee in info.funds (same denom as the bridged token)
    let eureka_fee_amount = cw_utils::must_pay(&info, &instructions.denom)?;

    // Load chain config
    let chain_config = CHAIN_REGISTRY
        .may_load(deps.storage, instructions.chain_id.clone())?
        .ok_or(ContractError::ChainNotRegistered {
            chain_id: instructions.chain_id.clone(),
        })?;

    // Validate fee bounds
    if eureka_fee_amount < chain_config.min_eureka_fee {
        return Err(ContractError::EurekaFeeTooLow {
            fee: eureka_fee_amount,
            min: chain_config.min_eureka_fee,
        });
    }
    if eureka_fee_amount > chain_config.max_eureka_fee {
        return Err(ContractError::EurekaFeeTooHigh {
            fee: eureka_fee_amount,
            max: chain_config.max_eureka_fee,
        });
    }

    if instructions.amount.is_zero() {
        return Err(ContractError::ZeroAmount {});
    }

    // Validate and load token config
    let token_config = TOKEN_REGISTRY
        .may_load(deps.storage, instructions.denom.clone())?
        .ok_or(ContractError::TokenNotRegistered {
            denom: instructions.denom.clone(),
        })?;

    // Validate destination address (EVM)
    let destination = get_destination_address(
        &deps.as_ref(),
        &instructions.chain_id,
        &instructions.recipient,
    )?;

    // Validate recover address
    validate_recover_address(&deps.as_ref(), &instructions.recover_address)?;

    // Verify adapter has sufficient balance for the bridge amount
    // (eureka_fee_amount is already in the contract via info.funds)
    // total_transfer = amount (from adapter balance) + eureka_fee (from info.funds)
    let total_transfer = instructions.amount + eureka_fee_amount;
    let balance = deps
        .querier
        .query_balance(env.contract.address.clone(), instructions.denom.clone())?;
    if balance.amount < total_transfer {
        return Err(ContractError::InsufficientBalance {
            has: balance.amount,
            needs: total_transfer,
        });
    }

    let config = CONFIG.load(deps.storage)?;

    let memo = construct_eureka_memo(
        &config,
        &chain_config,
        &token_config,
        &destination,
        &instructions.recover_address,
        eureka_fee_amount,
        &env,
    )?;

    let total_coin = Coin {
        denom: instructions.denom.clone(),
        amount: total_transfer,
    };

    let ibc_msg = create_ibc_transfer_msg(
        deps.as_ref(),
        &env,
        &config,
        total_coin,
        config.skip_ibc_adapter.clone(),
        memo,
        config.ibc_default_timeout_seconds,
    )?;

    Ok(Response::new()
        .add_message(ibc_msg)
        .add_attribute("action", "transfer_funds")
        .add_attribute("sender", info.sender)
        .add_attribute("amount", instructions.amount)
        .add_attribute("eureka_fee", eureka_fee_amount)
        .add_attribute("total_transfer", total_transfer)
        .add_attribute("chain_id", instructions.chain_id)
        .add_attribute("destination", destination)
        .add_attribute("denom", instructions.denom))
}

fn execute_update_config(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    update: UpdateConfigData,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_admin_caller(&deps.as_ref(), &info)?;

    let mut config = CONFIG.load(deps.storage)?;

    if let Some(skip_entry_point) = update.skip_entry_point {
        config.skip_entry_point = skip_entry_point;
    }
    if let Some(skip_ibc_adapter) = update.skip_ibc_adapter {
        config.skip_ibc_adapter = skip_ibc_adapter;
    }
    if let Some(channel) = update.neutron_to_hub_channel {
        config.neutron_to_hub_channel = channel;
    }
    if let Some(timeout) = update.ibc_default_timeout_seconds {
        config.ibc_default_timeout_seconds = timeout;
    }

    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new()
        .add_attribute("action", "update_config")
        .add_attribute("sender", info.sender))
}

fn execute_add_executor(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    executor_address: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_admin_caller(&deps.as_ref(), &info)?;

    let executor_addr = deps.api.addr_validate(&executor_address)?;
    if EXECUTORS.has(deps.storage, executor_addr.clone()) {
        return Err(ContractError::ExecutorAlreadyExists {
            executor: executor_address,
        });
    }

    EXECUTORS.save(deps.storage, executor_addr.clone(), &())?;

    Ok(Response::new()
        .add_attribute("action", "add_executor")
        .add_attribute("sender", info.sender)
        .add_attribute("executor", executor_addr))
}

fn execute_remove_executor(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    executor_address: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_admin_caller(&deps.as_ref(), &info)?;

    let executor_addr = deps.api.addr_validate(&executor_address)?;
    if !EXECUTORS.has(deps.storage, executor_addr.clone()) {
        return Err(ContractError::ExecutorNotFound {
            executor: executor_address,
        });
    }

    EXECUTORS.remove(deps.storage, executor_addr.clone());

    Ok(Response::new()
        .add_attribute("action", "remove_executor")
        .add_attribute("sender", info.sender)
        .add_attribute("executor", executor_addr))
}

fn execute_add_admin(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    admin_address: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_admin_caller(&deps.as_ref(), &info)?;

    let admin_addr = deps.api.addr_validate(&admin_address)?;
    let mut admins = ADMINS.load(deps.storage)?;

    if admins.contains(&admin_addr) {
        return Err(ContractError::AdminAlreadyExists {
            admin: admin_address,
        });
    }

    admins.push(admin_addr.clone());
    ADMINS.save(deps.storage, &admins)?;

    Ok(Response::new()
        .add_attribute("action", "add_admin")
        .add_attribute("sender", info.sender)
        .add_attribute("admin", admin_addr))
}

fn execute_remove_admin(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    admin_address: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_admin_caller(&deps.as_ref(), &info)?;

    let admin_addr = deps.api.addr_validate(&admin_address)?;
    let mut admins = ADMINS.load(deps.storage)?;

    if !admins.contains(&admin_addr) {
        return Err(ContractError::AdminNotFound {
            admin: admin_address,
        });
    }

    if admins.len() <= 1 {
        return Err(ContractError::CannotRemoveLastAdmin {});
    }

    admins.retain(|a| a != admin_addr);
    ADMINS.save(deps.storage, &admins)?;

    Ok(Response::new()
        .add_attribute("action", "remove_admin")
        .add_attribute("sender", info.sender)
        .add_attribute("admin", admin_addr))
}

fn execute_register_depositor(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    depositor_address: String,
    metadata: Option<cosmwasm_std::Binary>,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_admin_caller(&deps.as_ref(), &info)?;

    let depositor_address = deps.api.addr_validate(&depositor_address)?;
    if WHITELISTED_DEPOSITORS.has(deps.storage, depositor_address.clone()) {
        return Err(ContractError::DepositorAlreadyRegistered {
            depositor_address: depositor_address.to_string(),
        });
    }

    let capabilities = if let Some(cap_binary) = metadata {
        cosmwasm_std::from_json(&cap_binary)?
    } else {
        DepositorCapabilities { can_withdraw: true }
    };

    let depositor = Depositor {
        enabled: true,
        capabilities,
    };

    WHITELISTED_DEPOSITORS.save(deps.storage, depositor_address.clone(), &depositor)?;

    Ok(Response::new()
        .add_attribute("action", "register_depositor")
        .add_attribute("depositor_address", depositor_address))
}

fn execute_unregister_depositor(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    depositor_address: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_admin_caller(&deps.as_ref(), &info)?;

    let depositor_address = deps.api.addr_validate(&depositor_address)?;
    WHITELISTED_DEPOSITORS.remove(deps.storage, depositor_address.clone());

    Ok(Response::new()
        .add_attribute("action", "unregister_depositor")
        .add_attribute("sender", info.sender)
        .add_attribute("depositor_address", depositor_address))
}

fn execute_set_depositor_enabled(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    depositor_address: String,
    enabled: bool,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_admin_caller(&deps.as_ref(), &info)?;

    let depositor_address = deps.api.addr_validate(&depositor_address)?;
    let mut depositor = WHITELISTED_DEPOSITORS
        .may_load(deps.storage, depositor_address.clone())?
        .ok_or(ContractError::DepositorNotRegistered {
            depositor_address: depositor_address.to_string(),
        })?;

    depositor.enabled = enabled;
    WHITELISTED_DEPOSITORS.save(deps.storage, depositor_address.clone(), &depositor)?;

    Ok(Response::new()
        .add_attribute("action", "set_depositor_enabled")
        .add_attribute("sender", info.sender)
        .add_attribute("depositor_address", depositor_address.to_string())
        .add_attribute("enabled", enabled.to_string()))
}

fn execute_register_chain(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    mut chain_config: ChainConfig,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_admin_caller(&deps.as_ref(), &info)?;

    chain_config = chain_config.validate_and_normalize()?;
    let chain_id = chain_config.chain_id.clone();

    if CHAIN_REGISTRY.has(deps.storage, chain_id.clone()) {
        return Err(ContractError::ChainAlreadyRegistered {
            chain_id: chain_id.clone(),
        });
    }

    CHAIN_REGISTRY.save(deps.storage, chain_id.clone(), &chain_config)?;

    Ok(Response::new()
        .add_attribute("action", "register_chain")
        .add_attribute("sender", info.sender)
        .add_attribute("chain_id", chain_id))
}

fn execute_update_registered_chain(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    mut chain_config: ChainConfig,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_admin_caller(&deps.as_ref(), &info)?;

    chain_config = chain_config.validate_and_normalize()?;
    let chain_id = chain_config.chain_id.clone();

    if !CHAIN_REGISTRY.has(deps.storage, chain_id.clone()) {
        return Err(ContractError::ChainNotRegistered {
            chain_id: chain_id.clone(),
        });
    }

    CHAIN_REGISTRY.save(deps.storage, chain_id.clone(), &chain_config)?;

    Ok(Response::new()
        .add_attribute("action", "update_registered_chain")
        .add_attribute("sender", info.sender)
        .add_attribute("chain_id", chain_id))
}

fn execute_unregister_chain(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    chain_id: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_admin_caller(&deps.as_ref(), &info)?;

    let chain_id = chain_id.trim().to_string();

    if !CHAIN_REGISTRY.has(deps.storage, chain_id.clone()) {
        return Err(ContractError::ChainNotRegistered {
            chain_id: chain_id.clone(),
        });
    }

    // Remove all destination addresses for this chain
    let addresses_to_remove: Vec<String> = ALLOWED_DESTINATION_ADDRESSES
        .prefix(chain_id.clone())
        .keys(deps.storage, None, None, Order::Ascending)
        .collect::<StdResult<Vec<_>>>()?;

    for address in addresses_to_remove {
        ALLOWED_DESTINATION_ADDRESSES.remove(deps.storage, (chain_id.clone(), address));
    }

    CHAIN_REGISTRY.remove(deps.storage, chain_id.clone());

    Ok(Response::new()
        .add_attribute("action", "unregister_chain")
        .add_attribute("sender", info.sender)
        .add_attribute("chain_id", chain_id))
}

fn execute_register_token(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    denom: String,
    hub_denom: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_admin_caller(&deps.as_ref(), &info)?;

    if TOKEN_REGISTRY.has(deps.storage, denom.clone()) {
        return Err(ContractError::TokenAlreadyRegistered {
            denom: denom.clone(),
        });
    }

    let token_config = TokenConfig {
        denom: denom.clone(),
        hub_denom,
    };
    TOKEN_REGISTRY.save(deps.storage, denom.clone(), &token_config)?;

    Ok(Response::new()
        .add_attribute("action", "register_token")
        .add_attribute("sender", info.sender)
        .add_attribute("denom", denom))
}

fn execute_unregister_token(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    denom: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_admin_caller(&deps.as_ref(), &info)?;

    if !TOKEN_REGISTRY.has(deps.storage, denom.clone()) {
        return Err(ContractError::TokenNotRegistered {
            denom: denom.clone(),
        });
    }

    TOKEN_REGISTRY.remove(deps.storage, denom.clone());

    Ok(Response::new()
        .add_attribute("action", "unregister_token")
        .add_attribute("sender", info.sender)
        .add_attribute("denom", denom))
}

fn execute_add_allowed_destination_address(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    chain_id: String,
    address: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_admin_caller(&deps.as_ref(), &info)?;

    if !CHAIN_REGISTRY.has(deps.storage, chain_id.clone()) {
        return Err(ContractError::ChainNotRegistered {
            chain_id: chain_id.clone(),
        });
    }

    let normalized = normalize_evm_address(&address)?;
    let key = (chain_id.clone(), normalized.clone());

    if ALLOWED_DESTINATION_ADDRESSES.has(deps.storage, key.clone()) {
        return Err(ContractError::DestinationAddressAlreadyExists {
            chain_id: chain_id.clone(),
            address: normalized,
        });
    }

    ALLOWED_DESTINATION_ADDRESSES.save(deps.storage, key, &())?;

    Ok(Response::new()
        .add_attribute("action", "add_allowed_destination_address")
        .add_attribute("sender", info.sender)
        .add_attribute("chain_id", chain_id)
        .add_attribute("address", normalized))
}

fn execute_remove_allowed_destination_address(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    chain_id: String,
    address: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_admin_caller(&deps.as_ref(), &info)?;

    let normalized = normalize_evm_address(&address)?;
    let key = (chain_id.clone(), normalized.clone());

    if !ALLOWED_DESTINATION_ADDRESSES.has(deps.storage, key.clone()) {
        return Err(ContractError::DestinationAddressDoesNotExist {
            chain_id: chain_id.clone(),
            address: normalized,
        });
    }

    ALLOWED_DESTINATION_ADDRESSES.remove(deps.storage, key);

    Ok(Response::new()
        .add_attribute("action", "remove_allowed_destination_address")
        .add_attribute("sender", info.sender)
        .add_attribute("chain_id", chain_id)
        .add_attribute("address", normalized))
}

fn execute_add_allowed_recover_address(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    address: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_admin_caller(&deps.as_ref(), &info)?;

    let validated = validate_cosmos_address(&address)?;

    if ALLOWED_RECOVER_ADDRESSES.has(deps.storage, validated.clone()) {
        return Err(ContractError::RecoverAddressAlreadyExists { address: validated });
    }

    ALLOWED_RECOVER_ADDRESSES.save(deps.storage, validated.clone(), &())?;

    Ok(Response::new()
        .add_attribute("action", "add_allowed_recover_address")
        .add_attribute("sender", info.sender)
        .add_attribute("address", validated))
}

fn execute_remove_allowed_recover_address(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
    address: String,
) -> Result<Response<NeutronMsg>, ContractError> {
    validate_admin_caller(&deps.as_ref(), &info)?;

    let validated = validate_cosmos_address(&address)?;

    if !ALLOWED_RECOVER_ADDRESSES.has(deps.storage, validated.clone()) {
        return Err(ContractError::RecoverAddressDoesNotExist { address: validated });
    }

    ALLOWED_RECOVER_ADDRESSES.remove(deps.storage, validated.clone());

    Ok(Response::new()
        .add_attribute("action", "remove_allowed_recover_address")
        .add_attribute("sender", info.sender)
        .add_attribute("address", validated))
}

// ========== QUERY ==========

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps<NeutronQuery>, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::StandardQuery(interface_msg) => dispatch_query_standard(deps, env, interface_msg),
        QueryMsg::CustomQuery(custom_msg) => dispatch_query_custom(deps, custom_msg),
    }
}

fn dispatch_query_standard(
    deps: Deps<NeutronQuery>,
    env: Env,
    msg: AdapterInterfaceQueryMsg,
) -> StdResult<Binary> {
    match msg {
        AdapterInterfaceQueryMsg::Config {} => to_json_binary(&query_config(deps)?),
        AdapterInterfaceQueryMsg::AvailableForDeposit {
            depositor_address: _,
            denom: _,
        } => to_json_binary(&AvailableAmountResponse {
            amount: Uint128::MAX,
        }),
        AdapterInterfaceQueryMsg::AvailableForWithdraw {
            depositor_address: _,
            denom,
        } => to_json_binary(&query_available_for_withdraw(deps, env, denom)?),
        AdapterInterfaceQueryMsg::TimeToWithdraw {
            depositor_address: _,
            coin: _,
        } => to_json_binary(&TimeEstimateResponse {
            blocks: 0,
            seconds: 0,
        }),
        AdapterInterfaceQueryMsg::AllPositions {} => {
            to_json_binary(&AllPositionsResponse { positions: vec![] })
        }
        AdapterInterfaceQueryMsg::DepositorPosition {
            depositor_address: _,
            denom: _,
        } => to_json_binary(&DepositorPositionResponse {
            amount: Uint128::zero(),
        }),
        AdapterInterfaceQueryMsg::DepositorPositions {
            depositor_address: _,
        } => to_json_binary(&DepositorPositionsResponse { positions: vec![] }),
        AdapterInterfaceQueryMsg::RegisteredDepositors { enabled } => {
            to_json_binary(&query_registered_depositors(deps, enabled)?)
        }
        AdapterInterfaceQueryMsg::Admins {} => to_json_binary(&query_admins(deps)?),
    }
}

fn dispatch_query_custom(
    deps: Deps<NeutronQuery>,
    msg: EurekaAdapterQueryMsg,
) -> StdResult<Binary> {
    match msg {
        EurekaAdapterQueryMsg::Config {} => to_json_binary(&query_config(deps)?),
        EurekaAdapterQueryMsg::ChainConfig { chain_id } => {
            to_json_binary(&query_chain_config(deps, chain_id)?)
        }
        EurekaAdapterQueryMsg::AllChains {} => to_json_binary(&query_all_chains(deps)?),
        EurekaAdapterQueryMsg::TokenConfig { denom } => {
            to_json_binary(&query_token_config(deps, denom)?)
        }
        EurekaAdapterQueryMsg::AllTokens {} => to_json_binary(&query_all_tokens(deps)?),
        EurekaAdapterQueryMsg::Executors {} => to_json_binary(&query_executors(deps)?),
        EurekaAdapterQueryMsg::DepositorCapabilities { depositor_address } => {
            to_json_binary(&query_depositor_capabilities(deps, depositor_address)?)
        }
        EurekaAdapterQueryMsg::AllowedDestinationAddresses {
            chain_id,
            start_after,
            limit,
        } => to_json_binary(&query_allowed_destination_addresses(
            deps,
            chain_id,
            start_after,
            limit,
        )?),
        EurekaAdapterQueryMsg::AllowedRecoverAddresses { start_after, limit } => {
            to_json_binary(&query_allowed_recover_addresses(deps, start_after, limit)?)
        }
    }
}

// ========== QUERY HANDLERS ==========

fn query_config(deps: Deps<NeutronQuery>) -> StdResult<ConfigResponse> {
    Ok(ConfigResponse {
        config: CONFIG.load(deps.storage)?,
    })
}

fn query_available_for_withdraw(
    deps: Deps<NeutronQuery>,
    env: Env,
    denom: String,
) -> StdResult<AvailableAmountResponse> {
    let balance = deps.querier.query_balance(env.contract.address, denom)?;
    Ok(AvailableAmountResponse {
        amount: balance.amount,
    })
}

fn query_registered_depositors(
    deps: Deps<NeutronQuery>,
    enabled: Option<bool>,
) -> StdResult<RegisteredDepositorsResponse> {
    let depositors: Vec<RegisteredDepositorInfo> = WHITELISTED_DEPOSITORS
        .range(deps.storage, None, None, Order::Ascending)
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

fn query_admins(deps: Deps<NeutronQuery>) -> StdResult<AdminsResponse> {
    let admins = ADMINS.load(deps.storage)?;
    Ok(AdminsResponse {
        admins: admins.into_iter().map(|a| a.to_string()).collect(),
    })
}

fn query_chain_config(
    deps: Deps<NeutronQuery>,
    chain_id: String,
) -> StdResult<ChainConfigResponse> {
    let chain_config = CHAIN_REGISTRY.load(deps.storage, chain_id)?;
    Ok(ChainConfigResponse { chain_config })
}

fn query_all_chains(deps: Deps<NeutronQuery>) -> StdResult<AllChainsResponse> {
    let chains: Vec<ChainConfig> = CHAIN_REGISTRY
        .range(deps.storage, None, None, Order::Ascending)
        .map(|item| item.map(|(_, config)| config))
        .collect::<StdResult<Vec<_>>>()?;
    Ok(AllChainsResponse { chains })
}

fn query_token_config(deps: Deps<NeutronQuery>, denom: String) -> StdResult<TokenConfigResponse> {
    let token_config = TOKEN_REGISTRY.load(deps.storage, denom)?;
    Ok(TokenConfigResponse { token_config })
}

fn query_all_tokens(deps: Deps<NeutronQuery>) -> StdResult<AllTokensResponse> {
    let tokens: Vec<TokenConfig> = TOKEN_REGISTRY
        .range(deps.storage, None, None, Order::Ascending)
        .map(|item| item.map(|(_, config)| config))
        .collect::<StdResult<Vec<_>>>()?;
    Ok(AllTokensResponse { tokens })
}

fn query_executors(deps: Deps<NeutronQuery>) -> StdResult<ExecutorsResponse> {
    let executors: StdResult<Vec<ExecutorInfo>> = EXECUTORS
        .range(deps.storage, None, None, Order::Ascending)
        .map(|item| {
            let (addr, _) = item?;
            Ok(ExecutorInfo {
                executor_address: addr.to_string(),
            })
        })
        .collect();
    Ok(ExecutorsResponse {
        executors: executors?,
    })
}

fn query_depositor_capabilities(
    deps: Deps<NeutronQuery>,
    depositor_address: String,
) -> StdResult<DepositorCapabilitiesResponse> {
    let depositor = get_depositor(deps, depositor_address)?;
    Ok(DepositorCapabilitiesResponse {
        capabilities: depositor.capabilities,
    })
}

fn query_allowed_destination_addresses(
    deps: Deps<NeutronQuery>,
    chain_id: String,
    start_after: Option<String>,
    limit: Option<u32>,
) -> StdResult<AllowedDestinationAddressesResponse> {
    let limit = limit
        .unwrap_or(DEFAULT_PAGINATION_LIMIT)
        .min(MAX_PAGINATION_LIMIT) as usize;

    let start_bound = start_after
        .map(|addr| normalize_evm_address(&addr).map(Bound::exclusive))
        .transpose()
        .map_err(|e| cosmwasm_std::StdError::generic_err(e.to_string()))?;

    let addresses: Vec<String> = ALLOWED_DESTINATION_ADDRESSES
        .prefix(chain_id)
        .range(deps.storage, start_bound, None, Order::Ascending)
        .take(limit)
        .map(|item| item.map(|(addr, _)| addr))
        .collect::<StdResult<Vec<_>>>()?;

    Ok(AllowedDestinationAddressesResponse { addresses })
}

fn query_allowed_recover_addresses(
    deps: Deps<NeutronQuery>,
    start_after: Option<String>,
    limit: Option<u32>,
) -> StdResult<AllowedRecoverAddressesResponse> {
    let limit = limit
        .unwrap_or(DEFAULT_PAGINATION_LIMIT)
        .min(MAX_PAGINATION_LIMIT) as usize;

    let start_bound = start_after.map(Bound::exclusive);

    let addresses: Vec<String> = ALLOWED_RECOVER_ADDRESSES
        .range(deps.storage, start_bound, None, Order::Ascending)
        .take(limit)
        .map(|item| item.map(|(addr, _)| addr))
        .collect::<StdResult<Vec<_>>>()?;

    Ok(AllowedRecoverAddressesResponse { addresses })
}
