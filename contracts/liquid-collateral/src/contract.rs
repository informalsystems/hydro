use crate::calculations::{price_to_tick, tick_to_sqrt_price};
use crate::error::ContractError;
use crate::msg::{
    CalculatedDataResponse, CreatePositionMsg, EndRoundBidMsg, ExecuteMsg, InstantiateMsg,
    QueryMsg, StateResponse,
};
use crate::state::{Bid, BidStatus, State, BIDS, SORTED_BIDS, STATE};
use bigdecimal::BigDecimal;
use cosmwasm_std::{
    entry_point, to_json_binary, Addr, BankMsg, Binary, Coin, Decimal, Deps, DepsMut, Env, Event,
    MessageInfo, Order, Reply, Response, StdError, StdResult, SubMsg, Uint128,
};
use osmosis_std::types::cosmos::base::v1beta1::Coin as OsmosisCoin;
use osmosis_std::types::osmosis::concentratedliquidity::v1beta1::{
    ClaimableIncentivesResponse, ConcentratedliquidityQuerier,
};
use osmosis_std::types::osmosis::concentratedliquidity::v1beta1::{
    MsgCreatePosition, MsgCreatePositionResponse, MsgWithdrawPosition, MsgWithdrawPositionResponse,
};
use std::str::FromStr;

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let state = State {
        project_owner: match msg.project_owner {
            Some(owner_str) => Some(deps.api.addr_validate(&owner_str)?),
            None => None,
        },
        pool_id: msg.pool_id,
        position_created_address: None,
        position_id: None,
        principal_denom: msg.principal_denom,
        counterparty_denom: msg.counterparty_denom,
        initial_principal_amount: Uint128::zero(),
        initial_counterparty_amount: Uint128::zero(),
        liquidity_shares: None,
        liquidator_address: None,
        round_end_time: _env.block.time.plus_seconds(msg.round_duration),
        auction_duration: msg.auction_duration,
        auction_end_time: None,
        auction_principal_deposited: Uint128::zero(),
        principal_funds_owner: deps.api.addr_validate(&msg.principal_funds_owner)?,
        principal_to_replenish: Uint128::zero(),
        counterparty_to_give: None,
        position_rewards: None,
        principal_first: msg.principal_first,
    };
    STATE.save(deps.storage, &state)?;

    Ok(Response::new()
        .add_attribute("method", "instantiate")
        .add_attribute("owner", info.sender)
        .add_attribute("pool_id", msg.pool_id.to_string()))
}

#[entry_point]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::CreatePosition(msg) => create_position(deps, env, info, msg),
        ExecuteMsg::Liquidate => liquidate(deps, env, info),
        ExecuteMsg::EndRound => end_round(deps, env, info),
        ExecuteMsg::EndRoundBid(msg) => end_round_bid(deps, env, info, msg),
        ExecuteMsg::WithdrawBid => withdraw_bid(deps, env, info),
        ExecuteMsg::ResolveAuction => resolve_auction(deps, env, info),
    }
}

#[entry_point]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        // Handle the GetState query
        QueryMsg::State {} => to_json_binary(&query_get_state(deps)?),
        QueryMsg::Bid { address } => to_json_binary(&query_bid(deps, address)?),
        QueryMsg::Bids {} => to_json_binary(&query_get_bids(deps)?),
        QueryMsg::CounterpartyAndUpperTick {
            lower_tick,
            principal_token_amount,
            liquidation_bonus,
            price_ratio,
            tick_spacing,
        } => {
            // Call the query function with the fields directly
            to_json_binary(&calculate_optimal_counterparty_and_upper_tick(
                lower_tick,
                principal_token_amount,
                liquidation_bonus,
                price_ratio,
                tick_spacing,
            )?)
        }
        QueryMsg::SortedBids {} => to_json_binary(&query_sorted_bids(deps)?),
    }
}

