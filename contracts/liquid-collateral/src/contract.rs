use cosmwasm_std::{
    to_binary, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdResult,
    entry_point, Uint128, coin, coins,
};
use osmosis_std::types::osmosis::concentratedliquidity::v1beta1::{
    MsgCreatePosition, PositionByIdRequest,
};
use osmosis_std::types::cosmos::base::v1beta1::Coin;

use crate::error::ContractError;
use crate::msg::{ExecuteMsg, InstantiateMsg, QueryMsg, StateResponse, CreatePositionMsg};
use crate::state::{State, STATE};

const MIN_TICK: i32 = -1_000_000;
const MAX_TICK: i32 = 1_000_000;

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let state = State {
        owner: info.sender.clone(),
        pool_id: msg.pool_id,
        position_id: None,
        token0_denom: msg.token0_denom,
        token1_denom: msg.token1_denom,
        initial_token0_amount: Uint128::zero(),
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
        ExecuteMsg::CreatePosition(msg) => execute_create_position(deps, env, info, msg),
        ExecuteMsg::WithdrawPosition {} => execute_withdraw_position(deps, env, info),
    }
}

pub fn execute_create_position(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: CreatePositionMsg,
) -> Result<Response, ContractError> {
    let state = STATE.load(deps.storage)?;
    
    // Only owner can create position
    if info.sender != state.owner {
        return Err(ContractError::Unauthorized {});
    }

    // Validate ticks
    if msg.lower_tick < MIN_TICK || msg.upper_tick > MAX_TICK || msg.lower_tick >= msg.upper_tick {
        return Err(ContractError::InvalidTickRange {});
    }

    // Validate funds sent
    let token0 = info.funds.iter().find(|c| c.denom == state.token0_denom);
    let token1 = info.funds.iter().find(|c| c.denom == state.token1_denom);

    if token0.is_none() || token1.is_none() {
        return Err(ContractError::InsufficientFunds {});
    }

    let token0_amount = token0.unwrap().amount;
    let token1_amount = token1.unwrap().amount;

    if token0_amount != msg.token0_amount || token1_amount != msg.token1_amount {
        return Err(ContractError::InsufficientFunds {});
    }

    // Create position message
    let create_position_msg = MsgCreatePosition {
        pool_id: state.pool_id,
        sender: env.contract.address.to_string(),
        lower_tick: msg.lower_tick,
        upper_tick: msg.upper_tick,
        tokens_provided: vec![
            Coin {
                denom: state.token0_denom.clone(),
                amount: token0_amount.to_string(),
            },
            Coin {
                denom: state.token1_denom.clone(),
                amount: token1_amount.to_string(),
            },
        ],
        token_min_amount0: token0_amount.to_string(),
        token_min_amount1: token1_amount.to_string(),
    };

    Ok(Response::new()
        .add_message(create_position_msg)
        .add_attribute("action", "create_position")
        .add_attribute("pool_id", state.pool_id.to_string())
        .add_attribute("lower_tick", msg.lower_tick.to_string())
        .add_attribute("upper_tick", msg.upper_tick.to_string()))
}

pub fn execute_withdraw_position(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    let state = STATE.load(deps.storage)?;
    
    // Only owner can withdraw position
    if info.sender != state.owner {
        return Err(ContractError::Unauthorized {});
    }

    // Ensure position exists
    let position_id = state.position_id.ok_or(ContractError::NoPosition {})?;

    // Create withdraw message
    let withdraw_msg = osmosis_std::types::osmosis::concentratedliquidity::v1beta1::MsgWithdrawPosition {
        position_id,
        sender: info.sender.to_string(),
        liquidity_amount: "0".to_string(), // Withdraw all liquidity
    };

    Ok(Response::new()
        .add_message(withdraw_msg)
        .add_attribute("action", "withdraw_position")
        .add_attribute("position_id", position_id.to_string()))
}

#[entry_point]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::GetState {} => to_binary(&query_state(deps)?),
    }
}

fn query_state(deps: Deps) -> StdResult<StateResponse> {
    let state = STATE.load(deps.storage)?;
    Ok(StateResponse {
        owner: state.owner.to_string(),
        pool_id: state.pool_id,
        position_id: state.position_id,
        token0_denom: state.token0_denom,
        token1_denom: state.token1_denom,
        initial_token0_amount: state.initial_token0_amount,
    })
} 