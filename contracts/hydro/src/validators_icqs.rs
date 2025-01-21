use std::str::FromStr;

use cosmwasm_std::{
    from_json, to_json_vec, Coin, Decimal, Deps, DepsMut, Env, Order, Reply, Response, StdError,
    StdResult, SubMsg, Uint128,
};

use neutron_sdk::{
    bindings::{msg::NeutronMsg, query::NeutronQuery},
    interchain_queries::v047::{queries::query_staking_validators, types::Validator},
    interchain_txs::helpers::decode_message_response,
    proto_types::neutron::interchainqueries::{
        MsgRegisterInterchainQueryResponse, MsgRemoveInterchainQueryResponse,
    },
    NeutronError,
};
use neutron_std::types::neutron::interchainqueries::InterchainqueriesQuerier;
use serde::{Deserialize, Serialize};

use crate::{
    contract::{compute_current_round_id, NATIVE_TOKEN_DENOM},
    error::ContractError,
    lsm_integration::update_stores_due_to_power_ratio_change,
    state::{
        Constants, ValidatorInfo, QUERY_ID_TO_VALIDATOR, VALIDATORS_INFO, VALIDATORS_PER_ROUND,
        VALIDATOR_TO_QUERY_ID,
    },
    utils::{load_current_constants, run_on_each_transaction},
};

// A multiplier to normalize shares, such that when a validator has just been created
// and was never slashed, 1 Token = Shares / TOKEN_TO_SHARES_MULTIPLIER.
pub const TOKENS_TO_SHARES_MULTIPLIER: Uint128 = Uint128::new(1_000_000_000_000_000_000);

// SubMsg ID is used so that we can differentiate submessages sent by the smart contract when the Wasm SDK module
// calls back the reply() function on the smart contract. Since we are using the payload to populate all the data
// that we need when reply() is called, we don't need to set a unique SubMsg ID and can use 0 for all SubMsgs.
const UNUSED_MSG_ID: u64 = 0;

#[derive(Serialize, Deserialize)]
pub enum ReplyPayload {
    CreateValidatorICQ(String),
    RemoveValidatorICQ(u64),
}

pub fn build_create_interchain_query_submsg(
    msg: NeutronMsg,
    validator_address: String,
) -> StdResult<SubMsg<NeutronMsg>> {
    Ok(
        SubMsg::reply_on_success(msg, UNUSED_MSG_ID).with_payload(to_json_vec(
            &ReplyPayload::CreateValidatorICQ(validator_address),
        )?),
    )
}

fn build_remove_interchain_query_submsg(query_id: u64) -> StdResult<SubMsg<NeutronMsg>> {
    Ok(
        SubMsg::reply_on_success(NeutronMsg::remove_interchain_query(query_id), UNUSED_MSG_ID)
            .with_payload(to_json_vec(&ReplyPayload::RemoveValidatorICQ(query_id))?),
    )
}

pub fn handle_submsg_reply(
    deps: DepsMut<NeutronQuery>,
    msg: Reply,
) -> Result<Response<NeutronMsg>, ContractError> {
    // No need to use msg.id to determine what to do, since we can extract everything we need from the msg.payload.
    let reply_paylod = from_json(&msg.payload)?;
    match reply_paylod {
        ReplyPayload::CreateValidatorICQ(validator_address) => {
            let register_query_resp: MsgRegisterInterchainQueryResponse = decode_message_response(
                &msg.result
                    .into_result()
                    .map_err(StdError::generic_err)?
                    .msg_responses[0]
                    .clone()
                    .value
                    .to_vec(),
            )
            .map_err(|e| {
                StdError::generic_err(format!("failed to parse reply message: {:?}", e))
            })?;

            QUERY_ID_TO_VALIDATOR.save(deps.storage, register_query_resp.id, &validator_address)?;
            VALIDATOR_TO_QUERY_ID.save(deps.storage, validator_address, &register_query_resp.id)?;
        }
        ReplyPayload::RemoveValidatorICQ(query_id) => {
            // just validate that we received the response type that we expected
            decode_message_response::<MsgRemoveInterchainQueryResponse>(
                &msg.result
                    .into_result()
                    .map_err(StdError::generic_err)?
                    .msg_responses[0]
                    .clone()
                    .value
                    .to_vec(),
            )
            .map_err(|e| {
                StdError::generic_err(format!("failed to parse reply message: {:?}", e))
            })?;

            let validator_address = QUERY_ID_TO_VALIDATOR.load(deps.storage, query_id)?;
            QUERY_ID_TO_VALIDATOR.remove(deps.storage, query_id);
            VALIDATOR_TO_QUERY_ID.remove(deps.storage, validator_address);
        }
    }

    Ok(Response::default())
}

