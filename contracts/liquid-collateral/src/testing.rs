#![cfg(test)]
use crate::calculations::{
    calc_amount0, calc_amount1, calc_final_amount_at_lower_tick_with_bonus,
    calc_required_token1_with_bonus, tick_to_sqrt_price,
};
use crate::mock::{store_contracts_code, PoolMockup};
use crate::msg::{
    BidsResponse, CreatePositionMsg, ExecuteMsg, InstantiateMsg, PlaceBidMsg, QueryMsg,
    SortedBidsResponse, StateResponse,
};
use crate::state::{Bid, BidStatus, SortedBid, State, BIDS, SORTED_BIDS, STATE};
use crate::ContractError;
use bigdecimal::BigDecimal;
use cosmwasm_std::{
    testing::{mock_dependencies, mock_env, mock_info},
    Addr, Coin, Decimal, Uint128,
};
use cosmwasm_std::{Binary, Reply, SubMsgResponse, SubMsgResult};
use osmosis_std::types::cosmos::base::v1beta1::Coin as OsmosisCoin;
use osmosis_std::types::osmosis::concentratedliquidity::v1beta1::MsgWithdrawPositionResponse;
use osmosis_std::types::osmosis::poolmanager::v1beta1::SpotPriceRequest;
use osmosis_std::types::{
    cosmos::bank::v1beta1::QueryBalanceRequest, cosmwasm::wasm::v1::MsgExecuteContractResponse,
    osmosis::concentratedliquidity::v1beta1::FullPositionBreakdown,
};
use osmosis_test_tube::{
    Account, Bank, ExecuteResponse, Module, OsmosisTestApp, PoolManager, SigningAccount, Wasm,
};
use prost::Message;
use std::str::FromStr;

use crate::contract::{
    calculate_withdraw_liquidity_amount, fetch_all_rewards,
    handle_withdraw_position_end_round_reply, parse_liquidity, resolve_auction,
};

pub const USDC_DENOM: &str = "ibc/498A0751C798A0D9A389AA3691123DADA57DAA4FE165D5C75894505B876BA6E4";
pub const OSMO_DENOM: &str = "uosmo";

pub const HUNDRED_SECONDS: u64 = 100;
pub const FIVE_SECONDS: u64 = 5;