pub fn calculate_optimal_counterparty_and_upper_tick(
    lower_tick: String,
    principal_token_amount: String,
    liquidation_bonus: String,
    price_ratio: String,
    tick_spacing: String,
) -> StdResult<Vec<CalculatedDataResponse>> {
    // inputs: lower tick, base token amount, and liquidation bonus
    // output:  upper tick and counterparty

    let price_ratio: f64 = match price_ratio.parse() {
        Ok(val) => val,
        Err(_) => return Err(StdError::generic_err("Failed to parse price_ratio to f64")),
    };

    let tick_spacing: i64 = match tick_spacing.parse() {
        Ok(val) => val,
        Err(_) => return Err(StdError::generic_err("Failed to parse tick_spacing to i64")),
    };

    // Calculate the square root of the ratio (current price)
    let sqrt_current = price_ratio.sqrt();
    let lower_tick: i64 = match lower_tick.parse() {
        Ok(val) => val,
        Err(_) => return Err(StdError::generic_err("Failed to parse lower_tick to i64")),
    };
    // Validate lower_tick alignment with tick spacing
    if lower_tick.abs() % tick_spacing != 0 {
        return Err(StdError::generic_err(format!(
            "lower_tick must be aligned with tick spacing: {}",
            tick_spacing
        )));
    }

    let sqrt_lower = tick_to_sqrt_price(lower_tick);
    let sqrt_lower = match sqrt_lower {
        Ok(value) => value.to_string().parse::<f64>().unwrap_or(f64::NAN),
        Err(_) => f64::NAN,
    }; // Convert lower_tick to f64 and take the square root

    let principal_token_amount: f64 = match principal_token_amount.parse() {
        Ok(val) => val,
        Err(_) => {
            return Err(StdError::generic_err(
                "Failed to parse principal_token_amount to f64",
            ))
        }
    };

    let liquidation_bonus: f64 = match liquidation_bonus.parse() {
        Ok(val) => val,
        Err(_) => {
            return Err(StdError::generic_err(
                "Failed to parse liquidation_bonus to f64",
            ))
        }
    };

    if sqrt_lower >= sqrt_current {
        return Err(StdError::generic_err(format!(
            "Invalid lower_tick: its corresponding price ({}) must be lower than the current price ({})",
            sqrt_lower * sqrt_lower,
            sqrt_current * sqrt_current
        )));
    }

    // Step 1: Adjust liquidity based on liquidation bonus
    let bonus_amount = principal_token_amount * liquidation_bonus;
    let adjusted_base_token_amount = principal_token_amount + bonus_amount;
    let adjusted_liquidity = adjusted_base_token_amount / (sqrt_current - sqrt_lower);

    // Step 2: Calculate the upper tick based on adjusted liquidity

    let strategies = vec![("tight", 0.05), ("passive", 0.15), ("conservative", 0.30)];

    let mut results = vec![];

    for (label, range_percent) in strategies {
        let sqrt_upper = sqrt_current * (1.0 + range_percent);
        let upper_price = sqrt_upper * sqrt_upper;
        let upper_price_str = format!("{:.30}", upper_price); // preserve precision
        let upper_price_bd = BigDecimal::from_str(&upper_price_str)
            .expect("Failed to convert upper_price to BigDecimal");
        let mut upper_tick = match price_to_tick(&upper_price_bd) {
            Ok(tick) => tick,
            Err(e) => return Err(StdError::generic_err("Failed to parse upper_tick to i64")),
        };
        upper_tick = round_to_spacing(upper_tick, tick_spacing);

        // Step 3: Calculate counterparty amount (WOBBLE) based on liquidity

        let counterparty_amount = adjusted_liquidity * (1.0 / sqrt_current - 1.0 / sqrt_upper);

        results.push(CalculatedDataResponse {
            strategy: label.to_string(),
            upper_tick: upper_tick.to_string(),
            counterparty_amount: counterparty_amount.to_string(),
        });
    }

    Ok(results)
}

// Helper: round tick to spacing
pub fn round_to_spacing(tick: i64, spacing: i64) -> i64 {
    ((tick + spacing / 2) / spacing) * spacing
}

pub fn create_position(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: CreatePositionMsg,
) -> Result<Response, ContractError> {
    let mut state = STATE.load(deps.storage)?;

    // Check if the position_id already exists
    if state.position_id.is_some() {
        return Err(ContractError::PositionAlreadyExists {});
    }

    // Only owner can create position - if present
    match &state.project_owner {
        Some(owner) if info.sender != *owner => {
            return Err(ContractError::Unauthorized {});
        }
        _ => {}
    }

    // Fetch funds sent
    let counterparty = info
        .funds
        .iter()
        .find(|c| c.denom == state.counterparty_denom);
    let principal = info.funds.iter().find(|c| c.denom == state.principal_denom);

    let counterparty_amount = counterparty.unwrap().amount;
    let principal_amount = principal.unwrap().amount;

    /*
    The order of the tokens which will be put in tokens provided is very important
    on osmosis they check lexicographical order: https://github.com/osmosis-labs/osmosis/blob/main/x/concentrated-liquidity/types/msgs.go#L42C2-L44C3
    ibc/token should go before uosmo token for example
    if order is not correct - tx will fail!
     */

    let token_min_amount0;
    let token_min_amount1;

    if state.principal_first {
        token_min_amount0 = msg.principal_token_min_amount.to_string();
        token_min_amount1 = msg.counterparty_token_min_amount.to_string();
    } else {
        token_min_amount0 = msg.counterparty_token_min_amount.to_string();
        token_min_amount1 = msg.principal_token_min_amount.to_string();
    }

    // Create position message
    let create_position_msg = MsgCreatePosition {
        pool_id: state.pool_id,
        sender: env.contract.address.to_string(),
        lower_tick: msg.lower_tick,
        upper_tick: msg.upper_tick,
        tokens_provided: vec![
            OsmosisCoin {
                denom: state.counterparty_denom.clone(),
                amount: counterparty_amount.to_string(),
            },
            OsmosisCoin {
                denom: state.principal_denom.clone(),
                amount: principal_amount.to_string(),
            },
        ],
        token_min_amount0: token_min_amount0,
        token_min_amount1: token_min_amount1,
    };

    // store the address which initiated position creation
    state.position_created_address = Some(info.sender);
    // Save the updated state back to storage
    STATE.save(deps.storage, &state)?;

    // Wrap in SubMsg to handle response
    let submsg = SubMsg::reply_on_success(create_position_msg, 1);

    Ok(Response::new()
        .add_submessage(submsg)
        .add_attribute("action", "create_position")
        .add_attribute("pool_id", state.pool_id.to_string())
        .add_attribute("lower_tick", msg.lower_tick.to_string())
        .add_attribute("upper_tick", msg.upper_tick.to_string()))
}

