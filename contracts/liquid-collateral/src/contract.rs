use cosmwasm_std::{
    entry_point, to_json_binary, Binary, CosmosMsg, Deps, DepsMut, Env, MessageInfo, Reply,
    Response, StdResult, SubMsg, Uint128,
};
use osmosis_std::types::cosmos::base::v1beta1::Coin;
use osmosis_std::types::osmosis::concentratedliquidity::v1beta1::{
    MsgCreatePosition, MsgCreatePositionResponse,
};

use crate::error::ContractError;
use crate::msg::{CreatePositionMsg, ExecuteMsg, InstantiateMsg, QueryMsg, StateResponse};
use crate::state::{State, STATE};

#[entry_point]
pub fn instantiate(
    _deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    _msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    Ok(Response::new()
        .add_attribute("method", "instantiate")
        .add_attribute("owner", info.sender))
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
    // Hardcoded denoms for token0 and token1
    let token0_denom =
        "ibc/498A0751C798A0D9A389AA3691123DADA57DAA4FE165D5C75894505B876BA6E4".to_string();
    let token1_denom = "uosmo".to_string();
    // Validate funds sent
    let token0 = info.funds.iter().find(|c| c.denom == token0_denom);
    let token1 = info.funds.iter().find(|c| c.denom == token1_denom);

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
        pool_id: msg.pool_id,
        sender: env.contract.address.to_string(),
        lower_tick: msg.lower_tick,
        upper_tick: msg.upper_tick,
        tokens_provided: vec![
            Coin {
                denom: token0_denom,
                amount: token0_amount.to_string(),
            },
            Coin {
                denom: token1_denom,
                amount: token1_amount.to_string(),
            },
        ],
        token_min_amount0: "85000".to_string(),
        token_min_amount1: "24978".to_string(),
    };

    // Wrap in SubMsg to handle response
    let submsg = SubMsg::reply_on_success(create_position_msg, 1);

    Ok(Response::new()
        .add_submessage(submsg)
        .add_attribute("action", "create_position")
        .add_attribute("pool_id", msg.pool_id.to_string())
        .add_attribute("lower_tick", msg.lower_tick.to_string())
        .add_attribute("upper_tick", msg.upper_tick.to_string()))
}

pub fn execute_withdraw_position(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    // Create withdraw message
    let withdraw_msg =
        osmosis_std::types::osmosis::concentratedliquidity::v1beta1::MsgWithdrawPosition {
            position_id: 2,
            sender: _env.contract.address.to_string(),
            liquidity_amount: "92195444572928873195000".to_string(), // Withdraw all liquidity
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
    let response: MsgCreatePositionResponse = msg.result.try_into()?;

    Ok(Response::new().add_attribute("position_id", response.position_id.to_string()))
}

#[cfg(test)]
mod tests {
    use crate::mock::{
        self,
        mock::{store_contracts_code, PoolMockup},
    };

    use super::*;
    use cosmwasm_std::{
        coins,
        testing::{mock_dependencies, mock_env, mock_info},
    };
    use osmosis_std::types::osmosis::concentratedliquidity::v1beta1::MsgCreatePosition;
    // use crate::mock::mock::{PoolMockup, ContractInfo, ATOM_DENOM, OSMO_DENOM};
    use cosmwasm_std::Coin;
    use osmosis_std::types::osmosis::concentratedliquidity::v1beta1::MsgCreatePositionResponse;
    use osmosis_test_tube::{Account, Gamm, Module, OsmosisTestApp, Wasm};

    #[test]
    fn test_create_and_withdraw_position_in_pool() {
        pub const USDC_DENOM: &str =
            "ibc/498A0751C798A0D9A389AA3691123DADA57DAA4FE165D5C75894505B876BA6E4";
        pub const OSMO_DENOM: &str = "uosmo";
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

        let instantiate_msg = InstantiateMsg {};

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
            pool_id: 1,
            lower_tick: -108000000,
            upper_tick: 342000000,
            token0_amount: 85000u128.into(),
            token1_amount: 100000u128.into(),
        });

        let coins = &[
            Coin::new(85000u128, USDC_DENOM),
            Coin::new(100000u128, OSMO_DENOM),
        ];

        //deployer enters position
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
            pool_id: 1,
            lower_tick: -108000000,
            upper_tick: 342000000,
            token0_amount: 85000u128.into(),
            token1_amount: 100000u128.into(),
        });

        let coins = &[
            Coin::new(85000u128, USDC_DENOM),
            Coin::new(100000u128, OSMO_DENOM),
        ];

        //user 1 enters position
        let response = wasm
            .execute(&contract_addr, &msg, coins, &pool_mockup.user1)
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
            pool_id: 1,
            lower_tick: -108000000,
            upper_tick: 342000000,
            token0_amount: 85000u128.into(),
            token1_amount: 100000u128.into(),
        });

        let coins = &[
            Coin::new(85000u128, USDC_DENOM),
            Coin::new(100000u128, OSMO_DENOM),
        ];

        //user2 enters position
        let response = wasm
            .execute(&contract_addr, &msg, coins, &pool_mockup.user2)
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

        let withdraw_msg = ExecuteMsg::WithdrawPosition {};
        let response = wasm
            .execute(&contract_addr, &withdraw_msg, &[], &pool_mockup.user2)
            .expect("Execution failed");
        //println!("Execution successful: {:?}", response);

        let position_response = pool_mockup.position_query(2);
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
