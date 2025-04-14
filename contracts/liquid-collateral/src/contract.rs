use crate::error::ContractError;
use crate::msg::{
    CalculatedDataResponse, CreatePositionMsg, EndRoundBidMsg, ExecuteMsg, InstantiateMsg,
    QueryMsg, StateResponse,
};
use crate::state::{Bid, State, BIDS, STATE};
use cosmwasm_std::{
    entry_point, to_binary, to_json_binary, BankMsg, Binary, Coin, Decimal, Deps, DepsMut, Env,
    Event, MessageInfo, Order, Reply, Response, StdError, StdResult, SubMsg, Timestamp, Uint128,
};
use osmosis_std::types::cosmos::bank::v1beta1::MsgSend;
use osmosis_std::types::cosmos::base::v1beta1::Coin as OsmosisCoin;
use osmosis_std::types::osmosis::concentratedliquidity::v1beta1::ConcentratedliquidityQuerier;
use osmosis_std::types::osmosis::concentratedliquidity::v1beta1::{
    MsgCreatePosition, MsgCreatePositionResponse, MsgWithdrawPosition, MsgWithdrawPositionResponse,
};
use osmosis_std::types::osmosis::poolmanager::v1beta1::PoolmanagerQuerier;
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
        position_created_price: None,
        auction_duration: msg.auction_duration,
        auction_end_time: None,
        principal_funds_owner: deps.api.addr_validate(&msg.principal_funds_owner)?,
        principal_to_replenish: Uint128::zero(),
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
        ExecuteMsg::EndRoundBid(msg) => end_round_bid(deps, env, info, msg),
        ExecuteMsg::WidthdrawBid => withdraw_bid(deps, env, info),
        ExecuteMsg::ResolveAuction => resolve_auction(deps, env, info),
    }
}

#[entry_point]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        // Handle the GetState query
        QueryMsg::GetState {} => to_binary(&query_get_state(deps)?),
        QueryMsg::GetBids {} => to_binary(&query_get_bids(deps)?),
        QueryMsg::GetCalculatedOptimalCounterpartyUpperTick {
            lower_tick,
            principal_token_amount,
            liquidation_bonus,
            price_ratio,
        } => {
            // Call the query function with the fields directly
            to_binary(&query_get_calculated_optimal_counterparty_upper_tick(
                lower_tick,
                principal_token_amount,
                liquidation_bonus,
                price_ratio,
            )?)
        }
    }
}