pub fn liquidate(deps: DepsMut, _env: Env, info: MessageInfo) -> Result<Response, ContractError> {
    let mut state = STATE.load(deps.storage)?;
    let position = ConcentratedliquidityQuerier::new(&deps.querier)
        .position_by_id(state.position_id.unwrap())
        .map_err(|_| ContractError::PositionQueryFailed {})? // Handle query errors
        .position
        .ok_or(ContractError::PositionNotFound {})?; // Return error if position is None

    // Conditionally extract only the principal asset based on `state.principal_first`
    let principal_asset = if state.principal_first {
        position.asset0 // principal is asset0
    } else {
        position.asset1 // principal is asset1
    };

    // Ensure the principal asset is valid
    let principal_asset = principal_asset.ok_or(ContractError::AssetNotFound {})?;

    // Use the principal asset amount
    let principal_amount = principal_asset.amount.clone();

    // Check if asset1_amount is non-zero
    // if it's zero - price went below lower tick (since principal token amount is zero)
    if principal_amount != "0" {
        return Err(ContractError::ThresholdNotMet {
            amount: principal_amount,
        });
    }

    // Validate funds sent
    let principal = info.funds.iter().find(|c| c.denom == state.principal_denom);

    if principal.is_none() {
        return Err(ContractError::InsufficientFunds {});
    }

    // Convert base_amount and initial_base_amount to Decimal for precise division
    let principal_amount = Decimal::from_atomics(principal.unwrap().amount, 0)
        .map_err(|_| ContractError::InvalidConversion {})?;
    let principal_amount_to_replenish = Decimal::from_atomics(state.principal_to_replenish, 0)
        .map_err(|_| ContractError::InvalidConversion {})?;

    let liquidity_shares = state
        .liquidity_shares
        .as_deref() // Converts Option<String> to Option<&str>
        .unwrap_or("0"); // Default value if None

    let liquidity_shares_decimal = parse_liquidity(&liquidity_shares)?;

    // Calculate the proportional liquidity amount to withdraw
    let withdraw_liquidity_amount = calculate_withdraw_liquidity_amount(
        principal_amount,
        principal_amount_to_replenish,
        liquidity_shares_decimal,
    )?;
    // substract liquidity and save again

    // Create withdraw message
    let withdraw_position_msg = MsgWithdrawPosition {
        position_id: state.position_id.unwrap(),
        sender: _env.contract.address.to_string(),
        liquidity_amount: withdraw_liquidity_amount.to_string(),
    };

    let liquidity_shares_uint = liquidity_shares_decimal.atomics();
    // Check if we're withdrawing the full liquidity
    let is_full_withdraw = withdraw_liquidity_amount == liquidity_shares_uint;
    if is_full_withdraw {
        // Query the claimable spread rewards
        let spread_rewards = ConcentratedliquidityQuerier::new(&deps.querier)
            .claimable_spread_rewards(state.pool_id)
            .map_err(|_| ContractError::ClaimableSpreadRewardsQueryFailed {})? // Handle query errors
            .claimable_spread_rewards;

        // Query the claimable incentives
        let incentives: ClaimableIncentivesResponse =
            ConcentratedliquidityQuerier::new(&deps.querier)
                .claimable_incentives(state.pool_id)
                .map_err(|_| ContractError::ClaimableSpreadRewardsQueryFailed {})?;

        // Save into state
        state.position_rewards = Some(
            fetch_all_rewards(
                spread_rewards,
                incentives.claimable_incentives,
                incentives.forfeited_incentives,
            )
            .unwrap_or_default(),
        );
    }

    // Wrap in SubMsg to handle response
    let submsg = SubMsg::reply_on_success(withdraw_position_msg, 2);

    state.liquidator_address = Some(info.sender);
    // update that liquidator replenished some principal amount
    state.principal_to_replenish -= principal.unwrap().amount;

    // Save the updated state back to storage
    STATE.save(deps.storage, &state)?;

    // immediately forward principal funds to principal owner
    let principal_msg = BankMsg::Send {
        to_address: state.principal_funds_owner.clone().into_string(),
        amount: vec![principal.unwrap().clone()],
    };

    Ok(Response::new()
        .add_submessage(submsg)
        .add_message(principal_msg)
        .add_attribute("action", "withdraw_position"))
}

