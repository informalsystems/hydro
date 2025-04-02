use crate::error::ContractError;
use crate::msg::{CreatePositionMsg, ExecuteMsg, InstantiateMsg, QueryMsg, StateResponse};
use crate::state::{State, STATE};
use cosmwasm_std::{
    entry_point, to_binary, to_json_binary, Binary, CosmosMsg, Decimal, Deps, DepsMut, Env, Event,
    MessageInfo, QueryResponse, Reply, Response, StdResult, SubMsg, Uint128,
};
use osmosis_std::types::cosmos::bank::v1beta1::MsgSend;
use osmosis_std::types::cosmos::base::v1beta1::Coin;
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
        owner: info.sender.clone(),
        pool_id: msg.pool_id,
        position_id: None,
        principal_denom: msg.principal_denom,
        counterparty_denom: msg.counterparty_denom,
        initial_principal_amount: Uint128::zero(),
        initial_counterparty_amount: Uint128::zero(),
        liquidity_shares: None,
        liquidator_address: None,
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
    }
}

#[entry_point]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        // Handle the GetState query
        QueryMsg::GetState {} => to_binary(&query_get_state(deps)?),
    }
}

pub fn calculate_position(deps: DepsMut, env: Env, info: MessageInfo, msg: CreatePositionMsg) {
    // inputs: lower tick, base token amount, and liquidation bonus
    // output:  upper tick and counterparty
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
    };

    to_json_binary(&response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::mock::{store_contracts_code, PoolMockup};
    use cosmwasm_std::{from_binary, from_json, from_slice, Coin};
    use osmosis_std::types::cosmos::bank::v1beta1::{BankQuerier, QueryBalanceRequest};
    use osmosis_test_tube::{Bank, Module, Wasm};

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

        let instantiate_msg = InstantiateMsg {
            pool_id: 1,
            counterparty_denom: USDC_DENOM.to_owned(),
            principal_denom: OSMO_DENOM.to_owned(),
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
            counterparty_token_min_amount: 85000u128.into(),
            principal_token_min_amount: 100000u128.into(),
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

        let position_response = pool_mockup.position_query(1);

        let query_msg = QueryMsg::GetState {};

        let query_result: Binary = wasm.query(&contract_addr, &query_msg).unwrap();
        let state_response: StateResponse = from_json(&query_result).unwrap();
        let formatted_output = serde_json::to_string_pretty(&state_response).unwrap();

        // Print the state response
        println!("{}", formatted_output);

        let liquidity = if let Ok(full_position) = position_response {
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
            println!("USDC Amount pre withdrawal: {}", asset0_amount);
            println!("OSMO Amount pre withdrawal: {}", asset1_amount);

            // Print claimable spread rewards
            println!("Claimable Spread Rewards:");
            for reward in &full_position.claimable_spread_rewards {
                let denom = &reward.denom;
                let amount = &reward.amount;
                println!("Denom: {}, Amount: {}", denom, amount);
            }
            if let Some(position) = full_position.position {
                println!("Liquidity pre withdrawal: {}", position.liquidity); // Print the value
                position.liquidity // Return liquidity
            } else {
                println!("Position not found");
                String::from("0") // Default value
            }
        } else {
            println!("Failed to get position response");
            String::from("0") // Default value
        };

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
        let swap_response = pool_mockup.swap_usdc_for_osmo(&pool_mockup.user1, usdc_needed, 1);
        let position_response = pool_mockup.position_query(1);
        let liquidity = if let Ok(full_position) = position_response {
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
            println!("USDC Amount after swap: {}", asset0_amount);
            println!("OSMO Amount after swap: {}", asset1_amount);

            // Print claimable spread rewards
            println!("Claimable Spread Rewards:");
            for reward in &full_position.claimable_spread_rewards {
                let denom = &reward.denom;
                let amount = &reward.amount;
                println!("Denom: {}, Amount: {}", denom, amount);
            }
        } else {
            println!("Failed to get position response");
        };

        //92195444572928873195000
        let liquidate_msg = ExecuteMsg::Liquidate;

        //100000
        let coins = &[Coin::new(100000u128, OSMO_DENOM)];

        let response = wasm
            .execute(&contract_addr, &liquidate_msg, coins, &pool_mockup.user1)
            .expect("Execution failed");
        //println!("Execution successful: {:?}", response);
        //println!("{:?}", response.events);

        let position_response = pool_mockup.position_query(1);
        //println!("{:#?}", position_response);
        let liquidity = if let Ok(full_position) = position_response {
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
            println!("USDC Amount after withdrawal: {}", asset0_amount);
            println!("OSMO Amount after withdrawal: {}", asset1_amount);
            if let Some(position) = full_position.position {
                println!("Liquidity after withdrawal: {}", position.liquidity); // Print the value
                position.liquidity // Return liquidity
            } else {
                println!("Position not found");
                String::from("0") // Default value
            }
        } else {
            println!("Failed to get position response");
            String::from("0") // Default value
        };

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
}
