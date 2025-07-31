#![allow(clippy::unneeded_struct_pattern)]
use crate::error::ContractError;
use crate::msg::{
    BidResponse, BidsResponse, CreatePositionMsg, ExecuteMsg, InstantiateMsg,
    IsLiquidatableResponse, PlaceBidMsg, QueryMsg, SimulateLiquidationResponse, SortedBidsResponse,
    StateResponse, WithdrawBidMsg,
};
use crate::state::{Bid, BidStatus, SortedBid, State, BIDS, BID_COUNTER, SORTED_BIDS, STATE};
use cosmwasm_std::{
    entry_point, to_json_binary, BankMsg, Binary, Coin, Decimal, Deps, DepsMut, Env, Event,
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
        position_admin: match msg.position_admin {
            Some(admin_str) => Some(deps.api.addr_validate(&admin_str)?),
            None => None,
        },
        pool_id: msg.pool_id,
        counterparty_owner: match msg.counterparty_owner {
            Some(owner_str) => Some(deps.api.addr_validate(&owner_str)?),
            None => None,
        },
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
        ExecuteMsg::Liquidate {} => liquidate(deps, env, info),
        ExecuteMsg::EndRound {} => end_round(deps, env, info),
        ExecuteMsg::PlaceBid(msg) => place_bid(deps, env, info, msg),
        ExecuteMsg::WithdrawBid(msg) => withdraw_bid(deps, env, info, msg),
        ExecuteMsg::ResolveAuction {} => resolve_auction(deps, env, info),
    }
}

#[entry_point]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        // Handle the GetState query
        QueryMsg::State {} => to_json_binary(&query_get_state(deps)?),
        QueryMsg::Bid { bid_id } => to_json_binary(&query_bid(deps, bid_id)?),
        QueryMsg::Bids { start_from, limit } => {
            to_json_binary(&query_get_bids(deps, start_from, limit)?)
        }
        QueryMsg::SortedBids {} => to_json_binary(&query_sorted_bids(deps)?),
        QueryMsg::IsLiquidatable {} => to_json_binary(&query_is_liquidatable(deps)?),
        QueryMsg::SimulateLiquidation { principal_amount } => {
            to_json_binary(&query_simulate_liquidation(deps, principal_amount)?)
        }
    }
}

/// Create position with reply
/// - based on the passed position arguments, method is sending MsgCreatePosition to the cl module on Osmosis
/// - in the reply contract is updating the state with the position information.
/// - The order of the tokens which will be put in tokens provided is very important
/// - on osmosis they check lexicographical order: https://github.com/osmosis-labs/osmosis/blob/main/x/concentrated-liquidity/types/msgs.go#L42C2-L44C3
/// - ibc/token should go before uosmo token for example
/// - if order is not correct - tx will fail!
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

    // Only admin can create position - if present
    match &state.position_admin {
        Some(admin) if info.sender != *admin => {
            return Err(ContractError::Unauthorized {});
        }
        _ => {}
    }

    let mut tokens_provided: Vec<OsmosisCoin> = Vec::new();

    for coin in &info.funds {
        let denom = coin.denom.as_str();

        if denom == state.principal_denom {
            state.initial_principal_amount = coin.amount;
        } else if denom == state.counterparty_denom {
            state.initial_counterparty_amount = coin.amount;
        } else {
            return Err(ContractError::AssetNotFound {});
        }

        tokens_provided.push(OsmosisCoin {
            denom: coin.denom.clone(),
            amount: coin.amount.to_string(),
        });
    }
    // Assign min amounts based on principal_first
    let (token_min_amount0, token_min_amount1) = if state.principal_first {
        (
            msg.principal_token_min_amount.to_string(),
            msg.counterparty_token_min_amount.to_string(),
        )
    } else {
        (
            msg.counterparty_token_min_amount.to_string(),
            msg.principal_token_min_amount.to_string(),
        )
    };

    let create_position_msg = MsgCreatePosition {
        pool_id: state.pool_id,
        sender: env.contract.address.to_string(),
        lower_tick: msg.lower_tick,
        upper_tick: msg.upper_tick,
        tokens_provided,
        token_min_amount0,
        token_min_amount1,
    };

    // Only assign if counterparty_owner wasn't already set during initialization
    if state.counterparty_owner.is_none() {
        state.counterparty_owner = Some(info.sender);
    }

    STATE.save(deps.storage, &state)?;

    let submsg = SubMsg::reply_on_success(create_position_msg, 1);

    Ok(Response::new()
        .add_submessage(submsg)
        .add_attribute("action", "create_position")
        .add_attribute("pool_id", state.pool_id.to_string())
        .add_attribute("lower_tick", msg.lower_tick.to_string())
        .add_attribute("upper_tick", msg.upper_tick.to_string()))
}

