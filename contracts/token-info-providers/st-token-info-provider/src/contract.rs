use cosmwasm_std::{
    entry_point, from_json, to_json_binary, to_json_vec, Addr, BankMsg, Binary, Coin, Decimal,
    Deps, DepsMut, Env, MessageInfo, Reply, Response, StdError, StdResult, Storage, SubMsg,
    Uint128, WasmMsg,
};
use cw2::set_contract_version;
use interface::token_info_provider::DenomInfoResponse;
use neutron_sdk::bindings::msg::NeutronMsg;
use neutron_sdk::bindings::query::NeutronQuery;
use neutron_sdk::bindings::types::{KVKey, StorageValue};
use neutron_sdk::interchain_queries::types::{KVReconstruct, QueryPayload, QueryType};
use neutron_sdk::interchain_queries::{check_query_type, get_registered_query, query_kv_result};
use neutron_sdk::interchain_txs::helpers::decode_message_response;
use neutron_sdk::proto_types::neutron::interchainqueries::{
    MsgRegisterInterchainQueryResponse, MsgRemoveInterchainQueryResponse,
};
use neutron_sdk::sudo::msg::SudoMsg;
use neutron_sdk::NeutronResult;
use prost::Message;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

use crate::error::ContractError;
use crate::msg::{ExecuteMsg, HydroExecuteMsg, InstantiateMsg};
use crate::query::{
    ConfigResponse, HydroCurrentRoundResponse, HydroQueryMsg, InterchainQueryInfoResponse, QueryMsg,
};
use crate::state::{Config, InterchainQueryInfo, CONFIG, INTERCHAIN_QUERY_INFO, TOKEN_RATIO};

/// Contract name that is used for migration.
pub const CONTRACT_NAME: &str = env!("CARGO_PKG_NAME");
/// Contract version that is used for migration.
pub const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

// Stride stakeibc module KV store key.
// https://github.com/Stride-Labs/stride/blob/f9307f7012f4111678a4c6fa4d50f6ae638f4005/x/stakeibc/types/keys.go#L8
pub const STRIDE_STAKEIBC_STORE_KEY: &str = "stakeibc";

// Stride host zone key prefix
// https://github.com/Stride-Labs/stride/blob/f9307f7012f4111678a4c6fa4d50f6ae638f4005/x/stakeibc/types/keys.go#L57
pub const STRIDE_HOST_ZONE_STORE_KEY: &str = "HostZone-value-";

const UNUSED_MSG_ID: u64 = 0;
pub const NATIVE_TOKEN_DENOM: &str = "untrn";

// Decimal values received through Interchain Query results are represented with 18 decimal places,
// so we need to divide them by 10^18 to get the actual value.
pub const DENOMINATOR: Uint128 = Uint128::new(1_000_000_000_000_000_000);

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut<NeutronQuery>,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    let config = Config {
        hydro_contract_address: info.sender.clone(),
        st_token_denom: msg.st_token_denom.clone(),
        token_group_id: msg.token_group_id.clone(),
        stride_connection_id: msg.stride_connection_id.clone(),
        icq_update_period: msg.icq_update_period,
        stride_host_zone_id: msg.stride_host_zone_id.clone(),
    };

    CONFIG.save(deps.storage, &config)?;
    TOKEN_RATIO.save(deps.storage, 0, &Decimal::zero())?;

    Ok(Response::new()
        .add_attribute("action", "initialisation")
        .add_attribute("sender", info.sender)
        .add_attribute("st_token_denom", msg.st_token_denom)
        .add_attribute("token_group_id", msg.token_group_id)
        .add_attribute("stride_connection_id", msg.stride_connection_id)
        .add_attribute("icq_update_period", msg.icq_update_period.to_string())
        .add_attribute("stride_host_zone_id", msg.stride_host_zone_id))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut<NeutronQuery>,
    _env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response<NeutronMsg>, ContractError> {
    match msg {
        ExecuteMsg::RegisterHostZoneICQ {} => register_host_zone_icq(deps, info),
        ExecuteMsg::RemoveHostZoneICQ {} => remove_host_zone_icq(deps, info),
    }
}