pub fn fetch_all_rewards(
    spread_rewards: Vec<OsmosisCoin>,
    claimable_incentives: Vec<OsmosisCoin>,
    forfeited_incentives: Vec<OsmosisCoin>,
) -> Result<Vec<Coin>, ContractError> {
    let convert_coin = |c: OsmosisCoin| -> Result<Coin, ContractError> {
        Ok(Coin {
            denom: c.denom,
            amount: Uint128::from_str(&c.amount)
                .map_err(|_| ContractError::InvalidConversion {})?,
        })
    };

    let mut all_rewards: Vec<Coin> = Vec::with_capacity(
        spread_rewards.len() + claimable_incentives.len() + forfeited_incentives.len(),
    );

    for c in spread_rewards {
        all_rewards.push(convert_coin(c)?);
    }

    for c in claimable_incentives {
        all_rewards.push(convert_coin(c)?);
    }

    for c in forfeited_incentives {
        all_rewards.push(convert_coin(c)?);
    }

    Ok(all_rewards)
}

pub fn calculate_withdraw_liquidity_amount(
    principal_amount: Decimal,
    principal_amount_to_replenish: Decimal,
    liquidity_shares_decimal: Decimal,
) -> Result<Uint128, ContractError> {
    // Ensure the supplied amount is not greater than the initial amount
    if principal_amount > principal_amount_to_replenish {
        return Err(ContractError::ExcessiveLiquidationAmount {});
    }

    // Calculate percentage to liquidate
    let perc_to_liquidate = principal_amount / principal_amount_to_replenish;

    // Perform high-precision multiplication
    let withdraw_liquidity_amount = liquidity_shares_decimal * perc_to_liquidate;

    // Round to the nearest integer and convert back to Uint128
    let liquidity_amount = withdraw_liquidity_amount.atomics();

    Ok(liquidity_amount)
}

pub fn parse_liquidity(liq: &str) -> Result<Decimal, ContractError> {
    if liq.contains('.') {
        Decimal::from_str(liq).map_err(|_| ContractError::InvalidConversion {})
    } else {
        Decimal::from_atomics(
            Uint128::from_str(liq).map_err(|_| ContractError::InvalidConversion {})?,
            18,
        )
        .map_err(|_| ContractError::InvalidConversion {})
    }
}

#[entry_point]
pub fn reply(deps: DepsMut, _env: Env, msg: Reply) -> Result<Response, ContractError> {
    match msg.id {
        1 => handle_create_position_reply(deps, msg),
        2 => handle_withdraw_position_reply(deps, _env, msg),
        3 => handle_withdraw_position_end_round_reply(deps, _env, msg),
        _ => Err(ContractError::UnknownReplyId { id: msg.id }),
    }
}

fn handle_create_position_reply(deps: DepsMut, msg: Reply) -> Result<Response, ContractError> {
    // Parse the reply result into MsgCreatePositionResponse
    let response: MsgCreatePositionResponse = msg.result.try_into()?;

    // Load the current state
    let mut state = STATE.load(deps.storage)?;

    // Parse both amounts
    let amount0 =
        Uint128::from_str(&response.amount0).map_err(|_| ContractError::AssetNotFound {})?;
    let amount1 =
        Uint128::from_str(&response.amount1).map_err(|_| ContractError::AssetNotFound {})?;

    // Assign based on whether principal is first
    if state.principal_first {
        state.initial_principal_amount = amount0;
        state.principal_to_replenish = amount0;
        state.initial_counterparty_amount = amount1;
    } else {
        state.initial_principal_amount = amount1;
        state.principal_to_replenish = amount1;
        state.initial_counterparty_amount = amount0;
    }

    // Update the state with the new position ID
    state.position_id = Some(response.position_id);
    state.liquidity_shares = Some(response.liquidity_created);

    // Save the updated state back to storage
    STATE.save(deps.storage, &state)?;

    Ok(Response::new().add_attribute("position_id", response.position_id.to_string()))
}

