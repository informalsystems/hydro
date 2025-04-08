use crate::error::ContractError;
use crate::msg::{
    CalculatedDataResponse, CreatePositionMsg, EndRoundBidMsg, ExecuteMsg, InstantiateMsg,
    ParametersMsg, QueryMsg, StateResponse,
};
use crate::state::{Bid, State, BIDS, RESERVATIONS, STATE};
use cosmwasm_std::{
    entry_point, to_binary, to_json_binary, Binary, CosmosMsg, Decimal, Deps, DepsMut, Env, Event,
    MessageInfo, Order, QueryResponse, Reply, Response, StdError, StdResult, SubMsg, Timestamp,
    Uint128,
};
use osmosis_std::types::cosmos::bank::v1beta1::MsgSend;
use osmosis_std::types::cosmos::base::v1beta1::Coin;
use osmosis_std::types::osmosis::concentratedliquidity::v1beta1::ConcentratedliquidityQuerier;
use osmosis_std::types::osmosis::concentratedliquidity::v1beta1::{
    MsgCreatePosition, MsgCreatePositionResponse, MsgWithdrawPosition, MsgWithdrawPositionResponse,
};
use osmosis_std::types::osmosis::poolmanager::v1beta1::PoolmanagerQuerier;
use std::error::Error;
use std::str::FromStr;

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let round_id = compute_current_round_id(_env, msg.first_round_start, msg.round_length);

    let state = State {
        owner: info.sender.clone(),
        pool_id: msg.pool_id,
        position_id: None,
        principal_denom: msg.principal_denom,
        counterparty_denom: msg.counterparty_denom,
        initial_principal_amount: Uint128::zero(),
        initial_counterparty_amount: Uint128::zero(),
        liquidity_shares: None,
        liquidator_address: None,
        first_round_start: msg.first_round_start,
        round_length: msg.round_length,
        position_created_price: None,
        round_id: round_id.unwrap(),
        auction_duration: msg.auction_duration,
        auction_period: false,
        auction_end_time: None,
        hydro: deps.api.addr_validate(&msg.hydro)?,
        principal_to_replenish: None,
        counterparty_to_give: None,
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
        ExecuteMsg::EndRoundBid(msg) => end_round_bid(deps, info, msg),
        ExecuteMsg::WidthdrawBid => withdraw_bid(deps, env, info),
        ExecuteMsg::ResolveAuction => resolve_auction(deps, env, info),
    }
}

#[entry_point]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        // Handle the GetState query
        QueryMsg::GetState {} => to_binary(&query_get_state(deps)?),
        QueryMsg::GetReservations {} => to_binary(&query_get_reservations(deps)?),
        QueryMsg::GetBids {} => to_binary(&query_get_bids(deps)?),
        QueryMsg::GetCalculatedPosition {
            lower_tick,
            principal_token_amount,
            liquidation_bonus,
            price_ratio,
        } => {
            // Call the query function with the fields directly
            to_binary(&query_get_calculated_position(
                deps,
                lower_tick,
                principal_token_amount,
                liquidation_bonus,
                price_ratio,
            )?)
        }
    }
}

pub fn query_get_calculated_position(
    deps: Deps,
    lower_tick: String,
    principal_token_amount: String,
    liquidation_bonus: String,
    price_ratio: String,
) -> StdResult<Binary> {
    // inputs: lower tick, base token amount, and liquidation bonus
    // output:  upper tick and counterparty

    let price_ratio: f64 = match price_ratio.parse() {
        Ok(val) => val,
        Err(_) => panic!("Failed to parse price_ratio to f64"), // Handle parsing error
    };

    // Calculate the square root of the ratio (current price)
    let sqrt_current = price_ratio.sqrt();
    let lower_tick: f64 = match lower_tick.parse() {
        Ok(val) => val,
        Err(_) => panic!("Failed to parse lower_tick to f64"), // Handle parsing error
    };
    let sqrt_lower = lower_tick.sqrt(); // Convert lower_tick to f64 and take the square root

    let principal_token_amount: f64 = match principal_token_amount.parse() {
        Ok(val) => val,
        Err(_) => panic!("Failed to parse principal_token_amount to f64"), // Handle parsing error
    };

    let liquidation_bonus: f64 = match liquidation_bonus.parse() {
        Ok(val) => val,
        Err(_) => panic!("Failed to parse liquidation_bonus to f64"), // Handle parsing error
    };

    // Step 1: Calculate liquidity based on the principal token amount
    let liquidity = principal_token_amount / (sqrt_current - sqrt_lower);

    // Step 2: Adjust liquidity based on liquidation bonus
    let adjusted_base_token_amount = principal_token_amount * (1.0 + liquidation_bonus);
    let adjusted_liquidity = adjusted_base_token_amount / (sqrt_current - sqrt_lower);

    // Step 3: Calculate the upper tick based on adjusted liquidity
    let upper_tick = ((liquidity / adjusted_liquidity) + sqrt_current) * sqrt_current;

    // Step 4: Calculate counterparty amount (WOBBLE) based on liquidity
    let sqrt_upper = upper_tick.sqrt();
    let counterparty_amount = adjusted_liquidity * (1.0 / sqrt_current - 1.0 / sqrt_upper);

    // Create and return the response struct
    let response = CalculatedDataResponse {
        upper_tick: upper_tick.to_string(),
        counterparty_amount: counterparty_amount.to_string(),
    };
    to_json_binary(&response)
}