// RegisterHostZoneICQ():
//     Check if the interchain query is already registered.
//     Build the KV store key of the Stride host zone we are interested in.
//     Build SubMsg to register interchain query with the created key.
//
// Note that the ICQ creation deposit isn't validated, since this contract will not
// hold any funds, thus the transaction will fail if the deposit is not provided.
// Instead, we keep track of the funds sent by wallet that executed the transaction,
// so that we can refund them back to the sender when they remove the interchain query.
fn register_host_zone_icq(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
) -> Result<Response<NeutronMsg>, ContractError> {
    if query_interchain_query_info(deps.as_ref())?.info.is_some() {
        return Err(ContractError::Std(StdError::generic_err(
            "host zone interchain query is already registered",
        )));
    }

    let config = CONFIG.load(deps.storage)?;

    let mut key = Vec::new();
    key.extend_from_slice(STRIDE_HOST_ZONE_STORE_KEY.as_bytes());
    key.extend_from_slice(config.stride_host_zone_id.as_bytes());

    let kv_keys = vec![KVKey {
        path: STRIDE_STAKEIBC_STORE_KEY.to_string(),
        key: Binary::new(key),
    }];

    let register_icq_msg = NeutronMsg::register_interchain_query(
        QueryPayload::KV(kv_keys),
        config.stride_connection_id,
        config.icq_update_period,
    )?;

    let submsg = SubMsg::reply_on_success(register_icq_msg, UNUSED_MSG_ID).with_payload(
        to_json_vec(&ReplyPayload::RegisterHostZoneICQ {
            creator: info.sender.to_string(),
            funds: info.funds,
        })?,
    );

    Ok(Response::new()
        .add_submessage(submsg)
        .add_attribute("action", "register_host_zone_icq")
        .add_attribute("sender", info.sender))
}

// RemoveHostZoneICQ():
//     Check if the interchain query is registered.
//     Check if the tx sender is the creator of the given interchain query.
//     Build SubMsg to remove interchain query with the registered query ID.
//     Build SubMsg to refund the deposit paid for the interchain query creation.
fn remove_host_zone_icq(
    deps: DepsMut<NeutronQuery>,
    info: MessageInfo,
) -> Result<Response<NeutronMsg>, ContractError> {
    let interchain_query_info = INTERCHAIN_QUERY_INFO
        .load(deps.storage)
        .map_err(|_| StdError::generic_err("interchain query is not registered"))?;

    if interchain_query_info.creator != info.sender.to_string() {
        return Err(ContractError::Unauthorized {});
    }

    let remove_icq_msg = SubMsg::reply_on_success(
        NeutronMsg::remove_interchain_query(interchain_query_info.query_id),
        UNUSED_MSG_ID,
    )
    .with_payload(to_json_vec(&ReplyPayload::RemoveHostZoneICQ {
        query_id: interchain_query_info.query_id,
    })?);

    let refund_deposit_msg = SubMsg::new(BankMsg::Send {
        to_address: info.sender.to_string(),
        amount: interchain_query_info.deposit_paid,
    });

    Ok(Response::new()
        .add_submessages(vec![remove_icq_msg, refund_deposit_msg])
        .add_attribute("action", "remove_host_zone_icq")
        .add_attribute("sender", info.sender)
        .add_attribute(
            "interchain_query_id",
            interchain_query_info.query_id.to_string(),
        ))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps<NeutronQuery>, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&query_config(deps)?),
        QueryMsg::InterchainQueryInfo {} => to_json_binary(&query_interchain_query_info(deps)?),
        QueryMsg::DenomInfo { round_id } => to_json_binary(&query_denom_info(deps, round_id)?),
    }
}

fn query_config(deps: Deps<NeutronQuery>) -> StdResult<ConfigResponse> {
    Ok(ConfigResponse {
        config: CONFIG.load(deps.storage)?,
    })
}

fn query_interchain_query_info(deps: Deps<NeutronQuery>) -> StdResult<InterchainQueryInfoResponse> {
    Ok(InterchainQueryInfoResponse {
        info: INTERCHAIN_QUERY_INFO.may_load(deps.storage)?,
    })
}