/*
## Liquidate position with reply
 - method checks whether principal amount in the position is zero - which needs to be the case in order to allow liquidation
 - the percentage of liquidation amount is calculated based on the principal funds liquidator sent to the contract (can be full or partial)
 - amount is immediately transferred to the principal_funds owner (so contract doesn't need to hold anything)
 - MsgWithdrawPosition on Osmosis module is called
 - in the reply the counterparty amount which was pulled from the pool is sent to the liquidator address
 - in case of full liquidation - all rewards are being sent to principal_funds_owner
*/
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

    // Check if current principal amount inside position is non-zero
    // if it's zero - price hit the lower/upper tick (since principal token amount is zero)
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

    let liquidity_shares_decimal = parse_liquidity(liquidity_shares)?;

    // Calculate the proportional liquidity amount to withdraw
    let withdraw_liquidity_amount = calculate_withdraw_liquidity_amount(
        principal_amount,
        principal_amount_to_replenish,
        liquidity_shares_decimal,
    )?;

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
            .claimable_spread_rewards(state.position_id.unwrap())
            .map_err(|_| ContractError::ClaimableSpreadRewardsQueryFailed {})? // Handle query errors
            .claimable_spread_rewards;

        // Query the claimable incentives
        let incentives: ClaimableIncentivesResponse =
            ConcentratedliquidityQuerier::new(&deps.querier)
                .claimable_incentives(state.position_id.unwrap())
                .map_err(|_| ContractError::ClaimableSpreadRewardsQueryFailed {})?;

        state.position_rewards = Some(
            fetch_all_rewards(
                spread_rewards,
                incentives.claimable_incentives,
                incentives.forfeited_incentives,
            )
            .unwrap_or_default(),
        );
    }

    let submsg = SubMsg::reply_on_success(withdraw_position_msg, 2);

    state.liquidator_address = Some(info.sender);
    // update that liquidator replenished some principal amount
    state.principal_to_replenish -= principal.unwrap().amount;

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

    let mut state = STATE.load(deps.storage)?;

    // Parse both amounts
    let amount0 =
        Uint128::from_str(&response.amount0).map_err(|_| ContractError::AssetNotFound {})?;
    let amount1 =
        Uint128::from_str(&response.amount1).map_err(|_| ContractError::AssetNotFound {})?;

    let (principal_used, counterparty_used) = if state.principal_first {
        (amount0, amount1)
    } else {
        (amount1, amount0)
    };

    let mut messages = vec![];

    // determine the refund address recipient
    let position_created_address = state
        .position_admin
        .clone()
        .or(state.counterparty_owner.clone())
        .ok_or(ContractError::MissingPositionCreator)?;

    if state.initial_principal_amount > principal_used {
        let principal_diff = BankMsg::Send {
            to_address: position_created_address.clone().into_string(),
            amount: vec![Coin {
                denom: state.principal_denom.to_string(),
                amount: state.initial_principal_amount - principal_used,
            }],
        };
        messages.push(principal_diff);
    }

    if state.initial_counterparty_amount > counterparty_used {
        let counterparty_diff = BankMsg::Send {
            to_address: position_created_address.clone().into_string(),
            amount: vec![Coin {
                denom: state.counterparty_denom.to_string(),
                amount: state.initial_counterparty_amount - counterparty_used,
            }],
        };
        messages.push(counterparty_diff);
    }

    state.initial_principal_amount = principal_used;
    state.principal_to_replenish = principal_used;
    state.initial_counterparty_amount = counterparty_used;

    // Update the state with the new position ID
    state.position_id = Some(response.position_id);
    state.liquidity_shares = Some(response.liquidity_created);

    // Save the updated state back to storage
    STATE.save(deps.storage, &state)?;

    Ok(Response::new()
        .add_messages(messages)
        .add_attribute("position_id", response.position_id.to_string()))
}