fn handle_withdraw_position_reply(
    deps: DepsMut,
    env: Env,
    msg: Reply,
) -> Result<Response, ContractError> {
    // Parse the reply result into MsgCreatePositionResponse
    let response: MsgWithdrawPositionResponse = msg.result.try_into()?;

    // Load the current state
    let mut state = STATE.load(deps.storage)?;

    let liquidity = state
        .position_id
        .map(|id| {
            ConcentratedliquidityQuerier::new(&deps.querier)
                .position_by_id(id)
                .ok()
                .and_then(|resp| resp.position) // FullPositionBreakdown
                .and_then(|breakdown| breakdown.position) // Option<Position>
                .map(|pos| pos.liquidity) // Get liquidity as String
        })
        .flatten(); // Will be None if any of the steps fail

    state.liquidity_shares = liquidity; // Set to Some(liquidity) or None

    let counterparty_amount = if state.principal_first {
        // If principal is amount0, counterparty is amount1
        Uint128::from_str(&response.amount1)
    } else {
        // If principal is amount1, counterparty is amount0
        Uint128::from_str(&response.amount0)
    }
    .map_err(|_| StdError::generic_err("Invalid Uint128 value"))?;

    let liquidator_address = state
        .liquidator_address
        .clone()
        .ok_or(ContractError::NoLiquidatorAddress {})?;

    let mut messages = vec![];

    if counterparty_amount > Uint128::zero() {
        let counterparty_msg = BankMsg::Send {
            to_address: liquidator_address.into_string(),
            amount: vec![Coin {
                denom: state.counterparty_denom.to_string(),
                amount: counterparty_amount,
            }],
        };
        messages.push(counterparty_msg);
    }

    // Handle rewards if present
    if let Some(rewards) = &state.position_rewards {
        for reward in rewards {
            // Query contract's balance for the denom
            let contract_balance = deps
                .querier
                .query_balance(env.contract.address.clone(), &reward.denom)?;

            // If contract has enough balance, create a send msg
            if contract_balance.amount >= reward.amount {
                let reward_msg = BankMsg::Send {
                    to_address: state.principal_funds_owner.to_string(),
                    amount: vec![reward.clone()],
                };
                messages.push(reward_msg);
            }
        }
    }

    // Reset liquidator and save state
    state.liquidator_address = None;
    state.position_rewards = None;
    STATE.save(deps.storage, &state)?;

    Ok(Response::new()
        .add_messages(messages)
        .add_attribute("counterparty_amount", counterparty_amount.to_string()))
}
fn handle_withdraw_position_end_round_reply(
    deps: DepsMut,
    env: Env,
    msg: Reply,
) -> Result<Response, ContractError> {
    // Parse the reply result into MsgCreatePositionResponse
    let response: MsgWithdrawPositionResponse = msg.result.try_into()?;

    // Load the current state
    let mut state = STATE.load(deps.storage)?;

    // Parse both amounts
    let amount_0 = Uint128::from_str(&response.amount0)
        .map_err(|_| StdError::generic_err("Invalid Uint128 value"))?;
    let amount_1 = Uint128::from_str(&response.amount1)
        .map_err(|_| StdError::generic_err("Invalid Uint128 value"))?;

    // Determine which amount is principal and which is counterparty
    let (principal_amount, counterparty_amount) = if state.principal_first {
        (amount_0, amount_1)
    } else {
        (amount_1, amount_0)
    };

    let project_owner = state.position_created_address.clone();
    let principal_owner = state.principal_funds_owner.clone();

    // we have fully withdrawn position
    state.liquidity_shares = None;

    let mut messages = vec![];

    // Handle rewards if present
    if let Some(rewards) = &state.position_rewards {
        for reward in rewards {
            // Query contract's balance for the denom
            let contract_balance = deps
                .querier
                .query_balance(env.contract.address.clone(), &reward.denom)?;

            // If contract has enough balance, create a send msg
            if contract_balance.amount >= reward.amount {
                let reward_msg = BankMsg::Send {
                    to_address: state.principal_funds_owner.to_string(),
                    amount: vec![reward.clone()],
                };
                messages.push(reward_msg);
            }
        }
    }

    state.position_rewards = None;

    // check pulled principal amount
    if principal_amount >= state.principal_to_replenish {
        // send COUNTERPARTY to the project
        if counterparty_amount > Uint128::zero() {
            let counterparty_msg = BankMsg::Send {
                to_address: project_owner.clone().unwrap().into_string(),
                amount: vec![Coin {
                    denom: state.counterparty_denom.to_string(),
                    amount: counterparty_amount,
                }],
            };
            messages.push(counterparty_msg);
        }

        // send PRINCIPAL to principal owner
        if state.principal_to_replenish > Uint128::zero() {
            let principal_msg = BankMsg::Send {
                to_address: principal_owner.into_string(),
                amount: vec![Coin {
                    denom: state.principal_denom.to_string(),
                    amount: state.principal_to_replenish,
                }],
            };
            messages.push(principal_msg);
        }

        let remaining_amount = principal_amount - state.principal_to_replenish;
        // send remaining PRINCIPAL to the project
        if remaining_amount > Uint128::zero() {
            let remaining_principal_msg = BankMsg::Send {
                to_address: project_owner.unwrap().into_string(),
                amount: vec![Coin {
                    denom: state.principal_denom.to_string(),
                    amount: remaining_amount,
                }],
            };
            messages.push(remaining_principal_msg);
        }

        // all amount is replenished
        state.principal_to_replenish = Uint128::zero();
    } else {
        // send PRINCIPAL to principal owner
        if principal_amount > Uint128::zero() {
            let principal_msg = BankMsg::Send {
                to_address: principal_owner.into_string(),
                amount: vec![Coin {
                    denom: state.principal_denom.to_string(),
                    amount: principal_amount,
                }],
            };
            messages.push(principal_msg);
        }
        // there is possibility principal amount is zero and this method is called
        // Question: should we then prevent liqidation msg to be called if contract reaches auction state?
        state.auction_end_time = Some(env.block.time.plus_seconds(state.auction_duration));
        state.principal_to_replenish -= principal_amount;
        state.counterparty_to_give = Some(counterparty_amount);
    }

    STATE.save(deps.storage, &state)?;

    let event = Event::new("withdraw_from_position")
        .add_attribute("counterparty_amount", counterparty_amount.to_string())
        .add_attribute("principal_amount", principal_amount.to_string());

    // Return the response with the transfer message
    Ok(Response::new().add_messages(messages).add_event(event))
}

