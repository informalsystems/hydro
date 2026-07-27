use std::collections::HashSet;
use std::str::FromStr;

use cosmos_sdk_proto::cosmos::staking::v1beta1::{
    QueryValidatorRequest, QueryValidatorResponse, Validator,
};
use cosmwasm_std::{
    to_json_binary, Addr, Attribute, Binary, Decimal, Deps, DepsMut, Order, Response, StdError,
    StdResult, Uint128, WasmMsg,
};
use interface::{
    hydro::{ExecuteMsg as HydroExecuteMsg, TokenGroupRatioChange},
    lsm::{ValidatorInfo, TOKENS_TO_SHARES_MULTIPLIER},
};
use prost::Message;

use crate::{
    error::ContractError,
    msg::ExecuteContext,
    state::{VALIDATORS_INFO, VALIDATORS_PER_ROUND},
    utils::COSMOS_VALIDATOR_PREFIX,
};

const STAKING_VALIDATOR_GRPC: &str = "/cosmos.staking.v1beta1.Query/Validator";

pub fn update_validators_ratios(
    mut deps: DepsMut,
    validators: Vec<String>,
    context: ExecuteContext,
) -> Result<Response, ContractError> {
    let valid_addresses = validate_and_deduplicate(&validators);

    let mut attributes = vec![Attribute::new("action", "update_validators_ratios")];
    let mut token_groups_ratios_changes: Vec<TokenGroupRatioChange> = vec![];

    for validator_address in valid_addresses {
        match query_staking_validator(&deps.as_ref(), validator_address.clone()) {
            Ok(validator) => {
                let new_tokens = Uint128::from_str(&validator.tokens).map_err(|e| {
                    StdError::generic_err(format!("Failed to parse validator tokens: {e}"))
                })?;
                let new_shares = Uint128::from_str(&validator.delegator_shares).map_err(|e| {
                    StdError::generic_err(format!(
                        "Failed to parse validator delegator_shares: {e}"
                    ))
                })?;

                // Treat a validator with zero tokens or shares the same as a removed validator.
                if new_tokens.is_zero() || new_shares.is_zero() {
                    if let Some(change) = handle_validator_removal(
                        &mut deps,
                        context.current_round_id,
                        validator_address.clone(),
                    )? {
                        token_groups_ratios_changes.push(change);
                    }
                    continue;
                }

                let new_power_ratio =
                    Decimal::from_ratio(new_tokens * TOKENS_TO_SHARES_MULTIPLIER, new_shares);

                let current_validator_info = VALIDATORS_INFO.may_load(
                    deps.storage,
                    (context.current_round_id, validator_address.clone()),
                )?;

                match current_validator_info {
                    Some(current_validator_info) => {
                        // Validator is already in top N- update tokens and ratio if they changed.

                        if let Some(change) = top_n_validator_update(
                            &mut deps,
                            context.current_round_id,
                            current_validator_info.clone(),
                            new_tokens,
                            new_power_ratio,
                        )? {
                            token_groups_ratios_changes.push(change);
                        }

                        let attr_key = format!("validator_updated_{}", validator_address);
                        let attr_val = format!(
                            "tokens: {} -> {}, power_ratio: {} -> {}",
                            current_validator_info.delegated_tokens,
                            new_tokens,
                            current_validator_info.power_ratio,
                            new_power_ratio
                        );

                        attributes.push(Attribute::new(attr_key, &attr_val));
                    }
                    None => {
                        // Validator not in top N yet - check if it should be added.
                        let validator_info = ValidatorInfo::new(
                            validator_address.clone(),
                            new_tokens,
                            new_power_ratio,
                        );

                        match get_last_validator(
                            &mut deps,
                            context.current_round_id,
                            context.config.max_validator_shares_participating,
                        ) {
                            None => {
                                // Less than the max validators currently tracked - add the new validator.
                                token_groups_ratios_changes.push(top_n_validator_add(
                                    &mut deps,
                                    context.current_round_id,
                                    validator_info.clone(),
                                )?);

                                let attr_key = format!("validator_added_{}", validator_address);
                                let attr_val = format!(
                                    "tokens: {}, power_ratio: {}",
                                    validator_info.delegated_tokens, validator_info.power_ratio,
                                );
                                attributes.push(Attribute::new(attr_key, &attr_val));
                            }
                            Some(last_validator) => {
                                // Top N is reached. Remove the validator with the fewest tokens
                                // if the new validator has more tokens, and add the new one.
                                let other_validator_tokens = Uint128::new(last_validator.0);

                                if validator_info.delegated_tokens <= other_validator_tokens {
                                    // New validator does not have more tokens than the last one- skip it.
                                    continue;
                                }

                                let other_validator_info = VALIDATORS_INFO.load(
                                    deps.storage,
                                    (context.current_round_id, last_validator.1.clone()),
                                )?;

                                token_groups_ratios_changes.push(top_n_validator_remove(
                                    &mut deps,
                                    context.current_round_id,
                                    other_validator_info.clone(),
                                )?);

                                token_groups_ratios_changes.push(top_n_validator_add(
                                    &mut deps,
                                    context.current_round_id,
                                    validator_info.clone(),
                                )?);

                                let attr_key =
                                    format!("validator_added_{}", validator_info.address);
                                let attr_val = format!(
                                    "tokens: {}, power_ratio: {}",
                                    validator_info.delegated_tokens, validator_info.power_ratio,
                                );
                                attributes.push(Attribute::new(attr_key, &attr_val));

                                let attr_key =
                                    format!("validator_removed_{}", other_validator_info.address);
                                let attr_val = format!(
                                    "tokens: {}, power_ratio: {}",
                                    other_validator_info.delegated_tokens,
                                    other_validator_info.power_ratio,
                                );
                                attributes.push(Attribute::new(attr_key, &attr_val));
                            }
                        }
                    }
                }
            }
            Err(_) => {
                // Validator does not exist on chain or the query failed. Two possible scenarios:
                //     1) Validator address is valid, but the validator with the given address never existed.
                //     2) Validator was removed from the chain - treat as zero tokens and remove it.
                if let Some(change) = handle_validator_removal(
                    &mut deps,
                    context.current_round_id,
                    validator_address.clone(),
                )? {
                    token_groups_ratios_changes.push(change.clone());

                    let attr_key = format!("validator_removed_{}", validator_address);
                    let attr_val = format!("last_known_power_ratio: {}", change.old_ratio,);
                    attributes.push(Attribute::new(attr_key, &attr_val));
                }
            }
        }
    }

    let mut response = Response::new().add_attributes(attributes);

    if !token_groups_ratios_changes.is_empty() {
        response = response.add_message(build_token_groups_ratios_update_msg(
            &context.config.hydro_contract_address,
            token_groups_ratios_changes,
        )?);
    }

    Ok(response)
}

