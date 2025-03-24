#[cfg(any(test, feature = "fuzzing"))]

pub mod mock {
    use anyhow::Result;
    use std::str::FromStr;

    use cosmwasm_std::{testing::mock_dependencies, Addr, Api, Coin, Decimal, Uint128};
    use cw20_base::state::TokenInfo;
    use osmosis_std::types::{
        cosmos::bank::v1beta1::QueryBalanceRequest,
        cosmwasm::wasm::v1::MsgExecuteContractResponse,
        osmosis::{
            concentratedliquidity::v1beta1::{
                CreateConcentratedLiquidityPoolsProposal, FullPositionBreakdown, MsgCreatePosition,
                PoolRecord, PositionByIdRequest,
            },
            poolmanager::v1beta1::{MsgSwapExactAmountIn, SwapAmountInRoute},
        },
    };
    use osmosis_test_tube::{
        Account, Bank, ConcentratedLiquidity, ExecuteResponse, GovWithAppAccess, Module,
        OsmosisTestApp, PoolManager, SigningAccount, Wasm,
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
    pub const VAULT_CREATION_COST_DENOM: &str = "ibc/498A0751C798A0D9A389AA3691123DADA57DAA4FE165D5C75894505B876BA6E4";
    /// As USDC has 6 decimals, 5_000_000 USDC atoms is 5 USDC.
    pub const VAULT_CREATION_COST: Uint128 = Uint128::new(5_000_000);

    


    // use crate::{
    //     constants::{MAX_TICK, MIN_TICK, TWAP_SECONDS, VAULT_CREATION_COST, VAULT_CREATION_COST_DENOM},
    //     msg::{
    //         CalcSharesAndUsableAmountsResponse, DepositMsg, ExecuteMsg, InstantiateMsg, PositionBalancesWithFeesResponse, QueryMsg, VaultBalancesResponse, VaultInfoInstantiateMsg, VaultParametersInstantiateMsg, VaultRebalancerInstantiateMsg, WithdrawMsg
    //     },
    //     state::{
    //         FeesInfo, PositionType, ProtocolFee, VaultParameters, VaultState,
    //     },
    // };

    // TODO: Ideally abstract those 2, so the tests dev doesnt has to keep
    // track of whats in the pool.
    pub const ATOM_DENOM: &str = "uatom";
    pub const OSMO_DENOM: &str = "uosmo";
    
    pub struct PoolMockup {
        pub pool_id: u64,
        pub initial_position_id: u64,
        pub app: OsmosisTestApp,
        pub deployer: SigningAccount,
        pub user1: SigningAccount,
        pub user2: SigningAccount,
        pub price: Decimal,
    }

    impl PoolMockup {
        pub fn new_with_spread(usdc_in: u128, osmo_in: u128, spread_factor: &str) -> Self {
            
            let app = OsmosisTestApp::new();
            
            let init_coins = &[
                Coin::new(1_000_000_000_000_000u128, ATOM_DENOM),
                Coin::new(1_000_000_000_000_000u128, OSMO_DENOM),
            ];

            let mut accounts = app.init_accounts(init_coins, 3).unwrap().into_iter();
            let deployer = accounts.next().unwrap();
            let user1 = accounts.next().unwrap();
            let user2 = accounts.next().unwrap();

            let cl = ConcentratedLiquidity::new(&app);
            let gov = GovWithAppAccess::new(&app);

            // Pool setup.
            gov.propose_and_execute(
                CreateConcentratedLiquidityPoolsProposal::TYPE_URL.to_string(),
                CreateConcentratedLiquidityPoolsProposal {
                    title: "Create cl uosmo:usdc pool".into(),
                    description: "blabla".into(),
                    pool_records: vec![PoolRecord {
                        denom0: ATOM_DENOM.into(),
                        denom1: OSMO_DENOM.into(),
                        tick_spacing: 30,
                        spread_factor: Decimal::from_str(spread_factor).unwrap().atomics().into()
                    }]
                },
                deployer.address(),
                &deployer,
            )
            .unwrap();

            // NOTE: Could fail if we test multiple pools/positions.
            let pool_id = 1;
            let initial_position_id = 1;

            let position_res = cl
                .create_position(
                    MsgCreatePosition {
                        pool_id,
                        sender: deployer.address(),
                        lower_tick: MIN_TICK.into(),
                        upper_tick: MAX_TICK.into(),
                        tokens_provided: vec![
                            Coin::new(usdc_in, ATOM_DENOM).into(),
                            Coin::new(osmo_in, OSMO_DENOM).into(),
                        ],
                        token_min_amount0: ((usdc_in*99999)/100000).to_string(),
                        token_min_amount1: ((osmo_in*99999)/100000).to_string(),
                    },
                    &deployer,
                )
                .unwrap()
                .data;

            // NOTE: Could fail if we test multiple positions.
            assert_eq!(position_res.position_id, 1);
            app.increase_time(TWAP_SECONDS);

            let price = Decimal::new(osmo_in.into()) / Decimal::new(usdc_in.into());

            Self {
                pool_id, initial_position_id, app, deployer, user1, user2, price
            }
            
        }

        pub fn new(usdc_in: u128, osmo_in: u128) -> Self {
            Self::new_with_spread(usdc_in, osmo_in, "0.01")
        }

        pub fn swap_osmo_for_usdc(&self, from: &SigningAccount, osmo_in: u128) -> Result<Uint128> {
            let pm = PoolManager::new(&self.app);
            let usdc_got = pm.swap_exact_amount_in(
                MsgSwapExactAmountIn {
                    sender: from.address(),
                    routes: vec![SwapAmountInRoute {
                        pool_id: self.pool_id,
                        token_out_denom: ATOM_DENOM.into(),
                    }],
                    token_in: Some(Coin::new(osmo_in, OSMO_DENOM).into()),
                    token_out_min_amount: "1".into(),
                },
                from
            )
                .map(|x| x.data.token_out_amount)
                .map(|amount| Uint128::from_str(&amount).unwrap());

            Ok(usdc_got?)
        }

        pub fn swap_usdc_for_osmo(&self, from: &SigningAccount, usdc_in: u128) -> Result<Uint128> {
            let pm = PoolManager::new(&self.app);
            let usdc_got = pm.swap_exact_amount_in(
                MsgSwapExactAmountIn {
                    sender: from.address(),
                    routes: vec![SwapAmountInRoute {
                        pool_id: self.pool_id,
                        token_out_denom: OSMO_DENOM.into(),
                    }],
                    token_in: Some(Coin::new(usdc_in, ATOM_DENOM).into()),
                    token_out_min_amount: "1".into(),
                },
                from
            )
                .map(|x| x.data.token_out_amount)
                .map(|amount| Uint128::from_str(&amount).unwrap());

            Ok(usdc_got?)
        }

        pub fn osmo_balance_query<T: ToString>(&self, address: T) -> Uint128 {
            let bank = Bank::new(&self.app);
            let amount = bank.query_balance(&QueryBalanceRequest {
                address: address.to_string(),
                denom: OSMO_DENOM.into()
            }).unwrap().balance.unwrap().amount;
            Uint128::from_str(&amount).unwrap()
        }

        pub fn usdc_balance_query<T: ToString>(&self, address: T) -> Uint128 {
            let bank = Bank::new(&self.app);
            let amount = bank.query_balance(&QueryBalanceRequest {
                address: address.to_string(),
                denom: ATOM_DENOM.into()
            }).unwrap().balance.unwrap().amount;
            Uint128::from_str(&amount).unwrap()
        }

        pub fn position_query(&self, position_id: u64) -> Result<FullPositionBreakdown> {
            let cl = ConcentratedLiquidity::new(&self.app);
            let pos = cl.query_position_by_id(&PositionByIdRequest { position_id })?;
            Ok(pos.position.expect("oops"))
        }

        pub fn position_liquidity(&self, position_id: u64) -> Result<Decimal> {
            let pos = self.position_query(position_id)?;
            let liq = pos.position
                .map(|x| Uint128::from_str(&x.liquidity))
                .expect("oops")
                .map(|x| Decimal::raw(x.u128()))?;
            Ok(liq)
        }
    }
}