pub fn end_round(deps: DepsMut, env: Env, _info: MessageInfo) -> Result<Response, ContractError> {
    // Load the current state
    let mut state = STATE.load(deps.storage)?;

    let current_time = env.block.time;

    // Check that the round is ended by checking that the round_id is less than the current round
    if current_time < state.round_end_time {
        return Err(ContractError::Std(StdError::generic_err(
            "Round has not ended yet",
        )));
    }

    // Create withdraw message
    let withdraw_position_msg = MsgWithdrawPosition {
        position_id: state.position_id.unwrap(),
        sender: env.contract.address.to_string(),
        liquidity_amount: state.liquidity_shares.clone().unwrap(),
    };

    // Query the claimable spread rewards
    let spread_rewards = ConcentratedliquidityQuerier::new(&deps.querier)
        .claimable_spread_rewards(state.pool_id)
        .map_err(|_| ContractError::ClaimableSpreadRewardsQueryFailed {})? // Handle query errors
        .claimable_spread_rewards;

    // Query the claimable incentives
    let incentives: ClaimableIncentivesResponse = ConcentratedliquidityQuerier::new(&deps.querier)
        .claimable_incentives(state.pool_id)
        .map_err(|_| ContractError::ClaimableSpreadRewardsQueryFailed {})?;

    // Save into state
    state.position_rewards = Some(
        fetch_all_rewards(
            spread_rewards,
            incentives.claimable_incentives,
            incentives.forfeited_incentives,
        )
        .unwrap_or_default(),
    );

    // Wrap in SubMsg to handle response
    let submsg = SubMsg::reply_on_success(withdraw_position_msg, 3);

    // Save the updated state back to storage
    STATE.save(deps.storage, &state)?;

    Ok(Response::new()
        .add_submessage(submsg)
        .add_attribute("action", "withdraw_position"))
}
pub fn end_round_bid(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: EndRoundBidMsg,
) -> Result<Response, ContractError> {
    // Reject if user already has an existing bid
    if BIDS.may_load(deps.storage, info.sender.clone())?.is_some() {
        return Err(ContractError::BidAlreadyExists {});
    }

    let mut state = STATE.load(deps.storage)?;

    // Check that auction is active and has not ended
    if state.auction_end_time.is_none() {
        return Err(ContractError::AuctionNotActive {});
    }

    if env.block.time >= state.auction_end_time.unwrap() {
        return Err(ContractError::AuctionEnded {});
    }

    let principal = info
        .funds
        .iter()
        .find(|c| c.denom == state.principal_denom)
        .filter(|c| !c.amount.is_zero())
        .ok_or(ContractError::InsufficientFunds {})?;

    // Ensure the principal amount is at least 1% of principal_to_replenish
    let min_required_amount = state.principal_to_replenish / Uint128::new(100); // 1% of principal_to_replenish
    if principal.amount < min_required_amount {
        return Err(ContractError::BidTooSmall {
            min_required: min_required_amount,
            provided: principal.amount,
        });
    }
    // do not allow requested amounts that cannot be covered
    let counterparty_to_give = state
        .counterparty_to_give
        .ok_or(ContractError::CounterpartyNotSet {})?;
    if msg.requested_amount > counterparty_to_give {
        return Err(ContractError::RequestedAmountTooHigh {
            requested: msg.requested_amount,
            available: counterparty_to_give,
        });
    }

    // Calculate the price of the new bid
    let new_bid_price = Decimal::from_ratio(msg.requested_amount, principal.amount);

    // Load the sorted bids array
    let mut sorted_bids = SORTED_BIDS.load(deps.storage).unwrap_or_default();

    // Insert the new bid into the sorted array
    let position = sorted_bids
        .iter()
        .position(|(_, price, _)| new_bid_price > *price)
        .unwrap_or(sorted_bids.len());
    sorted_bids.insert(
        position,
        (info.sender.clone(), new_bid_price, principal.amount),
    );

    // Update the total principal deposited
    state.auction_principal_deposited += principal.amount;

    // Save bid
    BIDS.save(
        deps.storage,
        info.sender.clone(),
        &Bid {
            bidder: info.sender.clone(),
            principal_deposited: principal.amount,
            tokens_requested: msg.requested_amount,
            tokens_fulfilled: Uint128::zero(),
            tokens_refunded: Uint128::zero(),
            status: BidStatus::Submitted,
        },
    )?;

    let mut messages = vec![];

    // Check if the total principal deposited is already sufficient
    if state.auction_principal_deposited >= state.principal_to_replenish {
        // Check if the top bids are sufficient to replenish the principal
        let mut accumulated_principal = Uint128::zero();
        let mut selected_bids = Vec::new();
        let mut collecting_bids = true;

        for bid in sorted_bids.iter().rev() {
            let (bidder, _, principal_deposited) = bid;

            if collecting_bids {
                accumulated_principal += *principal_deposited;
                selected_bids.push(bid.clone());

                if accumulated_principal >= state.principal_to_replenish {
                    collecting_bids = false; // Stop collecting
                }
            } else {
                // This bid goes beyond what we need — refund it
                let refund_msg = BankMsg::Send {
                    to_address: bidder.clone().into_string(),
                    amount: vec![Coin {
                        denom: state.principal_denom.to_string(),
                        amount: *principal_deposited,
                    }],
                };
                messages.push(refund_msg);
                state.auction_principal_deposited -= *principal_deposited;

                // Update bid status
                let mut refund_bid = BIDS.load(deps.storage, bidder.clone())?;
                refund_bid.tokens_refunded = *principal_deposited;
                refund_bid.status = BidStatus::Refunded;
                BIDS.save(deps.storage, bidder.clone(), &refund_bid)?;
            }
        }

        selected_bids.reverse();
        sorted_bids = selected_bids;
    }
    // Save the updated state
    STATE.save(deps.storage, &state)?;
    SORTED_BIDS.save(deps.storage, &sorted_bids)?;

    Ok(Response::new()
        .add_attribute("action", "end_round_bid")
        .add_attribute("bidder", info.sender)
        .add_attribute("principal", principal.amount)
        .add_attribute("tokens_requested", msg.requested_amount))
}

