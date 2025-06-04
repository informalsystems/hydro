#![cfg(test)]
use anyhow::Result;
use std::{env, str::FromStr};

use cosmwasm_std::{Coin, Decimal, Uint128};
use osmosis_std::types::{
    cosmos::bank::v1beta1::QueryBalanceRequest,
    osmosis::{
        concentratedliquidity::v1beta1::{
            CreateConcentratedLiquidityPoolsProposal, FullPositionBreakdown, PoolRecord,
            PositionByIdRequest,
        },
        poolmanager::v1beta1::{MsgSwapExactAmountIn, SwapAmountInRoute},
    },
};
use osmosis_test_tube::{
    Account, Bank, ConcentratedLiquidity, GovWithAppAccess, Module, OsmosisTestApp, PoolManager,
    SigningAccount, Wasm,
};

pub const MIN_TICK: i32 = -108_000_000;
pub const MAX_TICK: i32 = 342_000_000;
pub const MIN_LIQUIDITY: Uint128 = Uint128::new(1000);
pub const TWAP_SECONDS: u64 = 60;
pub const POSITION_CREATION_SLIPPAGE: Decimal = Decimal::permille(999);

pub static PROTOCOL_ADDR: &str = "osmo1m3kd260ek7rl3a78mwgzlcpgjlfafzuqgpx5mj";
pub const DEFAULT_PROTOCOL_FEE: Decimal = Decimal::permille(50);
pub const MAX_PROTOCOL_FEE: Decimal = Decimal::permille(100);
/// USDC denom for mainnet.
pub const VAULT_CREATION_COST_DENOM: &str =
    "ibc/498A0751C798A0D9A389AA3691123DADA57DAA4FE165D5C75894505B876BA6E4";
/// As USDC has 6 decimals, 5_000_000 USDC atoms is 5 USDC.
pub const VAULT_CREATION_COST: Uint128 = Uint128::new(5_000_000);

pub const USDC_DENOM: &str = "ibc/498A0751C798A0D9A389AA3691123DADA57DAA4FE165D5C75894505B876BA6E4";
pub const OSMO_DENOM: &str = "uosmo";

pub struct PoolMockup {
    pub app: OsmosisTestApp,
    pub deployer: SigningAccount,
    pub user1: SigningAccount,
    pub user2: SigningAccount,
    pub user3: SigningAccount,
    pub user4: SigningAccount,
    pub user5: SigningAccount,
    pub principal_funds_owner: SigningAccount,
    pub project_owner: SigningAccount,
    pub liquidator: SigningAccount,
}

impl PoolMockup {
    pub fn new(denom0: String, denom1: String) -> Self {
        let app = OsmosisTestApp::new();

        let init_coins = &[
            Coin::new(1_000_000_000_000_000u128, USDC_DENOM),
            Coin::new(1_000_000_000_000_000u128, OSMO_DENOM),
        ];

        let mut accounts = app.init_accounts(init_coins, 7).unwrap().into_iter();
        let user1 = accounts.next().unwrap();
        let user2 = accounts.next().unwrap();
        let user3 = accounts.next().unwrap();
        let user4 = accounts.next().unwrap();
        let user5 = accounts.next().unwrap();
        let deployer = accounts.next().unwrap();
        let principal_funds_owner = app.init_account(&[]).unwrap();
        let project_owner = app
            .init_account(&[
                Coin::new(10_000_000_000u128, OSMO_DENOM),
                Coin::new(85000u128, USDC_DENOM),
            ])
            .unwrap();
        //18375375000
        let liquidator = app
            .init_account(&[Coin::new(10_000_000_000_000u128, OSMO_DENOM)])
            .unwrap();

        let gov = GovWithAppAccess::new(&app);

        // Pool setup.
        gov.propose_and_execute(
            CreateConcentratedLiquidityPoolsProposal::TYPE_URL.to_string(),
            CreateConcentratedLiquidityPoolsProposal {
                title: "Create cl uosmo:usdc pool".into(),
                description: "blabla".into(),
                pool_records: vec![PoolRecord {
                    denom0,
                    denom1,
                    tick_spacing: 100,
                    spread_factor: Decimal::from_str("0.01").unwrap().atomics().into(),
                }],
            },
            deployer.address(),
            &deployer,
        )
        .unwrap();

        Self {
            app,
            deployer,
            user1,
            user2,
            user3,
            user4,
            user5,
            principal_funds_owner,
            project_owner,
            liquidator,
        }
    }