fn validate_and_deduplicate(validators: &[String]) -> HashSet<String> {
    validators
        .iter()
        .map(|v| v.trim().to_owned())
        .filter(|v| {
            bech32::decode(v)
                .map(|(hrp, _)| hrp.as_str() == COSMOS_VALIDATOR_PREFIX)
                .unwrap_or(false)
        })
        .collect()
}

fn query_staking_validator(deps: &Deps, validator_addr: String) -> StdResult<Validator> {
    let request = QueryValidatorRequest { validator_addr };
    let result = deps
        .querier
        .query_grpc(
            STAKING_VALIDATOR_GRPC.to_owned(),
            Binary::new(request.encode_to_vec()),
        )
        .map_err(|e| {
            StdError::generic_err(format!(
                "Failed to query validator from staking module: {e}"
            ))
        })?;

    QueryValidatorResponse::decode(result.as_slice())
        .map_err(|_| StdError::generic_err("Failed to decode QueryValidatorResponse"))?
        .validator
        .ok_or_else(|| StdError::generic_err("Validator not found in staking module"))
}

// Removes a validator from top-N state if it is present. Returns the ratio change if removed.
fn handle_validator_removal(
    deps: &mut DepsMut,
    current_round: u64,
    validator_address: String,
) -> Result<Option<TokenGroupRatioChange>, ContractError> {
    if let Some(validator_info) =
        VALIDATORS_INFO.may_load(deps.storage, (current_round, validator_address))?
    {
        return Ok(Some(top_n_validator_remove(
            deps,
            current_round,
            validator_info,
        )?));
    }

    Ok(None)
}