pub fn handle_delivered_interchain_query_result(
    mut deps: DepsMut<NeutronQuery>,
    env: Env,
    query_id: u64,
) -> Result<Response<NeutronMsg>, ContractError> {
    let validator = match get_interchain_query_result(deps.as_ref(), env.clone(), query_id) {
        Ok(validator) => validator,
        Err(_) => {
            return Ok(
                Response::default().add_submessage(build_remove_interchain_query_submsg(query_id)?)
            );
        }
    };
    let constants = load_current_constants(&deps.as_ref(), &env)?;
    let current_round = compute_current_round_id(&env, &constants)?;
    run_on_each_transaction(deps.storage, &env, current_round)?;

    let validator_address = validator.operator_address.clone();
    let new_tokens = Uint128::from_str(&validator.tokens)?;
    let new_shares = Uint128::from_str(&validator.delegator_shares)?;
    let new_power_ratio = Decimal::from_ratio(new_tokens * TOKENS_TO_SHARES_MULTIPLIER, new_shares);

    let mut submsgs = vec![];

    let current_validator_info =
        VALIDATORS_INFO.may_load(deps.storage, (current_round, validator_address.clone()))?;
    match current_validator_info {
        // If the validator_info is found, it means that it is among the top N for this round.
        // We just need to update its rank and power ratio, if they changed in the meantime.
        Some(validator_info) => {
            top_n_validator_update(
                &mut deps,
                &env,
                current_round,
                validator_info,
                new_tokens,
                new_power_ratio,
            )?;
        }
        // Use-cases:
        // 1) ICQ results were submitted for a brand new validator that wasn't earlier in the top N
        // 2) At the begining of a new round, we start receiving ICQ results for validators from previous round
        None => {
            let validator_info = ValidatorInfo::new(validator_address, new_tokens, new_power_ratio);
            match get_last_validator(&mut deps, current_round, &constants) {
                None => {
                    // if there are currently less than top N validators, add this one to the top N
                    top_n_validator_add(&mut deps, &env, current_round, validator_info)?;
                }
                Some(last_validator) => {
                    // there are top N validators already, so check if the new one has more
                    // delegated tokens than the one with the least tokens among the top N
                    let other_validator_tokens = Uint128::new(last_validator.0);
                    if validator_info.delegated_tokens > other_validator_tokens {
                        let other_validator_info = VALIDATORS_INFO
                            .load(deps.storage, (current_round, last_validator.1.clone()))?;

                        top_n_validator_remove(
                            &mut deps,
                            &env,
                            current_round,
                            other_validator_info,
                        )?;
                        top_n_validator_add(&mut deps, &env, current_round, validator_info)?;

                        // remove ICQ of the validator that was dropped from the top N
                        let last_validator_query_id =
                            VALIDATOR_TO_QUERY_ID.load(deps.storage, last_validator.1.clone())?;
                        submsgs.push(build_remove_interchain_query_submsg(
                            last_validator_query_id,
                        )?);
                    } else {
                        // remove ICQ for this validator since it is not in the top N
                        submsgs.push(build_remove_interchain_query_submsg(query_id)?);
                    }
                }
            }
        }
    };

    Ok(Response::default().add_submessages(submsgs))
}

fn top_n_validator_add(
    deps: &mut DepsMut<NeutronQuery>,
    env: &Env,
    current_round: u64,
    validator_info: ValidatorInfo,
) -> Result<(), NeutronError> {
    // this call only makes difference if some validator was in the top N,
    // then was droped out, and then got back in the top N again
    update_stores_due_to_power_ratio_change(
        deps.storage,
        env.block.height,
        &validator_info.address.clone(),
        current_round,
        Decimal::zero(),
        validator_info.power_ratio,
    )?;
    VALIDATORS_INFO.save(
        deps.storage,
        (current_round, validator_info.address.clone()),
        &validator_info,
    )?;
    VALIDATORS_PER_ROUND.save(
        deps.storage,
        (
            current_round,
            validator_info.delegated_tokens.u128(),
            validator_info.address.clone(),
        ),
        &validator_info.address,
    )?;
    Ok(())
}