pub fn create_position(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: CreatePositionMsg,
) -> Result<Response, ContractError> {
    // straight pass - no validation from contract
    let state = STATE.load(deps.storage)?;

    // Check if the position_id already exists
    if state.position_id.is_some() {
        return Err(ContractError::PositionAlreadyExists {});
    }

    // Only owner can create position
    if info.sender != state.owner {
        return Err(ContractError::Unauthorized {});
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

    // Create position message
    let create_position_msg = MsgCreatePosition {
        pool_id: state.pool_id,
        sender: env.contract.address.to_string(),
        lower_tick: msg.lower_tick,
        upper_tick: msg.upper_tick,
        tokens_provided: vec![
            Coin {
                denom: state.counterparty_denom.clone(),
                amount: counterparty_amount.to_string(),
            },
            Coin {
                denom: state.principal_denom.clone(),
                amount: principal_amount.to_string(),
            },
        ],
        token_min_amount0: msg.counterparty_token_min_amount.to_string(),
        token_min_amount1: msg.principal_token_min_amount.to_string(),
    };

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

    // Validate funds sent
    let principal = info.funds.iter().find(|c| c.denom == state.principal_denom);

    if principal.is_none() {
        return Err(ContractError::InsufficientFunds {});
    }

    // Convert base_amount and initial_base_amount to Decimal for precise division
    let principal_amount = Decimal::from_atomics(principal.unwrap().amount, 0)
        .map_err(|_| ContractError::InvalidConversion {})?;
    let initial_principal_amount = Decimal::from_atomics(state.initial_principal_amount, 0)
        .map_err(|_| ContractError::InvalidConversion {})?;

    // Ensure the supplied amount is not greater than the initial amount
    if principal_amount > initial_principal_amount {
        return Err(ContractError::ExcessiveLiquidationAmount {});
    }

    // Calculate percentage to liquidate
    let perc_to_liquidate = principal_amount / initial_principal_amount;

    let position = ConcentratedliquidityQuerier::new(&deps.querier)
        .position_by_id(state.position_id.unwrap())
        .map_err(|_| ContractError::PositionQueryFailed {})? // Handle query errors
        .position
        .ok_or(ContractError::PositionNotFound {})?; // Return error if position is None

    // Extract asset1 amount safely
    let asset1_amount = position
        .asset1
        .map(|coin| coin.amount)
        .ok_or(ContractError::AssetNotFound {})?;

    // Check if asset1_amount is non-zero
    // if it's zero - price went below lower tick (since principal token amount is zero)
    if asset1_amount != "0" {
        return Err(ContractError::ThresholdNotMet {
            amount: asset1_amount,
        });
    }

    let liquidity_shares = state
        .liquidity_shares
        .as_deref() // Converts Option<String> to Option<&str>
        .unwrap_or("0"); // Default value if None

    let liquidity_shares = Uint128::from_str(liquidity_shares).unwrap();

    // Calculate the proportional liquidity amount to withdraw
    let withdraw_liquidity_amount = liquidity_shares * perc_to_liquidate;
    // substract liquidity and save again

    // Create withdraw message
    let withdraw_position_msg = MsgWithdrawPosition {
        position_id: state.position_id.unwrap(),
        sender: _env.contract.address.to_string(),
        liquidity_amount: withdraw_liquidity_amount.to_string(),
    };

    // Wrap in SubMsg to handle response
    let submsg = SubMsg::reply_on_success(withdraw_position_msg, 2);

    state.liquidator_address = Some(info.sender);

    // Subtract liquidity shares (ensuring no underflow)
    let updated_liquidity_shares = liquidity_shares
        .checked_sub(withdraw_liquidity_amount)
        .unwrap_or(Uint128::zero()); // Prevents underflow

    // Update state with new liquidity shares value
    state.liquidity_shares = Some(updated_liquidity_shares.to_string());

    // Save the updated state back to storage
    STATE.save(deps.storage, &state)?;

    Ok(Response::new()
        .add_submessage(submsg)
        .add_attribute("action", "withdraw_position"))
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

    // Update the state with the new position ID
    state.position_id = Some(response.position_id);
    state.initial_principal_amount = Uint128::from_str(&response.amount1).unwrap();
    state.initial_counterparty_amount = Uint128::from_str(&response.amount0).unwrap();
    state.liquidity_shares = Some(response.liquidity_created);

    // Query the current spot price
    //TODO make sure this is accurate - async
    let ratio_str = PoolmanagerQuerier::new(&deps.querier)
        .spot_price(
            state.pool_id,
            state.counterparty_denom.clone(),
            state.principal_denom.clone(),
        )
        .map_err(|_| ContractError::PriceQueryFailed {})? // Handle query errors
        .spot_price;

    state.position_created_price = Some(ratio_str);

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
    // Get liquidator address (this should be set somewhere in your state or logic)
    if let Some(liquidator_address) = state.liquidator_address.clone() {
        // Create a transfer message to send amount0 to the liquidator
        let amount0 = response.amount0;
        let transfer_msg = MsgSend {
            from_address: env.contract.address.into_string(),
            to_address: liquidator_address.into_string(),
            amount: vec![Coin {
                denom: state.counterparty_denom.to_string(),
                amount: amount0.clone(),
            }],
        };

        // Save the updated state (if necessary)
        state.liquidator_address = None; // Reset liquidator address after the transfer
        STATE.save(deps.storage, &state)?;

        let event = Event::new("withdraw_from_position")
            .add_attribute("counterparty_amount", amount0.to_string());

        // Return the response with the transfer message
        return Ok(Response::new()
            .add_message(transfer_msg) // Add transfer message
            .add_event(event));
    } else {
        return Err(ContractError::NoLiquidatorAddress {});
    }
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

    let amount_1 = Uint128::from_str(&response.amount1)
        .map_err(|_| StdError::generic_err("Invalid Uint128 value"))?;

    let amount_0 = Uint128::from_str(&response.amount0)
        .map_err(|_| StdError::generic_err("Invalid Uint128 value"))?;

    if amount_1 >= state.initial_principal_amount {
        let mut owner_reservations = RESERVATIONS
            .may_load(deps.storage, &state.owner.to_string())?
            .unwrap_or_default();
        let mut hydro_reservations = RESERVATIONS
            .may_load(deps.storage, &state.hydro.to_string())?
            .unwrap_or_default();

        // Reserve WOBBLE for the project
        owner_reservations.push(Coin {
            denom: state.counterparty_denom.clone(),
            amount: amount_0.to_string(),
        });

        // Reserve PRINCIPAL for hydro
        hydro_reservations.push(Coin {
            denom: state.principal_denom.clone(),
            amount: state.initial_principal_amount.to_string(),
        });
        let remaining_amount = amount_1 - state.initial_principal_amount;
        // If remaining amount is positive, reserve it for the project
        if remaining_amount > Uint128::zero() {
            owner_reservations.push(Coin {
                denom: state.principal_denom.clone(),
                amount: amount_1.to_string(),
            });
        }
        RESERVATIONS.save(deps.storage, &state.owner.to_string(), &owner_reservations)?;
        RESERVATIONS.save(deps.storage, &state.hydro.to_string(), &hydro_reservations)?;
    } else {
        state.auction_period = true;
        state.auction_end_time = Some(env.block.time.plus_seconds(state.auction_duration));
        state.principal_to_replenish = Some(state.initial_principal_amount - amount_1);
        state.counterparty_to_give = Some(amount_0);
    }

    STATE.save(deps.storage, &state)?;

    let event = Event::new("withdraw_from_position")
        .add_attribute("counterparty_amount", response.amount0.to_string())
        .add_attribute("principal_amount", response.amount1.to_string());

    // Return the response with the transfer message
    return Ok(Response::new().add_event(event));
}

pub fn end_round(deps: DepsMut, env: Env, info: MessageInfo) -> Result<Response, ContractError> {
    // Load the current state
    let mut state = STATE.load(deps.storage)?;
    let current_round_id = compute_current_round_id(
        env.clone(),
        state.first_round_start,
        state.round_length.clone(),
    )?;

    // Check that the round is ended by checking that the round_id is less than the current round
    if state.round_id >= current_round_id {
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

    // Wrap in SubMsg to handle response
    let submsg = SubMsg::reply_on_success(withdraw_position_msg, 3);

    state.round_id = current_round_id;

    // Save the updated state back to storage
    STATE.save(deps.storage, &state)?;

    Ok(Response::new()
        .add_submessage(submsg)
        .add_attribute("action", "withdraw_position"))
}
pub fn end_round_bid(
    deps: DepsMut,
    info: MessageInfo,
    msg: EndRoundBidMsg,
) -> Result<Response, ContractError> {
    let state = STATE.load(deps.storage)?;

    // Check that auction is active and has not ended
    if !state.auction_period {
        return Err(ContractError::AuctionNotActive {});
    }

    let principal = info
        .funds
        .iter()
        .find(|c| c.denom == state.principal_denom)
        .filter(|c| !c.amount.is_zero())
        .ok_or(ContractError::InsufficientFunds {})?;

    // Calculate the percentage replenished
    let percentage_replenished = if state.principal_to_replenish.unwrap().is_zero() {
        Decimal::zero() // if no replenishment is needed, no percentage can be calculated
    } else {
        Decimal::from_ratio(principal.amount, state.principal_to_replenish.unwrap())
    };

    // Save bid
    BIDS.save(
        deps.storage,
        info.sender.clone(),
        &Bid {
            bidder: info.sender.clone(),
            principal_amount: principal.amount,
            tokens_requested: msg.requested_amount,
            percentage_replenished,
        },
    )?;

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

    let bid = BIDS
        .may_load(deps.storage, info.sender.clone())?
        .ok_or(ContractError::NoBidFound {})?;

    BIDS.remove(deps.storage, info.sender.clone());

    let bank_msg = MsgSend {
        from_address: _env.contract.address.into_string(),
        to_address: info.sender.clone().into_string(),
        amount: vec![Coin {
            denom: state.principal_denom.to_string(),
            amount: bid.principal_amount.to_string(),
        }],
    };

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
    if !state.auction_period || state.auction_end_time.is_none() {
        return Err(ContractError::AuctionNotActive {});
    }

    if _env.clone().block.time < state.auction_end_time.unwrap() {
        return Err(ContractError::AuctionNotYetEnded {});
    }

    let mut all_bids: Vec<Bid> = BIDS
        .range(deps.storage, None, None, Order::Ascending)
        .map(|item| item.map(|(_, bid)| bid))
        .collect::<StdResult<Vec<_>>>()?;

    // Sort bids by price per COUNTERPARTY (tokens_requested / principal_amount)
    all_bids.sort_by(|a, b| {
        let price_a = Decimal::from_ratio(a.tokens_requested, a.principal_amount);
        let price_b = Decimal::from_ratio(b.tokens_requested, b.principal_amount);
        price_a
            .partial_cmp(&price_b)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut principal_accumulated = Uint128::zero();
    let mut counterparty_spent = Uint128::zero();
    let principal_target = state
        .principal_to_replenish
        .ok_or(ContractError::PrincipalNotSet {})?;
    let counterparty_total = state
        .counterparty_to_give
        .ok_or(ContractError::CounterpartyNotSet {})?;
    let mut messages = vec![];

    for bid in all_bids.iter() {
        // If the full principal amount has been replenished, stop processing further bids
        if principal_accumulated >= principal_target {
            break;
        }

        //// Calculate how much principal is needed to reach the full amount
        let principal_needed = principal_target
            .checked_sub(principal_accumulated)
            .unwrap_or(Uint128::zero());

        // If no principal is left to be replenished, break the loop
        if principal_needed.is_zero() {
            break;
        }

        // If the bid's principal is greater than the remaining principal needed, skip it
        if bid.principal_amount > principal_needed {
            continue;
        }

        let counterparty_available = counterparty_total
            .checked_sub(counterparty_spent)
            .unwrap_or(Uint128::zero());

        if bid.tokens_requested > counterparty_available {
            continue; // skip bid if not enough counterparty tokens left
        }

        // Create message to send counterparty tokens
        let counterparty_msg = MsgSend {
            from_address: _env.contract.address.clone().into_string(),
            to_address: bid.bidder.clone().into_string(),
            amount: vec![Coin {
                denom: state.counterparty_denom.to_string(),
                amount: bid.tokens_requested.to_string(),
            }],
        };

        messages.push(counterparty_msg);

        // Update accumulated amounts
        principal_accumulated += bid.principal_amount;
        counterparty_spent += bid.tokens_requested;

        // Clean up: Remove the processed bid
        BIDS.remove(deps.storage, bid.bidder.clone());
    }

    // Check if the auction was able to fully replenish the principal amount
    if principal_accumulated < principal_target {
        return Err(ContractError::PrincipalNotFullyReplenished {});
    }

    // Send remaining counterparty tokens back to the project
    let counterparty_to_project = counterparty_total
        .checked_sub(counterparty_spent)
        .unwrap_or(Uint128::zero());

    if !counterparty_to_project.is_zero() {
        let send_back_msg = MsgSend {
            from_address: _env.contract.address.into_string(),
            to_address: state.owner.clone().into_string(),
            amount: vec![Coin {
                denom: state.counterparty_denom.to_string(),
                amount: counterparty_to_project.to_string(),
            }],
        };
        messages.push(send_back_msg);
    }

    // Reset auction state
    state.auction_period = false;
    state.auction_end_time = None;
    STATE.save(deps.storage, &state)?;

    Ok(Response::new()
        .add_messages(messages)
        .add_attribute("action", "resolve_auction")
        .add_attribute("counterparty_spent", counterparty_spent)
        .add_attribute("principal_replenished", principal_accumulated))
}

pub fn query_get_state(deps: Deps) -> StdResult<Binary> {
    // Load the current state from storage
    let state = STATE.load(deps.storage)?;

    // Build the StateResponse using the loaded state
    let response = StateResponse {
        owner: state.owner.to_string(),
        pool_id: state.pool_id,
        counterparty_denom: state.counterparty_denom.clone(),
        principal_denom: state.principal_denom.clone(),
        position_id: state.position_id,
        initial_principal_amount: state.initial_principal_amount,
        initial_counterparty_amount: state.initial_counterparty_amount,
        liquidity_shares: state.liquidity_shares,
        position_created_price: state.position_created_price,
        auction_period: state.auction_period,
        auction_end_time: state.auction_end_time,
        principal_to_replenish: state.principal_to_replenish,
        counterparty_to_give: state.counterparty_to_give,
    };

    to_json_binary(&response)
}

pub fn query_get_reservations(deps: Deps) -> StdResult<Binary> {
    let reservations = RESERVATIONS
        .range(deps.storage, None, None, Order::Ascending)
        .map(|result| {
            match result {
                Ok((key, coin_list)) => {
                    // Convert the raw key bytes to a UTF-8 string
                    let key_str =
                        String::from_utf8(key.into()).unwrap_or_else(|_| "Invalid Key".to_string());
                    Ok((key_str, coin_list)) // Now coin_list is Vec<Coin>
                }
                Err(e) => Err(e),
            }
        })
        .collect::<Result<Vec<(String, Vec<Coin>)>, cosmwasm_std::StdError>>()?;

    Ok(to_binary(&reservations)?)
}
pub fn query_get_bids(deps: Deps) -> StdResult<Binary> {
    // Collect all bids from the BIDS map, converting each entry to a tuple (String, Bid)
    let all_bids: StdResult<Vec<(String, Bid)>> = BIDS
        .range(deps.storage, None, None, Order::Ascending)
        .map(|item| item.map(|(addr, bid)| (addr.to_string(), bid))) // Convert Addr to String
        .collect();

    // Prepare the response as a Vec<(String, Bid)>
    let response = all_bids.unwrap_or_default();

    // Convert the response into Binary (CosmWasm format)
    Ok(to_binary(&response)?)
}

// Computes the current round_id by taking contract_start_time and dividing the time since
// by the round_length.
pub fn compute_current_round_id(
    env: Env,
    first_round_start: Timestamp,
    round_length: u64,
) -> StdResult<u64> {
    compute_round_id_for_timestamp(first_round_start, round_length, env.block.time.nanos())
}

fn compute_round_id_for_timestamp(
    first_round_start: Timestamp,
    round_length: u64,
    timestamp: u64,
) -> StdResult<u64> {
    // If the first round has not started yet, return an error
    if timestamp < first_round_start.nanos() {
        return Err(StdError::generic_err("The first round has not started yet"));
    }
    let time_since_start = timestamp - first_round_start.nanos();
    let round_id = time_since_start / round_length;

    Ok(round_id)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::mock::mock::{store_contracts_code, PoolMockup};
    use cosmwasm_std::{from_binary, from_json, from_slice, Addr, Coin};
    use osmosis_std::types::{
        cosmos::bank::v1beta1::{BankQuerier, QueryBalanceRequest},
        cosmwasm::wasm::v1::MsgExecuteContractResponse,
        osmosis::concentratedliquidity::v1beta1::FullPositionBreakdown,
    };
    use osmosis_test_tube::{
        Account, Bank, ExecuteResponse, Module, OsmosisTestApp, Runner, SigningAccount, Wasm,
    };

    pub const USDC_DENOM: &str =
        "ibc/498A0751C798A0D9A389AA3691123DADA57DAA4FE165D5C75894505B876BA6E4";
    pub const OSMO_DENOM: &str = "uosmo";

    pub const ONE_SECOND: u64 = 1;

    pub fn instantiate(
        wasm: &Wasm<OsmosisTestApp>, // Borrow wasm reference
        pool_mockup: &PoolMockup,
        code_id: u64, // Borrow pool_mockup reference
    ) -> String {
        // Return only the contract address as a String

        let instantiate_msg = InstantiateMsg {
            pool_id: 1,
            counterparty_denom: USDC_DENOM.to_owned(),
            principal_denom: OSMO_DENOM.to_owned(),
            first_round_start: Timestamp::from_seconds(0),
            round_length: ONE_SECOND,
            hydro: pool_mockup.user2.address(), // Addr
            auction_duration: 1000,
        };

        let contract_addr = wasm
            .instantiate(
                code_id,
                &instantiate_msg,
                None,
                Some("liquid-collateral"),
                &[],
                &pool_mockup.deployer,
            )
            .expect("Contract instantiation failed")
            .data
            .address;

        println!("Contract deployed at: {}", contract_addr);

        contract_addr // Return the contract address
    }

    pub fn create_position(
        wasm: &Wasm<OsmosisTestApp>,
        contract_addr: &str,
        deployer: &SigningAccount,
    ) -> ExecuteResponse<MsgExecuteContractResponse> {
        let msg = ExecuteMsg::CreatePosition(CreatePositionMsg {
            lower_tick: -108000000,
            upper_tick: 342000000,
            counterparty_token_min_amount: 85000u128.into(),
            principal_token_min_amount: 100000u128.into(),
        });

        let coins = &[
            Coin::new(85000u128, USDC_DENOM),
            Coin::new(100000u128, OSMO_DENOM),
        ];

        wasm.execute(contract_addr, &msg, coins, deployer)
            .expect("Execution failed")
    }

    pub fn print_position_details(full_position: &FullPositionBreakdown) {
        // Extract amounts safely
        let asset0_amount = full_position
            .asset0
            .as_ref()
            .map(|coin| coin.amount.clone())
            .unwrap_or_else(|| String::from("0"));

        let asset1_amount = full_position
            .asset1
            .as_ref()
            .map(|coin| coin.amount.clone())
            .unwrap_or_else(|| String::from("0"));

        // Print extracted values
        println!("USDC Amount: {}", asset0_amount);
        println!("OSMO Amount: {}", asset1_amount);

        // Print claimable spread rewards
        println!("Claimable Spread Rewards:");
        for reward in &full_position.claimable_spread_rewards {
            let denom = &reward.denom;
            let amount = &reward.amount;
            println!("Denom: {}, Amount: {}", denom, amount);
        }

        if let Some(position) = &full_position.position {
            println!("Liquidity: {}", position.liquidity); // Print the value
        } else {
            println!("Position not found");
        }
    }

    #[test]
    fn test_create_and_withdraw_position_in_pool() {
        /*
                        type: osmosis/cl-create-position
                value:
                  lower_tick: '-108000000'
                  pool_id: '1464'
                  sender: osmo1dlp3hevpc88upn06awnpu8zm37xn4etudrdx0s
                  token_min_amount0: '85000'
                  token_min_amount1: '24978'
                  tokens_provided:
                    - amount: '29387'
                      denom: ibc/498A0751C798A0D9A389AA3691123DADA57DAA4FE165D5C75894505B876BA6E4
                    - amount: '100000'
                      denom: uosmo
                  upper_tick: '342000000'

        type: osmosis/cl-create-position
        value:
          lower_tick: '-7568000'
          pool_id: '1464'
          sender: osmo1dlp3hevpc88upn06awnpu8zm37xn4etudrdx0s
          token_min_amount0: '170000'
          token_min_amount1: '0'
          tokens_provided:
            - amount: '24782'
              denom: ibc/498A0751C798A0D9A389AA3691123DADA57DAA4FE165D5C75894505B876BA6E4
            - amount: '200000'
              denom: uosmo
          upper_tick: '-6842600'

                 */
        let pool_mockup = PoolMockup::new();

        let wasm = Wasm::new(&pool_mockup.app);
        let code_id = store_contracts_code(&wasm, &pool_mockup.deployer);
        let contract_addr = instantiate(&wasm, &pool_mockup, code_id);
        println!("Creating position...\n");
        let response = create_position(&wasm, &contract_addr, &pool_mockup.deployer);
        //println!("Execution successful: {:?}", response);
        for event in response.events {
            if event.ty == "create_position" {
                for attr in event.attributes {
                    if attr.key == "position_id" {
                        println!("Position ID: {}", attr.value);
                    }
                }
            }
        }

        let position_response = pool_mockup.position_query(1);

        let query_msg = QueryMsg::GetState {};

        let query_result: Binary = wasm.query(&contract_addr, &query_msg).unwrap();
        let state_response: StateResponse = from_json(&query_result).unwrap();
        let formatted_output = serde_json::to_string_pretty(&state_response).unwrap();

        // Print the state response
        println!("{}", formatted_output);

        println!("Printing position details...\n");
        let liquidity = if let Ok(full_position) = position_response {
            // Print the full position details using the helper method
            print_position_details(&full_position);

            // Extract liquidity
            if let Some(position) = full_position.position {
                position.liquidity // Return liquidity
            } else {
                println!("Position not found");
                String::from("0") // Default value
            }
        } else {
            println!("Failed to get position response");
            String::from("0") // Default value
        };

        println!("Query-ing bank balances for contract...\n");
        let bank = Bank::new(&pool_mockup.app);
        let amount_usdc = bank
            .query_balance(&QueryBalanceRequest {
                address: contract_addr.to_string(),
                denom: USDC_DENOM.into(),
            })
            .unwrap()
            .balance
            .unwrap()
            .amount;
        let amount_osmo = bank
            .query_balance(&QueryBalanceRequest {
                address: contract_addr.to_string(),
                denom: OSMO_DENOM.into(),
            })
            .unwrap()
            .balance
            .unwrap()
            .amount;

        println!("Contract USDC pre withdrawal: {}", amount_usdc); // Print the value
        println!("Contract OSMO pre withdrawal: {}", amount_osmo);

        // this swap should make price goes below lower range - should make OSMO amount in pool be zero
        let usdc_needed: u128 = 117_647_058_823;
        println!("Doing a swap which will make principal amount goes to zero in the pool...\n");
        let swap_response = pool_mockup.swap_usdc_for_osmo(&pool_mockup.user1, usdc_needed, 1);
        let position_response = pool_mockup.position_query(1);

        println!("Printing position details after swap...\n");
        let liquidity = if let Ok(full_position) = position_response {
            // Print the full position details using the helper method
            print_position_details(&full_position);

            // Extract liquidity
            if let Some(position) = full_position.position {
                position.liquidity // Return liquidity
            } else {
                println!("Position not found");
                String::from("0") // Default value
            }
        } else {
            println!("Failed to get position response");
            String::from("0") // Default value
        };

        //92195444572928873195000
        let liquidate_msg = ExecuteMsg::Liquidate;

        //100000
        let coins = &[Coin::new(100000u128, OSMO_DENOM)];

        println!("Executing liquidate msg...\n");
        let response = wasm
            .execute(&contract_addr, &liquidate_msg, coins, &pool_mockup.user1)
            .expect("Execution failed");
        //println!("Execution successful: {:?}", response);
        //println!("{:?}", response.events);

        let position_response = pool_mockup.position_query(1);
        //println!("{:#?}", position_response);
        println!("Printing position details after liqudation...\n");
        let liquidity = if let Ok(full_position) = position_response {
            // Print the full position details using the helper method
            print_position_details(&full_position);

            // Extract liquidity
            if let Some(position) = full_position.position {
                position.liquidity // Return liquidity
            } else {
                println!("Position not found");
                String::from("0") // Default value
            }
        } else {
            println!("Failed to get position response");
            String::from("0") // Default value
        };

        println!("Query-ing contract bank balances after liquidation...\n");
        let bank = Bank::new(&pool_mockup.app);
        let amount_usdc = bank
            .query_balance(&QueryBalanceRequest {
                address: contract_addr.to_string(),
                denom: USDC_DENOM.into(),
            })
            .unwrap()
            .balance
            .unwrap()
            .amount;
        let amount_osmo = bank
            .query_balance(&QueryBalanceRequest {
                address: contract_addr.to_string(),
                denom: OSMO_DENOM.into(),
            })
            .unwrap()
            .balance
            .unwrap()
            .amount;

        println!("Contract USDC after withdrawal: {}", amount_usdc); // Print the value
        println!("Contract OSMO after withdrawal: {}", amount_osmo);

        let query_msg = QueryMsg::GetState {};

        let query_result: Binary = wasm.query(&contract_addr, &query_msg).unwrap();
        let state_response: StateResponse = from_json(&query_result).unwrap();
        let formatted_output = serde_json::to_string_pretty(&state_response).unwrap();
        // Print the state response
        println!("{}", formatted_output);
    }
    #[test]
    fn test_end_of_round_principal_higher_or_equal() {
        let pool_mockup = PoolMockup::new();
        let wasm = Wasm::new(&pool_mockup.app);
        let code_id = store_contracts_code(&wasm, &pool_mockup.deployer);
        let contract_addr = instantiate(&wasm, &pool_mockup, code_id);
        println!("Creating position...\n");
        let response = create_position(&wasm, &contract_addr, &pool_mockup.deployer);

        // this swap should make principal amount being higher than on creating position
        let osmo_needed: u128 = 10;
        println!("Doing a swap which will make principal amount being higher than on creating position...\n");
        let swap_response = pool_mockup.swap_osmo_for_usdc(&pool_mockup.user1, osmo_needed, 1);

        let position_response = pool_mockup.position_query(1);

        println!("Printing position details after swap...\n");
        let liquidity = if let Ok(full_position) = position_response {
            // Print the full position details using the helper method
            print_position_details(&full_position);

            // Extract liquidity
            if let Some(position) = full_position.position {
                position.liquidity // Return liquidity
            } else {
                println!("Position not found");
                String::from("0") // Default value
            }
        } else {
            println!("Failed to get position response");
            String::from("0") // Default value
        };

        let end_round_msg = ExecuteMsg::EndRound;

        println!("Executing end round msg...\n");
        let response = wasm
            .execute(&contract_addr, &end_round_msg, &[], &pool_mockup.user1)
            .expect("Execution failed");

        println!("End round msg events {:?}", response.events);

        let query_msg = QueryMsg::GetState {};

        let query_result: Binary = wasm.query(&contract_addr, &query_msg).unwrap();
        let state_response: StateResponse = from_json(&query_result).unwrap();
        let formatted_output = serde_json::to_string_pretty(&state_response).unwrap();

        println!("Printing contract state...\n");
        // Print the state response
        println!("{}", formatted_output);

        let query_msg = QueryMsg::GetReservations {};

        let query_result: Binary = wasm.query(&contract_addr, &query_msg).unwrap();
        let reservation_response: Vec<(String, Vec<Coin>)> = from_binary(&query_result).unwrap();
        println!(
            "Reservations... ({} total keys)\n",
            reservation_response.len()
        );

        for (key, coins) in reservation_response {
            println!("Key: {}", key);
            if coins.is_empty() {
                println!("  No coins reserved.");
            } else {
                for coin in coins {
                    println!("  - Amount: {} {}", coin.amount, coin.denom);
                }
            }
            println!(); // Print an empty line between reservations
        }

        println!("Query-ing contract bank balances after liquidation...\n");
        let bank = Bank::new(&pool_mockup.app);
        let amount_usdc = bank
            .query_balance(&QueryBalanceRequest {
                address: contract_addr.to_string(),
                denom: USDC_DENOM.into(),
            })
            .unwrap()
            .balance
            .unwrap()
            .amount;
        let amount_osmo = bank
            .query_balance(&QueryBalanceRequest {
                address: contract_addr.to_string(),
                denom: OSMO_DENOM.into(),
            })
            .unwrap()
            .balance
            .unwrap()
            .amount;

        println!("Contract USDC after withdrawal: {}", amount_usdc); // Print the value
        println!("Contract OSMO after withdrawal: {}", amount_osmo);
    }
    #[test]
    fn test_auction() {
        let pool_mockup = PoolMockup::new();
        let wasm = Wasm::new(&pool_mockup.app);
        let code_id = store_contracts_code(&wasm, &pool_mockup.deployer);
        let contract_addr = instantiate(&wasm, &pool_mockup, code_id);
        println!("Creating position...\n");
        let response = create_position(&wasm, &contract_addr, &pool_mockup.deployer);

        // this swap should make principal amount being lower than on creating position but not zero
        let usdc_needed: u128 = 100000;
        println!("Doing a swap which will make principal amount being lower than on creating position but not zero...\n");
        let swap_response = pool_mockup.swap_usdc_for_osmo(&pool_mockup.user1, usdc_needed, 1);

        let position_response = pool_mockup.position_query(1);

        println!("Printing position details after swap...\n");
        let liquidity = if let Ok(full_position) = position_response {
            // Print the full position details using the helper method
            print_position_details(&full_position);

            // Extract liquidity
            if let Some(position) = full_position.position {
                position.liquidity // Return liquidity
            } else {
                println!("Position not found");
                String::from("0") // Default value
            }
        } else {
            println!("Failed to get position response");
            String::from("0") // Default value
        };

        let end_round_msg = ExecuteMsg::EndRound;

        println!("Executing end round msg...\n");
        let response = wasm
            .execute(&contract_addr, &end_round_msg, &[], &pool_mockup.user1)
            .expect("Execution failed");

        println!("End round msg events {:?}", response.events);

        let query_msg = QueryMsg::GetState {};

        let query_result: Binary = wasm.query(&contract_addr, &query_msg).unwrap();
        let state_response: StateResponse = from_json(&query_result).unwrap();
        let formatted_output = serde_json::to_string_pretty(&state_response).unwrap();

        println!("Printing contract state...\n");
        // Print the state response
        println!("{}", formatted_output);

        let first_bid = ExecuteMsg::EndRoundBid(EndRoundBidMsg {
            requested_amount: 10u128.into(),
        });

        let coins = &[Coin::new(1u128, OSMO_DENOM)];

        println!("Executing end round bid msg...\n");
        let response = wasm
            .execute(&contract_addr, &first_bid, coins, &pool_mockup.user2)
            .expect("Execution failed");

        let withdraw_bid_msg = ExecuteMsg::WidthdrawBid;

        println!("Executing withdraw bid msg...\n");
        let response = wasm
            .execute(&contract_addr, &withdraw_bid_msg, &[], &pool_mockup.user2)
            .expect("Execution failed");

        let user3_bid = ExecuteMsg::EndRoundBid(EndRoundBidMsg {
            requested_amount: 10u128.into(),
        });

        let coins = &[Coin::new(10000u128, OSMO_DENOM)];

        println!("Executing end round bid msg from user3...\n");
        let response = wasm
            .execute(&contract_addr, &user3_bid, coins, &pool_mockup.user3)
            .expect("Execution failed");

        let user4_bid = ExecuteMsg::EndRoundBid(EndRoundBidMsg {
            requested_amount: 10000u128.into(),
        });

        let coins = &[Coin::new(10000u128, OSMO_DENOM)];

        println!("Executing end round bid msg from user 4...\n");
        let response = wasm
            .execute(&contract_addr, &user4_bid, coins, &pool_mockup.user4)
            .expect("Execution failed");

        let user5_bid = ExecuteMsg::EndRoundBid(EndRoundBidMsg {
            requested_amount: 10000u128.into(),
        });

        let coins = &[Coin::new(33805u128, OSMO_DENOM)];

        println!("Executing end round bid msg from user 5...\n");
        let response = wasm
            .execute(&contract_addr, &user5_bid, coins, &pool_mockup.user5)
            .expect("Execution failed");

        println!("Increasing time for 1000 seconds...\n");
        pool_mockup.app.increase_time(10000);

        let query_bids = QueryMsg::GetBids {};

        let query_result: Binary = wasm.query(&contract_addr, &query_bids).unwrap();
        let bids_response: Vec<(String, Bid)> = from_json(&query_result).unwrap();
        // Deserialize the response to get the bids

        // Print all bids in a structured format
        for (bidder, bid) in bids_response {
            println!(
        "Bidder Address: {}\n  Principal Amount: {}\n  Tokens Requested: {}\n  Percentage Replenished: {}\n",
        bidder, bid.principal_amount, bid.tokens_requested, bid.percentage_replenished
    );
        }

        let resolve_auction_msg = ExecuteMsg::ResolveAuction;

        println!("Executing resolve auction msg...\n");
        let response = wasm
            .execute(
                &contract_addr,
                &resolve_auction_msg,
                &[],
                &pool_mockup.deployer,
            )
            .expect("Execution failed");
    }

    #[test]
    fn test_calculate_position() {
        let pool_mockup = PoolMockup::new();
        let wasm = Wasm::new(&pool_mockup.app);
        let code_id = store_contracts_code(&wasm, &pool_mockup.deployer);
        let contract_addr = instantiate(&wasm, &pool_mockup, code_id);

        let query_calculated_data = QueryMsg::GetCalculatedPosition {
            lower_tick: "0.03".to_string(),              // Example lower tick
            principal_token_amount: "100.0".to_string(), // Example principal token amount
            liquidation_bonus: "0.0".to_string(),        // 10 %liquidation bonus
            price_ratio: "0.0555555556".to_string(),     // Example price ratio
        };

        let query_result: Binary = wasm.query(&contract_addr, &query_calculated_data).unwrap();
        let data_response: CalculatedDataResponse = from_json(&query_result).unwrap();

        // Deserialize the binary response into the appropriate struct
        //let data_response: CalculatedDataResponse = from_binary(&query_result).unwrap();

        // Print the values from the deserialized response
        println!("Upper Tick: {}", data_response.upper_tick);
        println!("Counterparty Amount: {}", data_response.counterparty_amount);
    }
}