fn query_denom_info(deps: Deps<NeutronQuery>, round_id: u64) -> StdResult<DenomInfoResponse> {
    let config = CONFIG.load(deps.storage)?;

    Ok(DenomInfoResponse {
        denom: config.st_token_denom,
        token_group_id: config.token_group_id,
        ratio: find_latest_known_token_ratio(deps.storage, round_id)?,
    })
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(
    deps: DepsMut<NeutronQuery>,
    _env: Env,
    msg: Reply,
) -> Result<Response<NeutronMsg>, ContractError> {
    match from_json::<ReplyPayload>(&msg.payload)? {
        ReplyPayload::RegisterHostZoneICQ { creator, funds } => {
            // TODO: Replace with utility function defined in LSM PR
            let register_query_resp: MsgRegisterInterchainQueryResponse = decode_message_response(
                &msg.result
                    .into_result()
                    .map_err(StdError::generic_err)?
                    .msg_responses[0]
                    .clone()
                    .value
                    .to_vec(),
            )
            .map_err(|e| StdError::generic_err(format!("failed to parse reply message: {e:?}")))?;

            INTERCHAIN_QUERY_INFO.save(
                deps.storage,
                &InterchainQueryInfo {
                    creator: creator.clone(),
                    query_id: register_query_resp.id,
                    deposit_paid: funds,
                },
            )?;

            Ok(Response::new()
                .add_attribute("action", "register_host_zone_icq_reply")
                .add_attribute("creator", creator)
                .add_attribute("query_id", register_query_resp.id.to_string()))
        }
        ReplyPayload::RemoveHostZoneICQ { query_id } => {
            // TODO: Replace with utility function defined in LSM PR
            decode_message_response::<MsgRemoveInterchainQueryResponse>(
                &msg.result
                    .into_result()
                    .map_err(StdError::generic_err)?
                    .msg_responses[0]
                    .clone()
                    .value
                    .to_vec(),
            )
            .map_err(|e| StdError::generic_err(format!("failed to parse reply message: {e:?}")))?;

            INTERCHAIN_QUERY_INFO.remove(deps.storage);

            Ok(Response::new()
                .add_attribute("action", "remove_host_zone_icq_reply")
                .add_attribute("query_id", query_id.to_string()))
        }
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn sudo(
    deps: DepsMut<NeutronQuery>,
    _env: Env,
    msg: SudoMsg,
) -> Result<Response<NeutronMsg>, ContractError> {
    match msg {
        SudoMsg::KVQueryResult { query_id } => {
            handle_delivered_interchain_query_result(deps, query_id)
        }
        _ => Err(ContractError::Std(StdError::generic_err(
            "Unexpected sudo message received",
        ))),
    }
}

pub fn handle_delivered_interchain_query_result(
    deps: DepsMut<NeutronQuery>,
    query_id: u64,
) -> Result<Response<NeutronMsg>, ContractError> {
    let config = &CONFIG.load(deps.storage)?;
    let current_round = query_current_round_id(&deps.as_ref(), &config.hydro_contract_address)?;

    // Make sure that the correct old token ratio value is used. If the ratio is being updated for the first time in
    // the current round, then the old token ratio should be copied from the latest round for which the ratio is known,
    // instead of using 0. Using wrong old value would report inaccurate token ratio updates to the main Hydro contract.
    initialize_token_ratio(deps.storage, current_round)?;

    let old_ratio = TOKEN_RATIO.load(deps.storage, current_round)?;

    let registered_query = get_registered_query(deps.as_ref(), query_id)?;
    check_query_type(registered_query.registered_query.query_type, QueryType::KV)?;
    let host_zone: HostZone = query_kv_result(deps.as_ref(), query_id)?;

    // Double check that the host zone chain ID matches the expected one.
    if host_zone.chain_id != config.stride_host_zone_id {
        return Err(ContractError::Std(StdError::generic_err(format!(
            "Unexpected host zone chain ID in the query result. Expected: {}, got: {}",
            config.stride_host_zone_id, host_zone.chain_id
        ))));
    }

    let mut submsgs = vec![];
    let new_ratio =
        Decimal::from_ratio(Uint128::from_str(&host_zone.redemption_rate)?, DENOMINATOR);

    if old_ratio != new_ratio {
        TOKEN_RATIO.save(deps.storage, current_round, &new_ratio)?;

        let update_token_ratio_msg = HydroExecuteMsg::UpdateTokenGroupRatio {
            token_group_id: config.token_group_id.clone(),
            old_ratio,
            new_ratio,
        };

        let wasm_execute_msg = WasmMsg::Execute {
            contract_addr: config.hydro_contract_address.to_string(),
            msg: to_json_binary(&update_token_ratio_msg)?,
            funds: vec![],
        };

        submsgs.push(SubMsg::reply_never(wasm_execute_msg));
    }

    Ok(Response::new()
        .add_submessages(submsgs)
        .add_attribute("action", "handle_delivered_interchain_query_result")
        .add_attribute("query_id", query_id.to_string())
        .add_attribute("host_zone_id", host_zone.chain_id)
        .add_attribute("old_ratio", old_ratio.to_string())
        .add_attribute("new_ratio", new_ratio.to_string()))
}

// Finds the latest known token ratio by going backwards from the given start round until it finds a round
// that has the token ratio initialized. This is useful for our token information provider API in case when
// a new round starts, so that we don't have to initialize the new round data immediately, but without stoping
// our users from locking their tokens in the new round. Once the ICQ result is delivered, it will first copy
// the same old value from the last known round, and then update it with the new value extracted from ICQ result,
// so there is no risk of using different ratios for the same round in different contexts.
pub fn find_latest_known_token_ratio(
    storage: &dyn Storage,
    start_round: u64,
) -> StdResult<Decimal> {
    let mut round = start_round;
    while !is_token_ratio_initialized(storage, round) {
        if round == 0 {
            return Err(StdError::generic_err(
                "first round must be initialized during contract instantiation",
            ));
        }
        round -= 1;
    }

    TOKEN_RATIO.load(storage, round)
}

// Initializes the token ratio for all rounds up to the current round. Starts from the current round
// and goes backwards until it finds the round that has the token ratio initialized.
pub fn initialize_token_ratio(storage: &mut dyn Storage, current_round: u64) -> StdResult<()> {
    let mut round = current_round;
    while !is_token_ratio_initialized(storage, round) {
        if round == 0 {
            return Err(StdError::generic_err(
                "first round must be initialized during contract instantiation",
            ));
        }
        round -= 1;
    }

    let last_known_ratio = TOKEN_RATIO.load(storage, round)?;

    while round < current_round {
        round += 1;
        TOKEN_RATIO.save(storage, round, &last_known_ratio)?;
    }

    Ok(())
}

pub fn is_token_ratio_initialized(storage: &dyn Storage, round_id: u64) -> bool {
    TOKEN_RATIO.has(storage, round_id)
}

pub fn query_current_round_id(
    deps: &Deps<NeutronQuery>,
    hydro_contract: &Addr,
) -> Result<u64, ContractError> {
    let current_round_resp: HydroCurrentRoundResponse = deps
        .querier
        .query_wasm_smart(hydro_contract, &HydroQueryMsg::CurrentRound {})?;

    Ok(current_round_resp.round_id)
}

// Stride HostZone contains information about stTOKEN:TOKEN ratio (redemption rate). This struct on Stride chain has
// many more fields, but we only need the chain ID and redemption rate for our purposes. The ICQ result will contain
// the full HostZone data, but due to the way Protobuf works, we can reconstruct only these two fields. Defining the
// struct manually saves us from having to keep the Stride protobuf definitions and having to maintain code that
// would compile them into Rust data structures.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct HostZone {
    #[prost(string, tag = "1")]
    pub chain_id: ::prost::alloc::string::String,
    #[prost(string, tag = "11")]
    pub redemption_rate: ::prost::alloc::string::String,
}

impl KVReconstruct for HostZone {
    fn reconstruct(storage_values: &[StorageValue]) -> NeutronResult<Self> {
        let kv = storage_values
            // We always query only one HostZone at the time
            .first()
            .expect("HostZone not found");

        let host_zone = HostZone::decode(kv.value.as_slice())?;

        Ok(host_zone)
    }
}

#[derive(Serialize, Deserialize)]
pub enum ReplyPayload {
    RegisterHostZoneICQ { creator: String, funds: Vec<Coin> },
    RemoveHostZoneICQ { query_id: u64 },
}