pub fn withdraw_bid(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    // Load the current state
    let state = STATE.load(deps.storage)?;

    // Make sure auction is resolved
    // auction end time may have passed, but auction may not be resolved yet
    if state.auction_end_time.is_some() {
        return Err(ContractError::AuctionNotYetEnded {});
    }

    let mut bid = BIDS
        .may_load(deps.storage, info.sender.clone())?
        .ok_or(ContractError::NoBidFound {})?;

    // Allow withdrawal only if bid is still in Submitted state
    if bid.status != BidStatus::Submitted {
        return Err(ContractError::BidNotWithdrawable {});
    }

    // Update bid status and tokens_refunded
    bid.status = BidStatus::Refunded;
    bid.tokens_refunded = bid.principal_deposited;

    let bank_msg = BankMsg::Send {
        to_address: info.sender.clone().into_string(),
        amount: vec![Coin {
            denom: state.principal_denom.to_string(),
            amount: bid.principal_deposited,
        }],
    };

    // Save updated bid back to storage
    BIDS.save(deps.storage, info.sender.clone(), &bid)?;

    Ok(Response::new()
        .add_message(bank_msg)
        .add_attribute("action", "withdraw_bid")
        .add_attribute("bidder", info.sender))
}
pub fn resolve_auction(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
) -> Result<Response, ContractError> {
    let mut state = STATE.load(deps.storage)?;

    // Check if the auction is active and has ended
    if state.auction_end_time.is_none() {
        return Err(ContractError::AuctionNotActive {});
    }

    if _env.clone().block.time < state.auction_end_time.unwrap() {
        return Err(ContractError::AuctionNotYetEnded {});
    }

    // Load sorted bids (already sorted by price ratio in descending order)
    let mut sorted_bids = SORTED_BIDS.load(deps.storage).unwrap_or_default();

    let mut principal_accumulated = Uint128::zero();
    let mut counterparty_spent = Uint128::zero();
    let principal_target = state.principal_to_replenish;
    let counterparty_total = state
        .counterparty_to_give
        .ok_or(ContractError::CounterpartyNotSet {})?;
    let mut messages = vec![];

    // Iterate backwards through the sorted bids
    while let Some((bidder, bid_price, principal_deposited)) = sorted_bids.pop() {
        // If the full principal amount has been replenished, stop processing further bids
        if principal_accumulated >= principal_target {
            break;
        }

        // Load the bid details
        let bid = BIDS.load(deps.storage, bidder.clone())?;

        // Calculate the remaining principal needed
        let remaining_principal = principal_target - principal_accumulated;

        // Calculate the maximum principal we can take from this bid
        let max_principal_from_bid = principal_deposited;

        // Calculate how much principal we can take based on available counterparty
        let max_principal_based_on_counterparty =
            Decimal::from_ratio(counterparty_total - counterparty_spent, Uint128::one())
                / bid_price;

        // Round the result to the nearest integer
        let max_principal_based_on_counterparty =
            round_decimal_to_uint128(max_principal_based_on_counterparty);

        // Determine the actual principal to take
        let principal_to_take = std::cmp::min(
            remaining_principal,
            std::cmp::min(max_principal_from_bid, max_principal_based_on_counterparty),
        );

        // Calculate the corresponding counterparty tokens
        let counterparty_to_give =
            bid_price * Decimal::from_ratio(principal_to_take, Uint128::one());

        // Round the result to the nearest integer
        let rounded_counterparty_to_give = round_decimal_to_uint128(counterparty_to_give);

        // Create message to send counterparty tokens
        let counterparty_msg = BankMsg::Send {
            to_address: bid.bidder.clone().into_string(),
            amount: vec![Coin {
                denom: state.counterparty_denom.to_string(),
                amount: rounded_counterparty_to_give,
            }],
        };
        messages.push(counterparty_msg);

        // Create message to send principal tokens
        let principal_msg = BankMsg::Send {
            to_address: state.principal_funds_owner.clone().into_string(),
            amount: vec![Coin {
                denom: state.principal_denom.to_string(),
                amount: principal_to_take,
            }],
        };
        messages.push(principal_msg);

        // Update accumulated amounts
        principal_accumulated += principal_to_take;
        counterparty_spent += rounded_counterparty_to_give;

        let remaining_principal_in_bid = bid.principal_deposited - principal_to_take;

        // Refund the remaining principal (if any) and update bid status
        if !remaining_principal_in_bid.is_zero() {
            let refund_msg = BankMsg::Send {
                to_address: bid.bidder.clone().into_string(),
                amount: vec![Coin {
                    denom: state.principal_denom.to_string(),
                    amount: remaining_principal_in_bid,
                }],
            };
            messages.push(refund_msg);
        }

        // Update the bid status to refunded
        BIDS.save(
            deps.storage,
            bid.bidder.clone(),
            &Bid {
                status: BidStatus::Processed,
                tokens_refunded: remaining_principal_in_bid,
                tokens_fulfilled: rounded_counterparty_to_give,
                ..bid
            },
        )?;
    }

    // Check if contract is having enough counterparty amount
    if counterparty_spent > counterparty_total {
        return Err(ContractError::NotEnoughCounterpartyAmount {});
    }

    // Send remaining counterparty tokens back to the project
    let counterparty_to_project = counterparty_total
        .checked_sub(counterparty_spent)
        .unwrap_or(Uint128::zero());

    if !counterparty_to_project.is_zero() {
        let send_back_msg = BankMsg::Send {
            to_address: state
                .position_created_address
                .clone()
                .unwrap()
                .into_string(),
            amount: vec![Coin {
                denom: state.counterparty_denom.to_string(),
                amount: counterparty_to_project,
            }],
        };
        messages.push(send_back_msg);
    }

    // Reset auction state
    state.auction_end_time = None;
    state.principal_to_replenish -= principal_accumulated;
    state.counterparty_to_give = None;
    STATE.save(deps.storage, &state)?;

    Ok(Response::new()
        .add_messages(messages)
        .add_attribute("action", "resolve_auction")
        .add_attribute("counterparty_spent", counterparty_spent)
        .add_attribute("principal_replenished", principal_accumulated))
}