fn top_n_validator_add(
    deps: &mut DepsMut,
    current_round: u64,
    validator_info: ValidatorInfo,
) -> Result<TokenGroupRatioChange, ContractError> {
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

    Ok(TokenGroupRatioChange {
        token_group_id: validator_info.address,
        old_ratio: Decimal::zero(),
        new_ratio: validator_info.power_ratio,
    })
}

fn top_n_validator_update(
    deps: &mut DepsMut,
    current_round: u64,
    mut current_validator_info: ValidatorInfo,
    new_tokens: Uint128,
    new_power_ratio: Decimal,
) -> Result<Option<TokenGroupRatioChange>, ContractError> {
    let mut should_update_info = false;

    if current_validator_info.delegated_tokens != new_tokens {
        VALIDATORS_PER_ROUND.remove(
            deps.storage,
            (
                current_round,
                current_validator_info.delegated_tokens.u128(),
                current_validator_info.address.clone(),
            ),
        );
        VALIDATORS_PER_ROUND.save(
            deps.storage,
            (
                current_round,
                new_tokens.u128(),
                current_validator_info.address.clone(),
            ),
            &current_validator_info.address,
        )?;

        current_validator_info.delegated_tokens = new_tokens;
        should_update_info = true;
    }

    let mut token_group_ratio_change = None;

    if current_validator_info.power_ratio != new_power_ratio {
        token_group_ratio_change = Some(TokenGroupRatioChange {
            token_group_id: current_validator_info.address.clone(),
            old_ratio: current_validator_info.power_ratio,
            new_ratio: new_power_ratio,
        });

        current_validator_info.power_ratio = new_power_ratio;
        should_update_info = true;
    }

    if should_update_info {
        VALIDATORS_INFO.save(
            deps.storage,
            (current_round, current_validator_info.address.clone()),
            &current_validator_info,
        )?;
    }

    Ok(token_group_ratio_change)
}

fn top_n_validator_remove(
    deps: &mut DepsMut,
    current_round: u64,
    validator_info: ValidatorInfo,
) -> Result<TokenGroupRatioChange, ContractError> {
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

    Ok(TokenGroupRatioChange {
        token_group_id: validator_info.address.clone(),
        old_ratio: validator_info.power_ratio,
        new_ratio: Decimal::zero(),
    })
}

// Returns the validator with the fewest delegated tokens among the top N, or None if fewer than
// max_validator_shares_participating validators are currently tracked.
pub fn get_last_validator(
    deps: &mut DepsMut,
    current_round: u64,
    max_validator_shares_participating: u64,
) -> Option<(u128, String)> {
    let last_validator: Vec<(u128, String)> = VALIDATORS_PER_ROUND
        .sub_prefix(current_round)
        .range(deps.storage, None, None, Order::Descending)
        .skip((max_validator_shares_participating - 1) as usize)
        .filter(|f| {
            let ok = f.is_ok();
            if !ok {
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

fn build_token_groups_ratios_update_msg(
    hydro_contract: &Addr,
    changes: Vec<TokenGroupRatioChange>,
) -> Result<WasmMsg, ContractError> {
    let msg = HydroExecuteMsg::UpdateTokenGroupsRatios { changes };
    Ok(WasmMsg::Execute {
        contract_addr: hydro_contract.to_string(),
        msg: to_json_binary(&msg)?,
        funds: vec![],
    })
}
