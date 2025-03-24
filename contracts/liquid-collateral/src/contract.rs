use cosmwasm_std::{
    to_json_binary, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdResult,
    entry_point, Uint128, CosmosMsg, SubMsg, Reply,
};
use osmosis_std::types::osmosis::concentratedliquidity::v1beta1::{MsgCreatePosition, MsgCreatePositionResponse};
use osmosis_std::types::cosmos::base::v1beta1::Coin;

use crate::error::ContractError;
use crate::msg::{ExecuteMsg, InstantiateMsg, QueryMsg, StateResponse, CreatePositionMsg};
use crate::state::{State, STATE};

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

    // Wrap in SubMsg to handle response
    let submsg = SubMsg::reply_on_success(create_position_msg, 1);

    Ok(Response::new()
        .add_submessage(submsg)
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

    // Wrap in CosmosMsg::Stargate
    let msg = CosmosMsg::Stargate { 
        type_url: "/osmosis.concentratedliquidity.v1beta1.MsgWithdrawPosition".to_string(),
        value: to_json_binary(&withdraw_msg)?,
    };

    Ok(Response::new()
        .add_message(msg)
        .add_attribute("action", "withdraw_position")
        .add_attribute("position_id", position_id.to_string()))
}

#[entry_point]
pub fn reply(deps: DepsMut, _env: Env, msg: Reply) -> Result<Response, ContractError> {
    match msg.id {
        1 => handle_create_position_reply(deps, msg),
        _ => Err(ContractError::UnknownReplyId { id: msg.id }),
    }
}

fn handle_create_position_reply(deps: DepsMut, msg: Reply) -> Result<Response, ContractError> {
    let response: MsgCreatePositionResponse = msg.result.try_into()?;
    
    // Update state with new position ID
    STATE.update(deps.storage, |mut state| -> Result<_, ContractError> {
        state.position_id = Some(response.position_id);
        Ok(state)
    })?;

    Ok(Response::new()
        .add_attribute("position_id", response.position_id.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
    use osmosis_std::types::osmosis::concentratedliquidity::v1beta1::MsgCreatePosition;
    // use crate::mock::mock::{PoolMockup, ContractInfo, ATOM_DENOM, OSMO_DENOM};
    use osmosis_std::types::osmosis::concentratedliquidity::v1beta1::MsgCreatePositionResponse;
    use cosmwasm_std::Coin;
    use osmosis_test_tube::{Account, Module, OsmosisTestApp, Gamm};

    

    #[test]
    fn test_create_position() {
        let app = OsmosisTestApp::default();
        let alice = app
            .init_account(&[
                Coin::new(1_000_000_000_000u128, "uatom"),
                Coin::new(1_000_000_000_000u128, "uosmo"),
            ])
            .unwrap();
        
        // create Gamm Module Wrapper
        let gamm = Gamm::new(&app);
        
        // create balancer pool with basic configuration
        let pool_liquidity = vec![Coin::new(1_000u128, "uatom"), Coin::new(1_000u128, "uosmo")];
        let pool_id = gamm
            .create_basic_pool(&pool_liquidity, &alice)
            .unwrap()
            .data
            .pool_id;
        
        // query pool and assert if the pool is created successfully
        let pool = gamm.query_pool(pool_id).unwrap();
        assert_eq!(
            pool_liquidity,
            pool.pool_assets
                .into_iter()
                .map(|a| cosmwasm_std::Coin {
                    denom: a.token.clone().unwrap().denom,
                    amount: a.token.unwrap().amount.parse().unwrap()
                })
                .collect::<Vec<Coin>>()
        );
    }

    // #[test]
    // fn test_instantiate() {
    //     let mut deps = mock_dependencies();
    //     let env = mock_env();
    //     let info = mock_info("creator", &[]);
    //     let msg = InstantiateMsg {
    //         pool_id: 1,
    //         token0_denom: ATOM_DENOM.to_string(),
    //         token1_denom: OSMO_DENOM.to_string(),
    //     };

    //     // Test instantiation
    //     let res = instantiate(deps.as_mut(), env, info, msg).unwrap();
    //     assert_eq!(0, res.messages.len());

    //     // Test state
    //     let state = STATE.load(&deps.storage).unwrap();
    //     assert_eq!(state.pool_id, 1);
    //     assert_eq!(state.token0_denom, ATOM_DENOM);
    //     assert_eq!(state.token1_denom, OSMO_DENOM);
    //     assert_eq!(state.position_id, None);
    // }

    // #[test]
    // fn test_create_position() {
    //     // Set up test environment
    //     let pool_mockup = PoolMockup::new(100_000, 200_000); // Initial pool liquidity
    //     let contract = ContractInfo::new(&pool_mockup);

    //     // Create a position
    //     let res = contract.create_position(
    //         &pool_mockup,
    //         -1000,  // lower tick
    //         1000,   // upper tick
    //         10_000, // ATOM amount
    //         20_000, // OSMO amount
    //         &pool_mockup.user1,
    //     );

    //     // Verify the response
    //     let create_position_response: MsgCreatePositionResponse = res.data.unwrap().try_into().unwrap();
    //     assert!(create_position_response.position_id > 0);

    //     // Verify the user's balances were deducted
    //     let bank = osmosis_test_tube::Bank::new(&pool_mockup.app);
        
    //     let atom_balance = bank.query_balance(&osmosis_std::types::cosmos::bank::v1beta1::QueryBalanceRequest {
    //         address: pool_mockup.user1.address(),
    //         denom: ATOM_DENOM.into(),
    //     }).unwrap().balance.unwrap();
        
    //     let osmo_balance = bank.query_balance(&osmosis_std::types::cosmos::bank::v1beta1::QueryBalanceRequest {
    //         address: pool_mockup.user1.address(),
    //         denom: OSMO_DENOM.into(),
    //     }).unwrap().balance.unwrap();

    //     // Initial balance was 1_000_000_000_000, we spent 10_000 and 20_000 respectively
    //     assert_eq!(atom_balance.amount, (1_000_000_000_000u128 - 10_000).to_string());
    //     assert_eq!(osmo_balance.amount, (1_000_000_000_000u128 - 20_000).to_string());
    }