fn handle_withdraw_position_reply(
    deps: DepsMut,
    _env: Env,
    msg: Reply,
) -> Result<Response, ContractError> {
    let response: MsgWithdrawPositionResponse = msg.result.try_into()?;

    let mut state = STATE.load(deps.storage)?;

    // Fetch the current liquidity of the position after withdrawal
    let liquidity = state.position_id.and_then(|id| {
        ConcentratedliquidityQuerier::new(&deps.querier)
            .position_by_id(id)
            .ok()
            .and_then(|resp| resp.position)
            .and_then(|breakdown| breakdown.position)
            .map(|pos| pos.liquidity)
    });

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
            let reward_msg = BankMsg::Send {
                to_address: state.principal_funds_owner.to_string(),
                amount: vec![reward.clone()],
            };
            messages.push(reward_msg);
        }
    }

    // Reset liquidator and save state
    state.liquidator_address = None;
    STATE.save(deps.storage, &state)?;

    Ok(Response::new()
        .add_messages(messages)
        .add_attribute("counterparty_amount", counterparty_amount.to_string()))
}
pub fn handle_withdraw_position_end_round_reply(
    deps: DepsMut,
    env: Env,
    msg: Reply,
) -> Result<Response, ContractError> {
    let response: MsgWithdrawPositionResponse = msg.result.try_into()?;

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

    let project_owner = state.counterparty_owner.clone();
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

/*
## End round with reply
 - is only executed if round has ended
 - full position withdraw is executed (MsgWithdrawPosition with all liquidity amount)
 - in the reply:
   - all rewards are sent to principal_funds_owner
   - if there are enough (equal or more) principal amount than needed for replenish:
     - all fetched counterparty amount is sent to position_created_address (or project owner)
     - potential excessive principal amount is sent to position_created_address
     - exact amount needed to be replenished is sent to principal_funds_owner
   - in case there were not enough principal amount for replenish:
     - send whatever principal amount is fetched to principal_funds_owner
     - decrement principal amount needed
     - update counteparty amount available in the auction
     - start the auction
*/
pub fn end_round(deps: DepsMut, env: Env, _info: MessageInfo) -> Result<Response, ContractError> {
    // Load the current state
    let mut state = STATE.load(deps.storage)?;

    let current_time = env.block.time;

    // Check if the round ended by checking if current block time is less than the round_end_time
    if current_time < state.round_end_time {
        return Err(ContractError::Std(StdError::generic_err(
            "Round has not ended yet",
        )));
    }

    let withdraw_position_msg = MsgWithdrawPosition {
        position_id: state.position_id.unwrap(),
        sender: env.contract.address.to_string(),
        liquidity_amount: state.liquidity_shares.clone().unwrap(),
    };

    // Query the claimable spread rewards
    let spread_rewards = ConcentratedliquidityQuerier::new(&deps.querier)
        .claimable_spread_rewards(state.position_id.unwrap())
        .map_err(|_| ContractError::ClaimableSpreadRewardsQueryFailed {})? // Handle query errors
        .claimable_spread_rewards;

    // Query the claimable incentives
    let incentives: ClaimableIncentivesResponse = ConcentratedliquidityQuerier::new(&deps.querier)
        .claimable_incentives(state.position_id.unwrap())
        .map_err(|_| ContractError::ClaimableIncentivesQueryFailed {})?;

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

    STATE.save(deps.storage, &state)?;

    Ok(Response::new()
        .add_submessage(submsg)
        .add_attribute("action", "withdraw_position"))
}

/*
## Place bid
 - can only be executed if auction is in progress
 - bidder sends desired principal amount to the contract and request some amount of counterparty token
 - bidder must replenish at least 1% of principal
 - bidder cannot request more counterparty than contract has available
 - bid is being saved in bids and in sorted bids
 - in case the new bid makes principal amount be potentially replenished, unneeded (worst) bid/s will be kicked out
 - if one or more bids are kicked out from sorted bids - bidders will be refunded and correct status of the bid will be saved
*/
pub fn place_bid(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: PlaceBidMsg,
) -> Result<Response, ContractError> {
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

    let mut sorted_bids = SORTED_BIDS.load(deps.storage).unwrap_or_default();

    let counter = BID_COUNTER.may_load(deps.storage)?.unwrap_or(1);

    // Insert the new bid into the sorted array
    let position = sorted_bids
        .iter()
        .position(|bid| new_bid_price > bid.price)
        .unwrap_or(sorted_bids.len());

    sorted_bids.insert(
        position,
        SortedBid {
            bid_id: counter,
            price: new_bid_price,
            principal_deposited: principal.amount,
            requested_counterparty: msg.requested_amount,
        },
    );

    BIDS.save(
        deps.storage,
        counter,
        &Bid {
            bidder: info.sender.clone(),
            principal_deposited: principal.amount,
            tokens_requested: msg.requested_amount,
            tokens_fulfilled: Uint128::zero(),
            tokens_refunded: Uint128::zero(),
            status: BidStatus::Submitted,
        },
    )?;
    BID_COUNTER.save(deps.storage, &(counter + 1))?;

    let mut messages = vec![];

    // Check if the total principal deposited is already sufficient
    if state.auction_principal_deposited >= state.principal_to_replenish {
        // Check if the top bids are sufficient to replenish the principal
        let mut accumulated_principal = Uint128::zero();
        let mut selected_bids = Vec::new();
        let mut collecting_bids = true;

        for bid in sorted_bids.iter().rev() {
            let bid_id = bid.bid_id;
            let principal_deposited = bid.principal_deposited;

            if collecting_bids {
                accumulated_principal += principal_deposited;
                selected_bids.push(bid.clone());

                if accumulated_principal >= state.principal_to_replenish {
                    collecting_bids = false;
                }
            } else {
                let mut refund_bid = BIDS.load(deps.storage, bid_id)?;
                // This bid goes beyond what we need — refund it
                let refund_msg = BankMsg::Send {
                    to_address: refund_bid.bidder.clone().into_string(),
                    amount: vec![Coin {
                        denom: state.principal_denom.to_string(),
                        amount: principal_deposited,
                    }],
                };
                messages.push(refund_msg);
                state.auction_principal_deposited -= principal_deposited;

                // Update bid status
                refund_bid.tokens_refunded = principal_deposited;
                refund_bid.status = BidStatus::Refunded;
                BIDS.save(deps.storage, bid_id, &refund_bid)?;
            }
        }

        selected_bids.reverse();
        sorted_bids = selected_bids;
    }
    // Update the total principal deposited
    state.auction_principal_deposited += principal.amount;
    // Save the updated state
    STATE.save(deps.storage, &state)?;
    SORTED_BIDS.save(deps.storage, &sorted_bids)?;

    Ok(Response::new()
        .add_messages(messages)
        .add_attribute("action", "end_round_bid")
        .add_attribute("bidder", info.sender)
        .add_attribute("principal", principal.amount)
        .add_attribute("tokens_requested", msg.requested_amount))
}

pub fn withdraw_bid(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: WithdrawBidMsg,
) -> Result<Response, ContractError> {
    let state = STATE.load(deps.storage)?;

    // Make sure auction is resolved
    // auction end time may have passed, but auction may not be resolved yet
    if state.auction_end_time.is_some() {
        return Err(ContractError::AuctionNotYetEnded {});
    }

    let mut bid = BIDS
        .may_load(deps.storage, msg.bid_id)?
        .ok_or(ContractError::NoBidFound {})?;

    if info.sender != bid.bidder {
        return Err(ContractError::Unauthorized {});
    }

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

    BIDS.save(deps.storage, msg.bid_id, &bid)?;

    Ok(Response::new()
        .add_message(bank_msg)
        .add_attribute("action", "withdraw_bid")
        .add_attribute("bidder", info.sender))
}
/*
## Resolve auction
 - after auction time elapses this method can be executed by anyone
 - sorted bids are taken and the iteration goes backwards
 - contract is taking as much as principal amount possible and is giving the bidder the counterparty tokens
 - in case some bid is partially fulfilled - the contract is refunding the bidder unused principal amount
 - all replenished principal amount contract is sending to the principal_funds_owner
 - in case all needed principals are replenished - the iteration stops and all unspent counterparty are sent to position_created_address
*/
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
    while let Some(bid) = sorted_bids.pop() {
        // If the full principal amount has been replenished, stop processing further bids
        if principal_accumulated >= principal_target {
            break;
        }

        let bid_id = bid.bid_id;
        let bid_price = bid.price;
        let principal_deposited = bid.principal_deposited;

        // Load the bid details
        let bid = BIDS.load(deps.storage, bid_id)?;

        // Calculate the remaining principal needed
        let remaining_principal = principal_target - principal_accumulated;

        // Calculate the maximum principal we can take from this bid
        let max_principal_from_bid = principal_deposited;

        // Calculate how much principal we can take based on available counterparty
        let max_principal_based_on_counterparty = if bid_price.is_zero() {
            Decimal::from_ratio(max_principal_from_bid, Uint128::one())
        } else {
            Decimal::from_ratio(counterparty_total - counterparty_spent, Uint128::one()) / bid_price
        };

        // Round the result to the nearest integer
        let max_principal_based_on_counterparty =
            round_decimal_to_uint128(max_principal_based_on_counterparty);

        // Determine the actual principal to take
        let principal_to_take = std::cmp::min(
            remaining_principal,
            std::cmp::min(max_principal_from_bid, max_principal_based_on_counterparty),
        );

        if principal_to_take.is_zero() {
            continue;
        }

        // Calculate the corresponding counterparty tokens
        let counterparty_to_give =
            bid_price * Decimal::from_ratio(principal_to_take, Uint128::one());

        // Round the result to the nearest integer
        let truncated_counterparty_to_give = truncate_decimal_to_uint128(counterparty_to_give);

        // we only allow check to pass if bid price is zero (meaning no counterparty requested)
        if (truncated_counterparty_to_give.is_zero() && !bid_price.is_zero())
            || truncated_counterparty_to_give > (counterparty_total - counterparty_spent)
        {
            continue;
        }

        if !truncated_counterparty_to_give.is_zero() {
            // Create message to send counterparty tokens
            let counterparty_msg = BankMsg::Send {
                to_address: bid.bidder.clone().into_string(),
                amount: vec![Coin {
                    denom: state.counterparty_denom.to_string(),
                    amount: truncated_counterparty_to_give,
                }],
            };
            messages.push(counterparty_msg);
        }

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
        counterparty_spent += truncated_counterparty_to_give;

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

        BIDS.save(
            deps.storage,
            bid_id,
            &Bid {
                status: BidStatus::Processed,
                tokens_refunded: remaining_principal_in_bid,
                tokens_fulfilled: truncated_counterparty_to_give,
                ..bid
            },
        )?;
    }

    // Send remaining counterparty tokens back to the project
    let counterparty_to_project = counterparty_total
        .checked_sub(counterparty_spent)
        .unwrap_or(Uint128::zero());

    if !counterparty_to_project.is_zero() {
        let send_back_msg = BankMsg::Send {
            to_address: state.counterparty_owner.clone().unwrap().into_string(),
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

/// Rounds a Decimal to the nearest Uint128 (half up)
pub fn round_decimal_to_uint128(decimal: Decimal) -> Uint128 {
    let atomics = decimal.atomics().u128();
    let base = 10u128.pow(18);

    let rounded = (atomics + base / 2) / base;
    Uint128::new(rounded)
}

/// Truncates a Decimal to the nearest lower Uint128
pub fn truncate_decimal_to_uint128(decimal: Decimal) -> Uint128 {
    let atomics = decimal.atomics().u128();
    let base = 10u128.pow(18);

    let truncated = atomics / base;
    Uint128::new(truncated)
}

pub fn query_get_state(deps: Deps) -> StdResult<StateResponse> {
    // Load the current state from storage
    let state = STATE.load(deps.storage)?;

    // Build the StateResponse using the loaded state
    let response = StateResponse {
        position_admin: state.position_admin,
        counterparty_owner: state.counterparty_owner,
        principal_funds_owner: state.principal_funds_owner.into_string(),
        pool_id: state.pool_id,
        counterparty_denom: state.counterparty_denom.clone(),
        principal_denom: state.principal_denom.clone(),
        principal_first: state.principal_first,
        position_id: state.position_id,
        initial_principal_amount: state.initial_principal_amount,
        initial_counterparty_amount: state.initial_counterparty_amount,
        liquidity_shares: state.liquidity_shares,
        auction_end_time: state.auction_end_time,
        principal_to_replenish: state.principal_to_replenish,
        counterparty_to_give: state.counterparty_to_give,
        auction_principal_deposited: state.auction_principal_deposited,
        position_rewards: state.position_rewards,
        round_end_time: state.round_end_time,
    };

    Ok(response)
}

pub fn query_get_bids(deps: Deps, start_from: u32, limit: u32) -> StdResult<BidsResponse> {
    let bids: Vec<BidResponse> = BIDS
        .range(deps.storage, None, None, Order::Ascending)
        .skip(start_from as usize)
        .take(limit as usize)
        .filter_map(|item| item.ok())
        .map(|(bid_id, bid)| BidResponse { bid_id, bid }) // <- convert tuple to struct
        .collect();

    Ok(BidsResponse { bids })
}

pub fn query_bid(deps: Deps, bid_id: u64) -> StdResult<BidResponse> {
    Ok(BidResponse {
        bid_id,
        bid: BIDS.load(deps.storage, bid_id)?,
    })
}

pub fn query_sorted_bids(deps: Deps) -> StdResult<SortedBidsResponse> {
    let sorted_bids = SORTED_BIDS.load(deps.storage).unwrap_or_default();
    Ok(SortedBidsResponse { sorted_bids })
}

pub fn query_is_liquidatable(deps: Deps) -> StdResult<IsLiquidatableResponse> {
    let state = STATE
        .load(deps.storage)
        .map_err(|e| StdError::generic_err(format!("State load failed: {e}")))?;

    let position = ConcentratedliquidityQuerier::new(&deps.querier)
        .position_by_id(
            state
                .position_id
                .ok_or_else(|| StdError::generic_err("Missing position_id in state"))?,
        )
        .map_err(|_| StdError::generic_err("Position query failed"))?
        .position
        .ok_or_else(|| StdError::not_found("Position"))?;

    let principal_asset = if state.principal_first {
        position.asset0
    } else {
        position.asset1
    };

    let principal_asset = principal_asset.ok_or_else(|| StdError::not_found("Principal Asset"))?;
    let principal_amount = principal_asset.amount.clone();

    Ok(IsLiquidatableResponse {
        liquidatable: principal_amount == "0",
    })
}
pub fn query_simulate_liquidation(
    deps: Deps,
    principal_input: Uint128,
) -> StdResult<SimulateLiquidationResponse> {
    let state = STATE
        .load(deps.storage)
        .map_err(|e| StdError::generic_err(format!("State load failed: {e}")))?;

    let position = ConcentratedliquidityQuerier::new(&deps.querier)
        .position_by_id(
            state
                .position_id
                .ok_or_else(|| StdError::generic_err("Missing position_id in state"))?,
        )
        .map_err(|_| StdError::generic_err("Position query failed"))?
        .position
        .ok_or_else(|| StdError::not_found("Position"))?;

    let (principal_asset, counterparty_asset) = if state.principal_first {
        (position.asset0, position.asset1)
    } else {
        (position.asset1, position.asset0)
    };

    let principal_asset = principal_asset.ok_or_else(|| StdError::not_found("Principal Asset"))?;
    let counterparty_asset =
        counterparty_asset.ok_or_else(|| StdError::not_found("Counterparty Asset"))?;
    let principal_amount = principal_asset.amount.clone();

    let counterparty_amount = Uint128::from_str(&counterparty_asset.amount)
        .map_err(|_| StdError::generic_err("Invalid counterparty asset amount"))?;

    // Check if asset1_amount is non-zero
    // if it's zero - price went below lower tick (since principal token amount is zero)
    if principal_amount != "0" {
        return Ok(SimulateLiquidationResponse {
            counterparty_to_receive: "Position is not liquidatable".to_string(),
        });
    }
    // Convert base_amount and initial_base_amount to Decimal for precise division
    let principal_input = Decimal::from_atomics(principal_input, 0)
        .map_err(|_| ContractError::InvalidConversion {})
        .unwrap();
    let principal_amount_to_replenish = Decimal::from_atomics(state.principal_to_replenish, 0)
        .map_err(|_| ContractError::InvalidConversion {})
        .unwrap();
    let counterparty_available = Decimal::from_atomics(counterparty_amount, 0)
        .map_err(|_| ContractError::InvalidConversion {})
        .unwrap();

    // Ensure the supplied amount is not greater than the initial amount
    if principal_input > principal_amount_to_replenish {
        return Ok(SimulateLiquidationResponse {
            counterparty_to_receive: "Excessive liquidation amount".to_string(),
        });
    }

    // Calculate percentage to liquidate
    let perc_to_liquidate = principal_input / principal_amount_to_replenish;

    let counterparty_to_liquidate = counterparty_available * perc_to_liquidate;

    let counterparty_to_liquidate = round_decimal_to_uint128(counterparty_to_liquidate);

    // Return as string
    let counterparty_str = counterparty_to_liquidate.to_string();

    Ok(SimulateLiquidationResponse {
        counterparty_to_receive: counterparty_str,
    })
}