    pub fn swap_osmo_for_usdc(
        &self,
        from: &SigningAccount,
        osmo_in: u128,
        pool_id: u64,
    ) -> Result<Uint128> {
        let pm = PoolManager::new(&self.app);
        let usdc_got = pm
            .swap_exact_amount_in(
                MsgSwapExactAmountIn {
                    sender: from.address(),
                    routes: vec![SwapAmountInRoute {
                        pool_id,
                        token_out_denom: USDC_DENOM.into(),
                    }],
                    token_in: Some(Coin::new(osmo_in, OSMO_DENOM).into()),
                    token_out_min_amount: "1".into(),
                },
                from,
            )
            .map(|x| x.data.token_out_amount)
            .map(|amount| Uint128::from_str(&amount).unwrap());

        Ok(usdc_got?)
    }

    pub fn swap_usdc_for_osmo(
        &self,
        from: &SigningAccount,
        usdc_in: u128,
        pool_id: u64,
    ) -> Result<Uint128> {
        let pm = PoolManager::new(&self.app);
        let usdc_got = pm
            .swap_exact_amount_in(
                MsgSwapExactAmountIn {
                    sender: from.address(),
                    routes: vec![SwapAmountInRoute {
                        pool_id,
                        token_out_denom: OSMO_DENOM.into(),
                    }],
                    token_in: Some(Coin::new(usdc_in, USDC_DENOM).into()),
                    token_out_min_amount: "1".into(),
                },
                from,
            )
            .map(|x| x.data.token_out_amount)
            .map(|amount| Uint128::from_str(&amount).unwrap());

        Ok(usdc_got?)
    }

    pub fn osmo_balance_query<T: ToString>(&self, address: T) -> Uint128 {
        let bank = Bank::new(&self.app);
        let amount = bank
            .query_balance(&QueryBalanceRequest {
                address: address.to_string(),
                denom: OSMO_DENOM.into(),
            })
            .unwrap()
            .balance
            .unwrap()
            .amount;
        Uint128::from_str(&amount).unwrap()
    }

    pub fn position_query(&self, position_id: u64) -> Result<FullPositionBreakdown> {
        let cl = ConcentratedLiquidity::new(&self.app);
        let pos = cl.query_position_by_id(&PositionByIdRequest { position_id })?;
        Ok(pos.position.expect("oops"))
    }

    pub fn position_liquidity(&self, position_id: u64) -> Result<Decimal> {
        let pos = self.position_query(position_id)?;
        let liq = pos
            .position
            .map(|x| Uint128::from_str(&x.liquidity))
            .expect("oops")
            .map(|x| Decimal::raw(x.u128()))?;
        Ok(liq)
    }
}

pub fn store_contracts_code(wasm: &Wasm<OsmosisTestApp>, deployer: &SigningAccount) -> u64 {
    // Get current working directory
    let cwd = env::current_dir().unwrap();
    println!("Current working directory: {:?}", cwd);

    // Define the relative wasm path
    let wasm_path = "../../target/wasm32-unknown-unknown/release/liquid_collateral.wasm";

    // Canonicalize the relative path to an absolute path
    let absolute_wasm_path = cwd.join(wasm_path);
    let full_path = absolute_wasm_path.canonicalize().unwrap();

    // Print the resolved absolute path
    println!("Attempting to read file at: {:?}", full_path);

    // Read the contract bytecode
    let contract_bytecode = std::fs::read(full_path)
        .expect("Failed to read contract file. Ensure it exists and the path is correct.");

    wasm.store_code(&contract_bytecode, None, deployer)
        .unwrap()
        .data
        .code_id
}