pub fn query_get_calculated_optimal_counterparty_upper_tick(
    lower_tick: String,
    principal_token_amount: String,
    liquidation_bonus: String,
    price_ratio: String,
) -> StdResult<Binary> {
    // inputs: lower tick, base token amount, and liquidation bonus
    // output:  upper tick and counterparty

    let price_ratio: f64 = match price_ratio.parse() {
        Ok(val) => val,
        Err(_) => return Err(StdError::generic_err("Failed to parse price_ratio to f64")),
    };

    // Calculate the square root of the ratio (current price)
    let sqrt_current = price_ratio.sqrt();
    let lower_tick: f64 = match lower_tick.parse() {
        Ok(val) => val,
        Err(_) => return Err(StdError::generic_err("Failed to parse lower_tick to f64")),
    };
    let sqrt_lower = lower_tick.sqrt(); // Convert lower_tick to f64 and take the square root

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

    // Step 1: Calculate liquidity based on the principal token amount
    let liquidity = principal_token_amount / (sqrt_current - sqrt_lower);

    // Step 2: Adjust liquidity based on liquidation bonus
    let adjusted_base_token_amount = principal_token_amount * (1.0 + liquidation_bonus);
    let adjusted_liquidity = adjusted_base_token_amount / (sqrt_current - sqrt_lower);

    // Step 3: Calculate the upper tick based on adjusted liquidity
    let upper_tick = ((liquidity / adjusted_liquidity) + sqrt_current) * sqrt_current;
    //let upper_tick = 0.1 as f64;

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
        token_min_amount0: msg.counterparty_token_min_amount.to_string(),
        token_min_amount1: msg.principal_token_min_amount.to_string(),
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

    // Calculate the proportional liquidity amount to withdraw
    let withdraw_liquidity_amount = calculate_withdraw_liquidity_amount(
        principal_amount,
        principal_amount_to_replenish,
        liquidity_shares,
    )?;
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
    // update that liquidator replenished some principal amount
    state.principal_to_replenish = state.principal_to_replenish - principal.unwrap().amount;

    // Convert liquidity_shares from &str to Uint128
    let liquidity_shares_uint =
        Uint128::from_str(liquidity_shares).map_err(|_| ContractError::InvalidConversion {})?;

    // Subtract liquidity shares (ensuring no underflow)
    let updated_liquidity_shares = liquidity_shares_uint
        .checked_sub(withdraw_liquidity_amount)
        .unwrap_or(Uint128::zero()); // Prevents underflow

    // Update state with new liquidity shares value
    state.liquidity_shares = Some(updated_liquidity_shares.to_string());

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

pub fn calculate_withdraw_liquidity_amount(
    principal_amount: Decimal,
    principal_amount_to_replenish: Decimal,
    liquidity_shares: &str,
) -> Result<Uint128, ContractError> {
    // Ensure the supplied amount is not greater than the initial amount
    if principal_amount > principal_amount_to_replenish {
        return Err(ContractError::ExcessiveLiquidationAmount {});
    }

    // Calculate percentage to liquidate
    let perc_to_liquidate = principal_amount / principal_amount_to_replenish;

    // Parse liquidity_shares as a Decimal with 18 decimal places
    let liquidity_shares_decimal = Decimal::from_atomics(
        Uint128::from_str(liquidity_shares).map_err(|_| ContractError::InvalidConversion {})?,
        18, // Default precision for Decimal
    )
    .map_err(|_| ContractError::InvalidConversion {})?;

    // Perform high-precision multiplication
    let withdraw_liquidity_amount = liquidity_shares_decimal * perc_to_liquidate;

    // Round to the nearest integer and convert back to Uint128
    let liquidity_amount = withdraw_liquidity_amount.atomics().into();

    Ok(liquidity_amount)
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
    state.principal_to_replenish = Uint128::from_str(&response.amount1).unwrap();
    state.initial_counterparty_amount = Uint128::from_str(&response.amount0).unwrap();
    state.liquidity_shares = Some(response.liquidity_created);

    // Query the current spot price
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

    let amount_0 = Uint128::from_str(&response.amount0)
        .map_err(|_| StdError::generic_err("Invalid Uint128 value"))?;

    // Load the current state
    let mut state = STATE.load(deps.storage)?;

    let liquidator_address = state
        .liquidator_address
        .clone()
        .ok_or(ContractError::NoLiquidatorAddress {})?;

    let mut messages = vec![];

    let amount0 = response.amount0;
    if amount_0 > Uint128::zero() {
        let counterparty_msg = BankMsg::Send {
            to_address: liquidator_address.into_string(),
            amount: vec![Coin {
                denom: state.counterparty_denom.to_string(),
                amount: amount_0,
            }],
        };
        messages.push(counterparty_msg);
    }

    // Reset liquidator and save state
    state.liquidator_address = None;
    STATE.save(deps.storage, &state)?;

    Ok(Response::new()
        .add_messages(messages)
        .add_attribute("principal_amount", &response.amount1.to_string())
        .add_attribute("counterparty_amount", amount0.to_string()))
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

    let project_owner = state.position_created_address.clone();
    let principal_owner = state.principal_funds_owner.clone();

    // we have fully withdrawn position
    state.liquidity_shares = None;

    let mut messages = vec![];

    // Question: should we check here state.principal_to_replenish amount?
    // since it's possible there were partial liquidations which didn't replenish all amount
    if amount_1 >= state.principal_to_replenish {
        // send COUNTERPARTY to the project
        if amount_0 > Uint128::zero() {
            let counterparty_msg = BankMsg::Send {
                to_address: project_owner.clone().unwrap().into_string(),
                amount: vec![Coin {
                    denom: state.counterparty_denom.to_string(),
                    amount: amount_0,
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

        let remaining_amount = amount_1 - state.principal_to_replenish;
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
        if amount_1 > Uint128::zero() {
            let principal_msg = BankMsg::Send {
                to_address: principal_owner.into_string(),
                amount: vec![Coin {
                    denom: state.principal_denom.to_string(),
                    amount: amount_1,
                }],
            };
            messages.push(principal_msg);
        }
        // there is possibility principal amount is zero and this method is called
        // Question: should we then prevent liqidation msg to be called if contract reaches auction state?
        state.auction_end_time = Some(env.block.time.plus_seconds(state.auction_duration));
        state.principal_to_replenish = state.principal_to_replenish - amount_1;
        state.counterparty_to_give = Some(amount_0);
    }

    STATE.save(deps.storage, &state)?;

    let event = Event::new("withdraw_from_position")
        .add_attribute("counterparty_amount", response.amount0.to_string())
        .add_attribute("principal_amount", response.amount1.to_string());

    // Return the response with the transfer message
    Ok(Response::new().add_messages(messages).add_event(event))
}

pub fn end_round(deps: DepsMut, env: Env, info: MessageInfo) -> Result<Response, ContractError> {
    // Load the current state
    let state = STATE.load(deps.storage)?;

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
    let state = STATE.load(deps.storage)?;

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

    // Calculate the percentage replenished
    let percentage_replenished = if state.principal_to_replenish.is_zero() {
        Decimal::zero() // if no replenishment is needed, no percentage can be calculated
    } else {
        Decimal::from_ratio(principal.amount, state.principal_to_replenish)
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

    let bank_msg = BankMsg::Send {
        to_address: info.sender.clone().into_string(),
        amount: vec![Coin {
            denom: state.principal_denom.to_string(),
            amount: bid.principal_amount,
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
    if state.auction_end_time.is_none() {
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
    let principal_target = state.principal_to_replenish;
    let counterparty_total = state
        .counterparty_to_give
        .ok_or(ContractError::CounterpartyNotSet {})?;
    let mut messages = vec![];

    for bid in all_bids.iter() {
        // If the full principal amount has been replenished, stop processing further bids
        if principal_accumulated >= principal_target {
            break;
        }

        // Create message to send counterparty tokens
        let counterparty_msg = BankMsg::Send {
            to_address: bid.bidder.clone().into_string(),
            amount: vec![Coin {
                denom: state.counterparty_denom.to_string(),
                amount: bid.tokens_requested,
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
        project_owner: state.project_owner,
        principal_funds_owner: state.principal_funds_owner.into_string(),
        pool_id: state.pool_id,
        counterparty_denom: state.counterparty_denom.clone(),
        principal_denom: state.principal_denom.clone(),
        position_id: state.position_id,
        initial_principal_amount: state.initial_principal_amount,
        initial_counterparty_amount: state.initial_counterparty_amount,
        liquidity_shares: state.liquidity_shares,
        position_created_price: state.position_created_price,
        auction_end_time: state.auction_end_time,
        principal_to_replenish: state.principal_to_replenish,
        counterparty_to_give: state.counterparty_to_give,
    };

    to_json_binary(&response)
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

    pub const FIVE_SECONDS: u64 = 5;

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
            round_duration: FIVE_SECONDS,
            principal_funds_owner: pool_mockup.principal_funds_owner.address(),
            project_owner: None, // Addr
            auction_duration: FIVE_SECONDS,
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

        println!("Contract deployed at: {}\n", contract_addr);

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
        println!("Claimable Spread Rewards:\n");
        for reward in &full_position.claimable_spread_rewards {
            let denom = &reward.denom;
            let amount = &reward.amount;
            println!("Denom: {}, Amount: {}", denom, amount);
        }

        if let Some(position) = &full_position.position {
            println!("Liquidity: {}\n", position.liquidity); // Print the value
        } else {
            println!("Position not found\n");
        }
    }
    fn query_and_print_balance(
        bank: &Bank<'_, OsmosisTestApp>,
        address: &str,
        denom: &str,
        user_name: &str,
    ) -> String {
        let amount = bank
            .query_balance(&QueryBalanceRequest {
                address: address.to_string(),
                denom: denom.to_string(),
            })
            .unwrap()
            .balance
            .unwrap()
            .amount;

        println!("{}'s balance for {}: {}", user_name, denom, amount);
        amount
    }

    #[test]
    fn test_calculate_withdraw_liquidity_amount() {
        // Create a mock principal Coin
        let coin = Coin {
            denom: "uosmo".to_string(),
            amount: Uint128::new(3333), // Example principal amount
        };
        let principal = Some(&coin);

        // Create a mock initial principal amount
        let initial_principal_amount = Uint128::new(100000); // Example initial principal amount
                                                             // Convert base_amount and initial_base_amount to Decimal for precise division
        let principal_amount = Decimal::from_atomics(principal.unwrap().amount, 0)
            .map_err(|_| ContractError::InvalidConversion {});
        let initial_principal_amount = Decimal::from_atomics(initial_principal_amount, 0)
            .map_err(|_| ContractError::InvalidConversion {});
        let mock_liquidity_shares = Some("92195444572928873195000".to_string());

        let liquidity_shares = &mock_liquidity_shares
            .as_deref() // Converts Option<String> to Option<&str>
            .unwrap_or("0"); // Default value if None

        let result = calculate_withdraw_liquidity_amount(
            principal_amount.unwrap(),
            initial_principal_amount.unwrap(),
            liquidity_shares,
        );

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Uint128::new(3072874167615719343589)); // 50% of 1000 = 500
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
        let response = create_position(&wasm, &contract_addr, &pool_mockup.project_owner);

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
                println!("Position not found\n");
                String::from("0") // Default value
            }
        } else {
            println!("Failed to get position response\n");
            String::from("0") // Default value
        };

        let bank = Bank::new(&pool_mockup.app);
        let usdc_balance = query_and_print_balance(&bank, &contract_addr, USDC_DENOM, "Contract");

        let osmo_balance = query_and_print_balance(&bank, &contract_addr, OSMO_DENOM, "Contract");

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
                println!("Position not found\n");
                String::from("0") // Default value
            }
        } else {
            println!("Failed to get position response\n");
            String::from("0") // Default value
        };

        //92195444572928873195000
        let liquidate_msg = ExecuteMsg::Liquidate;

        //100000
        let coins = &[Coin::new(99999u128, OSMO_DENOM)];

        println!("Executing liquidate msg...\n");
        let response = wasm
            .execute(
                &contract_addr,
                &liquidate_msg,
                coins,
                &pool_mockup.liquidator,
            )
            .expect("Execution failed");
        //println!("Execution successful: {:?}", response);
        //println!("{:?}", response.events);

        let position_response = pool_mockup.position_query(1);
        //println!("{:#?}", position_response);
        println!("Printing position details after liquidation...\n");
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

        println!("\nQuery-ing contract bank balances after liquidation...\n");
        let bank = Bank::new(&pool_mockup.app);
        let usdc_balance = query_and_print_balance(&bank, &contract_addr, USDC_DENOM, "Contract");

        let osmo_balance = query_and_print_balance(&bank, &contract_addr, OSMO_DENOM, "Contract");

        println!("\nQuery-ing principal owner bank balances after liquidation...\n");
        let usdc_balance = query_and_print_balance(
            &bank,
            &pool_mockup.principal_funds_owner.address(),
            USDC_DENOM,
            "Principal funds owner",
        );

        let osmo_balance = query_and_print_balance(
            &bank,
            &pool_mockup.principal_funds_owner.address(),
            OSMO_DENOM,
            "Principal funds owner",
        );
        let bank = Bank::new(&pool_mockup.app);
        println!("\nQuery-ing project owner bank balances after liquidation...\n");
        let usdc_balance = query_and_print_balance(
            &bank,
            &pool_mockup.project_owner.address(),
            USDC_DENOM,
            "Project owner",
        );

        let osmo_balance = query_and_print_balance(
            &bank,
            &pool_mockup.project_owner.address(),
            OSMO_DENOM,
            "Project owner",
        );

        println!("\nQuery-ing liquidator bank balances after liquidation...\n");
        let bank = Bank::new(&pool_mockup.app);
        let usdc_balance = query_and_print_balance(
            &bank,
            &pool_mockup.liquidator.address(),
            USDC_DENOM,
            "Liquidator",
        );

        let osmo_balance = query_and_print_balance(
            &bank,
            &pool_mockup.liquidator.address(),
            OSMO_DENOM,
            "Liquidator",
        );

        let query_msg = QueryMsg::GetState {};

        let query_result: Binary = wasm.query(&contract_addr, &query_msg).unwrap();
        let state_response: StateResponse = from_json(&query_result).unwrap();
        let formatted_output = serde_json::to_string_pretty(&state_response).unwrap();
        // Print the state response
        println!("{}", formatted_output);

        println!("Printing position details...\n");
        let position_response = pool_mockup.position_query(1);
        let liquidity = if let Ok(full_position) = position_response {
            // Print the full position details using the helper method
            print_position_details(&full_position);

            // Extract liquidity
            if let Some(position) = full_position.position {
                position.liquidity // Return liquidity
            } else {
                println!("Position not found\n");
                String::from("0") // Default value
            }
        } else {
            println!("Failed to get position response\n");
            String::from("0") // Default value
        };
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

        let query_calculated_data = QueryMsg::GetCalculatedOptimalCounterpartyUpperTick {
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
