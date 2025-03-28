use cosmwasm_std::{
    entry_point, to_json_binary, Binary, CosmosMsg, Deps, DepsMut, Env, MessageInfo, Reply,
    Response, StdResult, SubMsg, Uint128,
};
use osmosis_std::types::cosmos::base::v1beta1::Coin;
use osmosis_std::types::osmosis::concentratedliquidity::v1beta1::{
    MsgCreatePosition, MsgCreatePositionResponse,
};
use osmosis_std::types::osmosis::poolmanager::v1beta1::PoolmanagerQuerier;

use crate::error::ContractError;
use crate::msg::{
    CreatePositionMsg, ExecuteMsg, InstantiateMsg, QueryMsg, StateResponse, WithdrawPositionMsg,
};
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
        base_denom: msg.base_denom,
        counterparty_denom: msg.counterparty_denom,
        initial_base_amount: Uint128::zero(),
        initial_counterparty_amount: Uint128::zero(),
        threshold: None,
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
        ExecuteMsg::WithdrawPosition(msg) => execute_withdraw_position(deps, env, info, msg),
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
    let counterparty = info
        .funds
        .iter()
        .find(|c| c.denom == state.counterparty_denom);
    let base = info.funds.iter().find(|c| c.denom == state.base_denom);

    if counterparty.is_none() || base.is_none() {
        return Err(ContractError::InsufficientFunds {});
    }

    let counterparty_amount = counterparty.unwrap().amount;
    let base_amount = base.unwrap().amount;

    if counterparty_amount != msg.counterparty_token_amount || base_amount != msg.base_token_amount
    {
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
                denom: state.counterparty_denom.clone(),
                amount: counterparty_amount.to_string(),
            },
            Coin {
                denom: state.base_denom.clone(),
                amount: base_amount.to_string(),
            },
        ],
        token_min_amount0: counterparty_amount.to_string(),
        token_min_amount1: base_amount.to_string(),
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
    msg: WithdrawPositionMsg,
) -> Result<Response, ContractError> {
    let state = STATE.load(deps.storage)?;

    // Query the current spot price
    let ratio = PoolmanagerQuerier::new(&deps.querier)
        .spot_price(
            state.pool_id,
            state.counterparty_denom.clone(),
            state.base_denom.clone(),
        )
        .map_err(|_| ContractError::PriceQueryFailed {})? // Handle query errors
        .spot_price;

    // Check if the ratio is lower than the threshold
    if let Some(threshold) = state.threshold {
        let ratio: f64 = ratio
            .parse::<f64>()
            .map_err(|_| ContractError::InvalidRatioFormat {})?;

        if ratio >= threshold {
            return Err(ContractError::ThresholdNotMet {});
        }
    }

    // Create withdraw message
    let withdraw_msg =
        osmosis_std::types::osmosis::concentratedliquidity::v1beta1::MsgWithdrawPosition {
            position_id: state.position_id.unwrap(),
            sender: _env.contract.address.to_string(),
            liquidity_amount: msg.liquidity_amount,
        };

    Ok(Response::new()
        .add_message(withdraw_msg)
        .add_attribute("action", "withdraw_position"))
}

#[entry_point]
pub fn reply(deps: DepsMut, _env: Env, msg: Reply) -> Result<Response, ContractError> {
    match msg.id {
        1 => handle_create_position_reply(deps, msg),
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

    // Save the updated state back to storage
    STATE.save(deps.storage, &state)?;

    Ok(Response::new().add_attribute("position_id", response.position_id.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::mock::{store_contracts_code, PoolMockup};
    use cosmwasm_std::Coin;
    use osmosis_test_tube::{Module, Wasm};

    pub const USDC_DENOM: &str =
        "ibc/498A0751C798A0D9A389AA3691123DADA57DAA4FE165D5C75894505B876BA6E4";
    pub const OSMO_DENOM: &str = "uosmo";

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
         */
        let pool_mockup = PoolMockup::new();

        let wasm = Wasm::new(&pool_mockup.app);
        let code_id = store_contracts_code(&wasm, &pool_mockup.deployer);

        let instantiate_msg = InstantiateMsg {
            pool_id: 1,
            counterparty_denom: USDC_DENOM.to_owned(),
            base_denom: OSMO_DENOM.to_owned(),
        };

        // liquid-collateral contract instantiation
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

        let msg = ExecuteMsg::CreatePosition(CreatePositionMsg {
            lower_tick: -108000000,
            upper_tick: 342000000,
            counterparty_token_amount: 85000u128.into(),
            base_token_amount: 100000u128.into(),
        });

        let coins = &[
            Coin::new(85000u128, USDC_DENOM),
            Coin::new(100000u128, OSMO_DENOM),
        ];

        //deployer enters first position
        let response = wasm
            .execute(&contract_addr, &msg, coins, &pool_mockup.deployer)
            .expect("Execution failed");
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

        let msg = ExecuteMsg::CreatePosition(CreatePositionMsg {
            lower_tick: -108000000,
            upper_tick: 342000000,
            counterparty_token_amount: 85000u128.into(),
            base_token_amount: 100000u128.into(),
        });

        let coins = &[
            Coin::new(85000u128, USDC_DENOM),
            Coin::new(100000u128, OSMO_DENOM),
        ];

        //deployer enters second position
        let response = wasm
            .execute(&contract_addr, &msg, coins, &pool_mockup.deployer)
            .expect("Execution failed");
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
        //println!("{:#?}", position_response);
        let liquidity = if let Ok(full_position) = position_response {
            if let Some(position) = full_position.position {
                println!("Liquidity: {}", position.liquidity); // Print the value
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
        let withdraw_msg = ExecuteMsg::WithdrawPosition(WithdrawPositionMsg {
            liquidity_amount: "92195444572928873195000".to_string(),
        });
        let response = wasm
            .execute(&contract_addr, &withdraw_msg, &[], &pool_mockup.user1)
            .expect("Execution failed");
        //println!("Execution successful: {:?}", response);

        let position_response = pool_mockup.position_query(1);
        //println!("{:#?}", position_response);
        let liquidity = if let Ok(full_position) = position_response {
            if let Some(position) = full_position.position {
                println!("Liquidity: {}", position.liquidity); // Print the value
                position.liquidity // Return liquidity
            } else {
                println!("Position not found");
                String::from("0") // Default value
            }
        } else {
            println!("Failed to get position response");
            String::from("0") // Default value
        };
    }
}