fn round_decimal_to_uint128(decimal: Decimal) -> Uint128 {
    // Add (10^18 - 1) to ensure the value is rounded up
    let ceil = (decimal.atomics().u128() + 10u128.pow(18) - 1) / 10u128.pow(18);
    Uint128::new(ceil)
}

pub fn query_get_state(deps: Deps) -> StdResult<StateResponse> {
    // Load the current state from storage
    let state = STATE.load(deps.storage)?;

    // Build the StateResponse using the loaded state
    let response = StateResponse {
        project_owner: state.project_owner,
        position_created_address: state.position_created_address,
        principal_funds_owner: state.principal_funds_owner.into_string(),
        pool_id: state.pool_id,
        counterparty_denom: state.counterparty_denom.clone(),
        principal_denom: state.principal_denom.clone(),
        position_id: state.position_id,
        initial_principal_amount: state.initial_principal_amount,
        initial_counterparty_amount: state.initial_counterparty_amount,
        liquidity_shares: state.liquidity_shares,
        auction_end_time: state.auction_end_time,
        principal_to_replenish: state.principal_to_replenish,
        counterparty_to_give: state.counterparty_to_give,
        auction_principal_deposited: state.auction_principal_deposited,
    };

    Ok(response)
}

pub fn query_get_bids(deps: Deps) -> StdResult<Vec<(String, Bid)>> {
    // Collect all bids from the BIDS map, converting each entry to a tuple (String, Bid)
    let all_bids: StdResult<Vec<(String, Bid)>> = BIDS
        .range(deps.storage, None, None, Order::Ascending)
        .map(|item| item.map(|(addr, bid)| (addr.to_string(), bid))) // Convert Addr to String
        .collect();

    // Prepare the response as a Vec<(String, Bid)>
    let response = all_bids.unwrap_or_default();

    // Convert the response into Binary (CosmWasm format)
    Ok(response)
}

pub fn query_bid(deps: Deps, address: String) -> StdResult<Bid> {
    let addr = deps.api.addr_validate(&address)?;
    BIDS.load(deps.storage, addr)
}

pub fn query_sorted_bids(deps: Deps) -> StdResult<Vec<(Addr, Decimal, Uint128)>> {
    // Load the sorted bids from storage
    let sorted_bids = SORTED_BIDS.load(deps.storage).unwrap_or_default();
    Ok(sorted_bids)
}