fn top_n_validator_update(
    deps: &mut DepsMut<NeutronQuery>,
    env: &Env,
    current_round: u64,
    mut validator_info: ValidatorInfo,
    new_tokens: Uint128,
    new_power_ratio: Decimal,
) -> Result<(), NeutronError> {
    let mut should_update_info = false;
    if validator_info.delegated_tokens != new_tokens {
        VALIDATORS_PER_ROUND.remove(
            deps.storage,
            (
                current_round,
                validator_info.delegated_tokens.u128(),
                validator_info.address.clone(),
            ),
        );
        VALIDATORS_PER_ROUND.save(
            deps.storage,
            (
                current_round,
                new_tokens.u128(),
                validator_info.address.clone(),
            ),
            &validator_info.address,
        )?;

        validator_info.delegated_tokens = new_tokens;
        should_update_info = true;
    }

    if validator_info.power_ratio != new_power_ratio {
        update_stores_due_to_power_ratio_change(
            deps.storage,
            env.block.height,
            &validator_info.address.clone(),
            current_round,
            validator_info.power_ratio,
            new_power_ratio,
        )?;

        validator_info.power_ratio = new_power_ratio;
        should_update_info = true;
    }

    if should_update_info {
        VALIDATORS_INFO.save(
            deps.storage,
            (current_round, validator_info.address.clone()),
            &validator_info,
        )?;
    }

    Ok(())
}

fn top_n_validator_remove(
    deps: &mut DepsMut<NeutronQuery>,
    env: &Env,
    current_round: u64,
    validator_info: ValidatorInfo,
) -> Result<(), NeutronError> {
    update_stores_due_to_power_ratio_change(
        deps.storage,
        env.block.height,
        &validator_info.address.clone(),
        current_round,
        validator_info.power_ratio,
        Decimal::zero(),
    )?;

    VALIDATORS_INFO.remove(
        deps.storage,
        (current_round, validator_info.address.clone()),
    );
    VALIDATORS_PER_ROUND.remove(
        deps.storage,
        (
            current_round,
            validator_info.delegated_tokens.u128(),
            validator_info.address.clone(),
        ),
    );

    Ok(())
}

fn get_last_validator(
    deps: &mut DepsMut<NeutronQuery>,
    current_round: u64,
    constants: &Constants,
) -> Option<(u128, String)> {
    let last_validator: Vec<(u128, String)> = VALIDATORS_PER_ROUND
        .sub_prefix(current_round)
        .range(deps.storage, None, None, Order::Descending)
        .skip((constants.max_validator_shares_participating - 1) as usize)
        .filter(|f| {
            let ok = f.is_ok();
            if !ok {
                // log an error
                deps.api.debug(&format!(
                    "failed to obtain validator info: {}",
                    f.as_ref().err().unwrap()
                ));
            }
            ok
        })
        .take(1)
        .map(|f| f.unwrap().0)
        .collect();

    match last_validator.len() {
        0 => None,
        _ => Some(last_validator[0].clone()),
    }
}

fn get_interchain_query_result(
    deps: Deps<NeutronQuery>,
    env: Env,
    query_id: u64,
) -> Result<Validator, NeutronError> {
    let staking_validator = query_staking_validators(deps, env, query_id)?.validator;

    // Our interchain queries will always have exactly one validator. Everything else is invalid.
    // If the validator with the given address wasn't found, query_staking_validators() will return
    // a Validator instance with all fields initialized with default values. We should error in this
    // case and not try to parse tokens and delegator shares from uninitialized values.
    if staking_validator.validators.len() != 1
        || staking_validator.validators[0].operator_address.is_empty()
    {
        return Err(NeutronError::Std(StdError::generic_err(format!(
            "failed to obtain validator info from interchain query with id: {}",
            query_id
        ))));
    }

    Ok(staking_validator.validators[0].clone())
}

pub fn query_min_interchain_query_deposit(deps: &Deps<NeutronQuery>) -> StdResult<Coin> {
    match InterchainqueriesQuerier::new(&deps.querier)
        .params()?
        .params
    {
        Some(params) => {
            match params
                .query_deposit
                .iter()
                .find(|coin| coin.denom.eq(NATIVE_TOKEN_DENOM))
            {
                None => Err(StdError::generic_err(
                    "Failed to obtain interchain query creation deposit.",
                )),
                Some(coin) => Ok(Coin::new(
                    Uint128::from_str(coin.amount.as_str())?,
                    coin.denom.clone(),
                )),
            }
        }
        None => Err(StdError::generic_err(
            "Failed to obtain interchain query creation deposit.",
        )),
    }
}