pub fn instantiate(
    wasm: &Wasm<OsmosisTestApp>, // Borrow wasm reference
    pool_mockup: &PoolMockup,
    code_id: u64,
    principal_first: bool, // Borrow pool_mockup reference
) -> String {
    // Return only the contract address as a String

    let instantiate_msg = InstantiateMsg {
        pool_id: 1,
        counterparty_denom: USDC_DENOM.to_owned(),
        principal_denom: OSMO_DENOM.to_owned(),
        round_duration: FIVE_SECONDS,
        principal_funds_owner: pool_mockup.principal_funds_owner.address(),
        position_admin: None,
        counterparty_owner: None,
        auction_duration: HUNDRED_SECONDS,
        principal_first,
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
    coins: &[Coin],
    msg: CreatePositionMsg,
) -> ExecuteResponse<MsgExecuteContractResponse> {
    let exec_msg = ExecuteMsg::CreatePosition(msg.clone());

    wasm.execute(contract_addr, &exec_msg, coins, deployer)
        .expect("Execution failed")
}

pub fn print_position_details(full_position: &FullPositionBreakdown) {
    // Extract asset0 details safely
    let (asset0_amount, asset0_denom) = full_position
        .asset0
        .as_ref()
        .map(|coin| (coin.amount.clone(), coin.denom.clone()))
        .unwrap_or_else(|| (String::from("0"), String::from("unknown")));

    // Extract asset1 details safely
    let (asset1_amount, asset1_denom) = full_position
        .asset1
        .as_ref()
        .map(|coin| (coin.amount.clone(), coin.denom.clone()))
        .unwrap_or_else(|| (String::from("0"), String::from("unknown")));

    // Print asset details
    println!("Asset0: {} {}", asset0_amount, asset0_denom);
    println!("Asset1: {} {}", asset1_amount, asset1_denom);

    // Print claimable spread rewards
    println!("Claimable Spread Rewards:\n");
    for reward in &full_position.claimable_spread_rewards {
        let denom = &reward.denom;
        let amount = &reward.amount;
        println!("Denom: {}, Amount: {}", denom, amount);
    }

    // Print claimable incentives
    println!("\nClaimable Incentives:");
    for incentive in &full_position.claimable_incentives {
        println!("  Denom: {}, Amount: {}", incentive.denom, incentive.amount);
    }

    // Print forfeited incentives
    println!("\nForfeited Incentives:");
    for forfeited in &full_position.forfeited_incentives {
        println!("  Denom: {}, Amount: {}", forfeited.denom, forfeited.amount);
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
    // Create a mock initial principal amount
    let initial_principal_amount = Uint128::new(100000); // Example initial principal amount
                                                         // Convert base_amount and initial_base_amount to Decimal for precise division
    let principal_amount =
        Decimal::from_atomics(coin.amount, 0).map_err(|_| ContractError::InvalidConversion {});
    let initial_principal_amount = Decimal::from_atomics(initial_principal_amount, 0)
        .map_err(|_| ContractError::InvalidConversion {});
    let mock_liquidity_shares = Some("92195444572928873195000".to_string());

    let liquidity_shares = &mock_liquidity_shares
        .as_deref() // Converts Option<String> to Option<&str>
        .unwrap_or("0"); // Default value if None

    let liquidity_shares_decimal = parse_liquidity(liquidity_shares);

    let result = calculate_withdraw_liquidity_amount(
        principal_amount.unwrap(),
        initial_principal_amount.unwrap(),
        liquidity_shares_decimal.unwrap(),
    );

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Uint128::new(3072874167615719343589));
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
    let pool_mockup = PoolMockup::new(USDC_DENOM.to_string(), OSMO_DENOM.to_string());

    let wasm = Wasm::new(&pool_mockup.app);
    let code_id = store_contracts_code(&wasm, &pool_mockup.deployer);
    let contract_addr = instantiate(&wasm, &pool_mockup, code_id, false);
    /*
    let pm = PoolManager::new(&pool_mockup.app);
    let request = SpotPriceRequest {
        pool_id: 1,
        base_asset_denom: USDC_DENOM.to_string(),
        quote_asset_denom: OSMO_DENOM.to_string(),
    };
    let response = pm.query_spot_price(&request).unwrap();
    let price: Decimal = response.spot_price.parse().unwrap();

    println!("Spot price before entering position: {}", price);
    */
    println!("Creating position...\n");

    let position_msg = CreatePositionMsg {
        lower_tick: -108000000,
        upper_tick: 342000000,
        counterparty_token_min_amount: 85000u128.into(),
        principal_token_min_amount: 100000u128.into(),
    };

    let coins = &[
        Coin::new(85000u128, USDC_DENOM),
        Coin::new(100000u128, OSMO_DENOM),
    ];

    let _response = create_position(
        &wasm,
        &contract_addr,
        &pool_mockup.project_owner,
        coins,
        position_msg,
    );

    let position_response = pool_mockup.position_query(1);

    let query_msg = QueryMsg::State {};

    let state_response: StateResponse = wasm.query(&contract_addr, &query_msg).unwrap();
    //let state_response: StateResponse = from_json(&query_result).unwrap();
    let formatted_output = serde_json::to_string_pretty(&state_response).unwrap();

    // Print the state response
    println!("{}", formatted_output);

    println!("Printing position details...\n");
    let _liquidity = if let Ok(full_position) = position_response {
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
    let _usdc_balance = query_and_print_balance(&bank, &contract_addr, USDC_DENOM, "Contract");

    let _osmo_balance = query_and_print_balance(&bank, &contract_addr, OSMO_DENOM, "Contract");

    let pm = PoolManager::new(&pool_mockup.app);
    let request = SpotPriceRequest {
        pool_id: 1,
        base_asset_denom: OSMO_DENOM.to_string(),
        quote_asset_denom: USDC_DENOM.to_string(),
    };
    let response = pm.query_spot_price(&request).unwrap();
    let price: Decimal = response.spot_price.parse().unwrap();

    println!("Spot price before swap: {}", price);

    // this swap should make price goes below lower range - should make OSMO amount in pool be zero
    let usdc_needed: u128 = 117_647_058_823;
    println!("Doing a swap which will make principal amount goes to zero in the pool...\n");
    let _swap_response = pool_mockup.swap_usdc_for_osmo(&pool_mockup.user1, usdc_needed, 1);
    let position_response = pool_mockup.position_query(1);

    println!("Printing position details after swap...\n");
    let _liquidity = if let Ok(full_position) = position_response {
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

    let pm = PoolManager::new(&pool_mockup.app);
    let request = SpotPriceRequest {
        pool_id: 1,
        base_asset_denom: OSMO_DENOM.to_string(),
        quote_asset_denom: USDC_DENOM.to_string(),
    };
    let response = pm.query_spot_price(&request).unwrap();
    let price: Decimal = response.spot_price.parse().unwrap();

    println!("Spot price after swap: {}", price);

    //92195444572928873195000
    let liquidate_msg = ExecuteMsg::Liquidate;

    //100000
    let coins = &[Coin::new(100000u128, OSMO_DENOM)];

    println!("Executing liquidate msg...\n");
    let _response = wasm
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
    let _liquidity = if let Ok(full_position) = position_response {
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

    let query_msg = QueryMsg::State {};

    let state_response: StateResponse = wasm.query(&contract_addr, &query_msg).unwrap();
    let formatted_output = serde_json::to_string_pretty(&state_response).unwrap();

    // Print the state response
    println!("{}", formatted_output);

    println!("\nQuery-ing contract bank balances after liquidation...\n");
    let bank = Bank::new(&pool_mockup.app);
    let _usdc_balance = query_and_print_balance(&bank, &contract_addr, USDC_DENOM, "Contract");

    let _osmo_balance = query_and_print_balance(&bank, &contract_addr, OSMO_DENOM, "Contract");

    println!("\nQuery-ing principal owner bank balances after liquidation...\n");
    let _usdc_balance = query_and_print_balance(
        &bank,
        &pool_mockup.principal_funds_owner.address(),
        USDC_DENOM,
        "Principal funds owner",
    );

    let _osmo_balance = query_and_print_balance(
        &bank,
        &pool_mockup.principal_funds_owner.address(),
        OSMO_DENOM,
        "Principal funds owner",
    );
    let bank = Bank::new(&pool_mockup.app);
    println!("\nQuery-ing project owner bank balances after liquidation...\n");
    let _usdc_balance = query_and_print_balance(
        &bank,
        &pool_mockup.project_owner.address(),
        USDC_DENOM,
        "Project owner",
    );

    let _osmo_balance = query_and_print_balance(
        &bank,
        &pool_mockup.project_owner.address(),
        OSMO_DENOM,
        "Project owner",
    );

    println!("\nQuery-ing liquidator bank balances after liquidation...\n");
    let bank = Bank::new(&pool_mockup.app);
    let _usdc_balance = query_and_print_balance(
        &bank,
        &pool_mockup.liquidator.address(),
        USDC_DENOM,
        "Liquidator",
    );

    let _osmo_balance = query_and_print_balance(
        &bank,
        &pool_mockup.liquidator.address(),
        OSMO_DENOM,
        "Liquidator",
    );

    let query_msg = QueryMsg::State {};

    let state_response: StateResponse = wasm.query(&contract_addr, &query_msg).unwrap();

    let formatted_output = serde_json::to_string_pretty(&state_response).unwrap();
    // Print the state response
    println!("{}", formatted_output);

    println!("Printing position details...\n");
    let position_response = pool_mockup.position_query(1);
    let _liquidity = if let Ok(full_position) = position_response {
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
fn test_partial_liquidations() {
    let pool_mockup = PoolMockup::new(USDC_DENOM.to_string(), OSMO_DENOM.to_string());

    let wasm = Wasm::new(&pool_mockup.app);
    let code_id = store_contracts_code(&wasm, &pool_mockup.deployer);
    let contract_addr = instantiate(&wasm, &pool_mockup, code_id, false);
    println!("Creating position...\n");

    let position_msg = CreatePositionMsg {
        lower_tick: -108000000,
        upper_tick: 342000000,
        counterparty_token_min_amount: 85000u128.into(),
        principal_token_min_amount: 100000u128.into(),
    };

    let coins = &[
        Coin::new(85000u128, USDC_DENOM),
        Coin::new(100000u128, OSMO_DENOM),
    ];

    let _response = create_position(
        &wasm,
        &contract_addr,
        &pool_mockup.project_owner,
        coins,
        position_msg,
    );

    let position_response = pool_mockup.position_query(1);
    println!("Printing position details after creating position...\n");
    let _liquidity = if let Ok(full_position) = position_response {
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

    let query_msg = QueryMsg::State {};

    let state_response: StateResponse = wasm.query(&contract_addr, &query_msg).unwrap();
    let formatted_output = serde_json::to_string_pretty(&state_response).unwrap();

    // Print the state response
    println!("{}", formatted_output);

    // this swap should make price goes below lower range - should make OSMO amount in pool be zero
    let usdc_needed: u128 = 117_647_058_823;
    println!("Doing a swap which will make principal amount goes to zero in the pool...\n");
    let _swap_response = pool_mockup.swap_usdc_for_osmo(&pool_mockup.user1, usdc_needed, 1);
    let position_response = pool_mockup.position_query(1);
    println!("Printing position details after swap...\n");
    let _liquidity = if let Ok(full_position) = position_response {
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

    let query_msg = QueryMsg::State {};

    let state_response: StateResponse = wasm.query(&contract_addr, &query_msg).unwrap();
    let formatted_output = serde_json::to_string_pretty(&state_response).unwrap();

    // Print the state response
    println!("{}", formatted_output);

    let liquidate_msg = ExecuteMsg::Liquidate;

    let coins = &[Coin::new(50000u128, OSMO_DENOM)];

    println!("Executing first partial liquidate msg...\n");
    let _response = wasm
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
    let _liquidity = if let Ok(full_position) = position_response {
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

    let query_msg = QueryMsg::State {};

    let state_response: StateResponse = wasm.query(&contract_addr, &query_msg).unwrap();
    let formatted_output = serde_json::to_string_pretty(&state_response).unwrap();

    // Print the state response
    println!("{}", formatted_output);

    let coins = &[Coin::new(30001u128, OSMO_DENOM)];

    println!("Executing second partial liquidate msg...\n");
    let _response = wasm
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
    let _liquidity = if let Ok(full_position) = position_response {
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

    let query_msg = QueryMsg::State {};

    let state_response: StateResponse = wasm.query(&contract_addr, &query_msg).unwrap();
    let formatted_output = serde_json::to_string_pretty(&state_response).unwrap();

    // Print the state response
    println!("{}", formatted_output);
    let coins = &[Coin::new(19999u128, OSMO_DENOM)];

    println!("Executing third/final partial liquidate msg...\n");
    let _response = wasm
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
    let _liquidity = if let Ok(full_position) = position_response {
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

    let query_msg = QueryMsg::State {};

    let state_response: StateResponse = wasm.query(&contract_addr, &query_msg).unwrap();
    let formatted_output = serde_json::to_string_pretty(&state_response).unwrap();

    // Print the state response
    println!("{}", formatted_output);
}
#[test]
fn test_end_of_round_principal_higher_or_equal() {
    let pool_mockup = PoolMockup::new(USDC_DENOM.to_string(), OSMO_DENOM.to_string());
    let wasm = Wasm::new(&pool_mockup.app);
    let code_id = store_contracts_code(&wasm, &pool_mockup.deployer);
    let contract_addr = instantiate(&wasm, &pool_mockup, code_id, false);
    println!("Creating position...\n");

    let position_msg = CreatePositionMsg {
        lower_tick: -108000000,
        upper_tick: 342000000,
        counterparty_token_min_amount: 85000u128.into(),
        principal_token_min_amount: 100000u128.into(),
    };

    let coins = &[
        Coin::new(85000u128, USDC_DENOM),
        Coin::new(100000u128, OSMO_DENOM),
    ];

    let _response = create_position(
        &wasm,
        &contract_addr,
        &pool_mockup.deployer,
        coins,
        position_msg,
    );

    // this swap should make principal amount being higher than on creating position
    let osmo_needed: u128 = 10;
    println!(
        "Doing a swap which will make principal amount being higher than on creating position...\n"
    );
    let _swap_response = pool_mockup.swap_osmo_for_usdc(&pool_mockup.user1, osmo_needed, 1);

    let position_response = pool_mockup.position_query(1);

    println!("Printing position details after swap...\n");
    let _liquidity = if let Ok(full_position) = position_response {
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

    let query_msg = QueryMsg::State {};

    let state_response: StateResponse = wasm.query(&contract_addr, &query_msg).unwrap();
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
    let pool_mockup = PoolMockup::new(USDC_DENOM.to_string(), OSMO_DENOM.to_string());
    let wasm = Wasm::new(&pool_mockup.app);
    let code_id = store_contracts_code(&wasm, &pool_mockup.deployer);
    let contract_addr = instantiate(&wasm, &pool_mockup, code_id, false);
    println!("Creating position...\n");

    let position_msg = CreatePositionMsg {
        lower_tick: -108000000,
        upper_tick: 342000000,
        counterparty_token_min_amount: 85000u128.into(),
        principal_token_min_amount: 100000u128.into(),
    };

    let coins = &[
        Coin::new(85000u128, USDC_DENOM),
        Coin::new(100000u128, OSMO_DENOM),
    ];

    let _response = create_position(
        &wasm,
        &contract_addr,
        &pool_mockup.deployer,
        coins,
        position_msg,
    );

    // this swap should make principal amount being lower than on creating position but not zero
    let usdc_needed: u128 = 100000;
    println!("Doing a swap which will make principal amount being lower than on creating position but not zero...\n");
    let _swap_response = pool_mockup.swap_usdc_for_osmo(&pool_mockup.user1, usdc_needed, 1);

    let position_response = pool_mockup.position_query(1);

    println!("Printing position details after swap...\n");
    let _liquidity = if let Ok(full_position) = position_response {
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

    let query_msg = QueryMsg::State {};

    let state_response: StateResponse = wasm.query(&contract_addr, &query_msg).unwrap();
    let formatted_output = serde_json::to_string_pretty(&state_response).unwrap();

    println!("Printing contract state...\n");
    // Print the state response
    println!("{}", formatted_output);

    // Execute the first bid
    let first_bid = ExecuteMsg::PlaceBid(PlaceBidMsg {
        requested_amount: 10u128.into(),
    });

    let coins = &[Coin::new(539u128, OSMO_DENOM)];

    println!("Executing bid msg from user1 ...\n");
    let _response = wasm
        .execute(&contract_addr, &first_bid, coins, &pool_mockup.user1)
        .expect("Execution failed");

    // Execute the second bid
    let second_bid = ExecuteMsg::PlaceBid(PlaceBidMsg {
        requested_amount: 10u128.into(),
    });

    let coins = &[Coin::new(10000u128, OSMO_DENOM)];

    println!("Executing bid msg from user2...\n");
    let _response = wasm
        .execute(&contract_addr, &second_bid, coins, &pool_mockup.user2)
        .expect("Execution failed");

    // Execute the third bid
    let third_bid = ExecuteMsg::PlaceBid(PlaceBidMsg {
        requested_amount: 10000u128.into(),
    });

    let coins = &[Coin::new(10000u128, OSMO_DENOM)];

    println!("Executing bid msg from user3...\n");
    let _response = wasm
        .execute(&contract_addr, &third_bid, coins, &pool_mockup.user3)
        .expect("Execution failed");

    let fourth_bid = ExecuteMsg::PlaceBid(PlaceBidMsg {
        requested_amount: 10000u128.into(),
    });

    let coins = &[Coin::new(33805u128, OSMO_DENOM)];

    println!("Executing bid msg from user4...\n");
    let _response = wasm
        .execute(&contract_addr, &fourth_bid, coins, &pool_mockup.user4)
        .expect("Execution failed");

    let query_msg = QueryMsg::State {};

    let state_response: StateResponse = wasm.query(&contract_addr, &query_msg).unwrap();
    let formatted_output = serde_json::to_string_pretty(&state_response).unwrap();

    println!("Printing contract state...\n");
    // Print the state response
    println!("{}", formatted_output);

    let fifth_bid = ExecuteMsg::PlaceBid(PlaceBidMsg {
        requested_amount: 1u128.into(),
    });

    let coins = &[Coin::new(53805u128, OSMO_DENOM)];

    println!("Executing bid msg from user5...\n");
    let _response = wasm
        .execute(&contract_addr, &fifth_bid, coins, &pool_mockup.user5)
        .expect("Execution failed");

    let query_msg = QueryMsg::State {};

    let state_response: StateResponse = wasm.query(&contract_addr, &query_msg).unwrap();
    let formatted_output = serde_json::to_string_pretty(&state_response).unwrap();

    println!("Printing contract state...\n");
    // Print the state response
    println!("{}", formatted_output);

    // Query the sorted bids
    let query_msg = QueryMsg::SortedBids {};
    let response: SortedBidsResponse = wasm.query(&contract_addr, &query_msg).unwrap();

    // Print the sorted bids
    println!("Sorted Bids:");
    for bid in response.sorted_bids {
        println!(
            "Bid Id: {}, Price Ratio: {}, Principal deposited: {}, Counterparty requested: {}",
            bid.bid_id, bid.price, bid.principal_deposited, bid.requested_counterparty
        );
    }
    println!("Increasing time for 1000 seconds...\n");
    pool_mockup.app.increase_time(10000);

    let query_bids = QueryMsg::Bids {
        start_from: 0,
        limit: 10,
    };

    let bids_response: BidsResponse = wasm.query(&contract_addr, &query_bids).unwrap();

    // Deserialize the response to get the bids

    // Print all bids in a structured format
    for bid_response in bids_response.bids {
        println!(
    "Bid Id: {}\n Bidder Address: {}\n  Principal Deposited: {}\n  Tokens Requested: {}\n  Tokens Fulfilled: {}\n  Tokens Refunded: {}\n  Status: {:?}\n",
    bid_response.bid_id,
    bid_response.bid.bidder,
    bid_response.bid.principal_deposited,
    bid_response.bid.tokens_requested,
    bid_response.bid.tokens_fulfilled,
    bid_response.bid.tokens_refunded,
    bid_response.bid.status,
);
    }

    let resolve_auction_msg = ExecuteMsg::ResolveAuction;

    println!("Executing resolve auction msg...\n");
    let _response = wasm
        .execute(
            &contract_addr,
            &resolve_auction_msg,
            &[],
            &pool_mockup.deployer,
        )
        .expect("Execution failed");

    let query_bids = QueryMsg::Bids {
        start_from: 0,
        limit: 10,
    };

    let bids_response: BidsResponse = wasm.query(&contract_addr, &query_bids).unwrap();

    // Deserialize the response to get the bids

    // Print all bids in a structured format
    for bid_response in bids_response.bids {
        println!(
    "Bid Id: {}\n Bidder Address: {}\n  Principal Deposited: {}\n  Tokens Requested: {}\n  Tokens Fulfilled: {}\n  Tokens Refunded: {}\n  Status: {:?}\n",
    bid_response.bid_id,
    bid_response.bid.bidder,
    bid_response.bid.principal_deposited,
    bid_response.bid.tokens_requested,
    bid_response.bid.tokens_fulfilled,
    bid_response.bid.tokens_refunded,
    bid_response.bid.status,
);
    }
}

#[test]
fn test_resolve_auction() {
    // Step 1: Set up the mock environment and dependencies
    let mut deps = mock_dependencies();
    let mut env = mock_env();
    let info = mock_info("deployer", &[]);

    // Step 2: Initialize the state
    let state = State {
        auction_end_time: Some(env.block.time.plus_seconds(0)), // Set the auction end time
        principal_to_replenish: Uint128::new(53805),            // Target principal to replenish
        counterparty_to_give: Some(Uint128::new(183999)),       // Total counterparty available
        counterparty_owner: Some(Addr::unchecked("deployer")),
        principal_funds_owner: Addr::unchecked("principal_funds_owner"),
        pool_id: 1,
        counterparty_denom: "usdc".to_string(),
        principal_denom: "osmo".to_string(),
        position_id: Some(1),
        initial_principal_amount: Uint128::new(1000),
        initial_counterparty_amount: Uint128::new(500),
        liquidity_shares: Some("92195444572928873195000".to_string()),
        auction_principal_deposited: Uint128::new(54345),
        auction_duration: 100,
        position_admin: None,
        liquidator_address: None,
        round_end_time: env.block.time.plus_seconds(100),
        position_rewards: None,
        principal_first: false,
    };
    STATE.save(deps.as_mut().storage, &state).unwrap();

    env.block.time = env.block.time.plus_seconds(1);

    // Step 3: Add bids to the storage
    let bid1 = Bid {
        bidder: Addr::unchecked("bidder1"),
        principal_deposited: Uint128::new(539),
        tokens_requested: Uint128::new(10),
        tokens_fulfilled: Uint128::zero(),
        tokens_refunded: Uint128::zero(),
        status: BidStatus::Submitted,
    };
    let bid2 = Bid {
        bidder: Addr::unchecked("bidder2"),
        principal_deposited: Uint128::new(10000),
        tokens_requested: Uint128::new(10),
        tokens_fulfilled: Uint128::zero(),
        tokens_refunded: Uint128::zero(),
        status: BidStatus::Submitted,
    };
    let bid3 = Bid {
        bidder: Addr::unchecked("bidder3"),
        principal_deposited: Uint128::new(10000),
        tokens_requested: Uint128::new(10000),
        tokens_fulfilled: Uint128::zero(),
        tokens_refunded: Uint128::zero(),
        status: BidStatus::Submitted,
    };
    let bid4 = Bid {
        bidder: Addr::unchecked("bidder4"),
        principal_deposited: Uint128::new(33805),
        tokens_requested: Uint128::new(10000),
        tokens_fulfilled: Uint128::zero(),
        tokens_refunded: Uint128::zero(),
        status: BidStatus::Submitted,
    };

    let bid5 = Bid {
        bidder: Addr::unchecked("bidder5"),
        principal_deposited: Uint128::new(10001),
        tokens_requested: Uint128::new(1),
        tokens_fulfilled: Uint128::zero(),
        tokens_refunded: Uint128::zero(),
        status: BidStatus::Submitted,
    };

    BIDS.save(deps.as_mut().storage, 1, &bid1).unwrap();
    BIDS.save(deps.as_mut().storage, 2, &bid2).unwrap();
    BIDS.save(deps.as_mut().storage, 3, &bid3).unwrap();
    BIDS.save(deps.as_mut().storage, 4, &bid4).unwrap();
    BIDS.save(deps.as_mut().storage, 5, &bid5).unwrap();

    // Step 4: Sort the bids by price ratio (tokens_requested / principal_amount)

    let mut all_bids: Vec<SortedBid> = vec![
        SortedBid {
            bid_id: 1,
            price: Decimal::from_ratio(bid1.tokens_requested, bid1.principal_deposited),
            principal_deposited: bid1.principal_deposited,
            requested_counterparty: bid1.tokens_requested,
        },
        SortedBid {
            bid_id: 2,
            price: Decimal::from_ratio(bid2.tokens_requested, bid2.principal_deposited),
            principal_deposited: bid2.principal_deposited,
            requested_counterparty: bid2.tokens_requested,
        },
        SortedBid {
            bid_id: 3,
            price: Decimal::from_ratio(bid3.tokens_requested, bid3.principal_deposited),
            principal_deposited: bid3.principal_deposited,
            requested_counterparty: bid3.tokens_requested,
        },
        SortedBid {
            bid_id: 4,
            price: Decimal::from_ratio(bid4.tokens_requested, bid4.principal_deposited),
            principal_deposited: bid4.principal_deposited,
            requested_counterparty: bid4.tokens_requested,
        },
        SortedBid {
            bid_id: 5,
            price: Decimal::from_ratio(bid5.tokens_requested, bid5.principal_deposited),
            principal_deposited: bid5.principal_deposited,
            requested_counterparty: bid5.tokens_requested,
        },
    ];

    // Sort the bids in descending order of price ratio (highest price first)
    all_bids.sort_by(|a, b| b.price.partial_cmp(&a.price).unwrap());

    // Save the sorted bids to SORTED_BIDS storage
    SORTED_BIDS.save(deps.as_mut().storage, &all_bids).unwrap();

    // Step 4: Call the resolve_auction method
    let result = resolve_auction(deps.as_mut(), env.clone(), info.clone());

    // Step 5: Assert the results
    assert!(result.is_ok());
}
#[test]
fn test_tick_to_price() {
    let tick = 11046100; // Example tick value
    let price = tick_to_sqrt_price(tick).unwrap();

    println!("Sqrt price: {}", price);

    let sqrt_price_f64 = price.to_string().parse::<f64>().unwrap_or(f64::NAN);
    println!("Sqrt price (f64): {}", sqrt_price_f64);
}

#[test]
fn test_calculate_amount_1() {
    let amount0_str = "0.009578";
    let amount0 = BigDecimal::from_str(amount0_str).unwrap();
    let current_price_str = "1.461";
    let current_price = BigDecimal::from_str(current_price_str).unwrap();
    let current_sqrt_price = current_price.sqrt().expect("Failed to take sqrt");
    let lower_tick = 433500;
    let upper_tick = 500600;
    let amount1 = calc_amount1(
        amount0.clone(),
        lower_tick,
        upper_tick,
        current_sqrt_price.clone(),
    );

    let liquidation_bonus = "0.0";
    let liquidation_bonus = BigDecimal::from_str(liquidation_bonus).unwrap();

    println!("Token1 amount on entering position: {}", amount1);

    let response = calc_required_token1_with_bonus(amount0, amount1, lower_tick, liquidation_bonus);
    //921954445729288730000575
    //92195444572928873195000
    println!("Token1 amount on liquidation: {}", response);
}

#[test]
fn test_calculate_amount_0() {
    let amount1_str = "1000";
    let amount1 = BigDecimal::from_str(amount1_str).unwrap();
    // ratio calculated as: osmo/usdc (spot price: base - osmo, quote-usdc)
    // token0 is always quote in osmosis pool
    let current_price_str = "29.4696172";
    let current_price = BigDecimal::from_str(current_price_str).unwrap();
    let current_sqrt_price = current_price.sqrt().expect("Failed to take sqrt");
    let lower_tick = 10746100;
    let upper_tick = 11046100;
    let response = calc_amount0(
        amount1.clone(),
        lower_tick,
        upper_tick,
        current_sqrt_price.clone(),
    );

    println!("Token0 amount on entering position: {}", response); // <-- BigInt supports {}
}

#[test]
fn test_fetch_all_rewards() {
    // Simulate your input:
    let spread_rewards = vec![
        OsmosisCoin {
            denom: "ibc/9FF2B7A5F55038A7EE61F4FD6749D9A648B48E89830F2682B67B5DC158E2753C"
                .to_string(),
            amount: "23".to_string(),
        },
        OsmosisCoin {
            denom: "uosmo".to_string(),
            amount: "34".to_string(),
        },
    ];

    let claimable_incentives = vec![];
    let forfeited_incentives = vec![];

    let result =
        fetch_all_rewards(spread_rewards, claimable_incentives, forfeited_incentives).unwrap();

    let expected = vec![
        Coin {
            denom: "ibc/9FF2B7A5F55038A7EE61F4FD6749D9A648B48E89830F2682B67B5DC158E2753C"
                .to_string(),
            amount: Uint128::new(23),
        },
        Coin {
            denom: "uosmo".to_string(),
            amount: Uint128::new(34),
        },
    ];

    assert_eq!(result, expected);
}

#[test]
fn test_withdraw_end_round_reply() {
    // Step 1: Set up the mock environment and dependencies
    let mut deps = mock_dependencies();
    let mut env = mock_env();
    let _info = mock_info("deployer", &[]);

    // Step 2: Initialize the state
    let state = State {
        auction_end_time: Some(env.block.time.plus_seconds(0)), // Set the auction end time
        principal_to_replenish: Uint128::new(103333),           // Target principal to replenish
        counterparty_to_give: None,                             // Total counterparty available
        counterparty_owner: Some(Addr::unchecked("deployer")),
        principal_funds_owner: Addr::unchecked("principal_funds_owner"),
        pool_id: 1,
        counterparty_denom: "ibc/9FF2B7A5F55038A7EE61F4FD6749D9A648B48E89830F2682B67B5DC158E2753C"
            .to_string(),
        principal_denom: "uosmo".to_string(),
        position_id: Some(1),
        initial_principal_amount: Uint128::new(1000),
        initial_counterparty_amount: Uint128::new(500),
        liquidity_shares: Some("92195444572928873195000".to_string()),
        auction_principal_deposited: Uint128::new(0),
        auction_duration: 100,
        position_admin: None,
        liquidator_address: None,
        round_end_time: env.block.time.plus_seconds(100),
        position_rewards: Some(vec![
            Coin {
                denom: "ibc/9FF2B7A5F55038A7EE61F4FD6749D9A648B48E89830F2682B67B5DC158E2753C"
                    .to_string(),
                amount: Uint128::new(23),
            },
            Coin {
                denom: "uosmo".to_string(),
                amount: Uint128::new(34),
            },
        ]),
        principal_first: false,
    };
    STATE.save(deps.as_mut().storage, &state).unwrap();

    // Mock balances: contract holds enough
    deps.querier.update_balance(
        env.contract.address.clone(),
        vec![
            Coin {
                denom: "uosmo".to_string(),
                amount: Uint128::new(100),
            },
            Coin {
                denom: "ibc/9FF2B7A5F55038A7EE61F4FD6749D9A648B48E89830F2682B67B5DC158E2753C"
                    .to_string(),
                amount: Uint128::new(103371),
            },
        ],
    );

    env.block.time = env.block.time.plus_seconds(1);
    let proto = MsgWithdrawPositionResponse {
        amount0: "59998".to_string(),
        amount1: "103337".to_string(),
    };

    let mut buf = Vec::new();
    proto.encode(&mut buf).unwrap();

    let binary_data = Binary::from(buf);

    // Step 3: Build the Reply inline
    let reply = Reply {
        id: 3, // use the ID your contract expects
        result: SubMsgResult::Ok(SubMsgResponse {
            events: vec![],
            data: Some(binary_data),
        }),
    };

    // Step 4: Call the resolve_auction method
    let result = handle_withdraw_position_end_round_reply(deps.as_mut(), env.clone(), reply);

    // Step 5: Assert the results
    assert!(result.is_ok());
}
#[test]
fn test_calculate_final_amount_at_lower_tick_with_bonus() {
    let amount1_str = "1000";
    let amount1 = BigDecimal::from_str(amount1_str).unwrap();
    // ratio calculated as: osmo/usdc (spot price: base - osmo, quote-usdc)
    // token0 is always quote in osmosis pool
    let current_price_str = "29.46961726";
    let current_price = BigDecimal::from_str(current_price_str).unwrap();
    let current_sqrt_price = current_price.sqrt().expect("Failed to take sqrt");
    let lower_tick = 10746100;
    let upper_tick = 11046100;
    let bonus = 0.2;

    let (amount0, amount0_with_bonus, value_without_bonus, value_with_bonus) =
        calc_final_amount_at_lower_tick_with_bonus(
            amount1,
            lower_tick,
            upper_tick,
            current_sqrt_price,
            bonus,
        );

    println!("Token0 amount at lower tick (no bonus): {}", amount0);
    println!(
        "Token0 amount at lower tick (with bonus): {}",
        amount0_with_bonus
    );
    println!(
        "Value of token0 at lower tick (no bonus) in token1: {}",
        value_without_bonus
    );
    println!(
        "Value of token0 at lower tick (with bonus) in token1: {}",
        value_with_bonus
    );
}
