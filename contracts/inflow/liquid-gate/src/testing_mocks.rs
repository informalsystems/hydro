//! A small mocked chain for the gate's tests: bank, staking (delegations, unbonding entries with
//! completion times, the `max_entries` check, auto-claimed rewards), the x/liquid redeem (decoded
//! from the gate's `CosmosMsg::Any`), and mocks of hATOM and the Inflow vault (with its control
//! center). Multi-test cannot run `Any` messages nor return `MsgUndelegateResponse`, so the gate's
//! messages are executed here, depth-first as wasmd does, with each reply called with the
//! `msg_responses` the chain returns, and a failed transaction or submessage rolled back.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::rc::Rc;

use cosmwasm_std::testing::{mock_env, MockApi, MockStorage};
use cosmwasm_std::{
    from_json, to_json_binary, Addr, AllDelegationsResponse, BalanceResponse, BankMsg, BankQuery,
    Binary, BondedDenomResponse, Coin, ContractResult, CosmosMsg, Decimal256, Delegation, Empty,
    Env, MessageInfo, MsgResponse, Order, OwnedDeps, Querier, QuerierResult, QueryRequest, Reply,
    ReplyOn, Response, StakingMsg, StakingQuery, Storage, SubMsgResponse, SubMsgResult,
    SystemError, SystemResult, Timestamp, Uint128, Uint256, WasmMsg, WasmQuery,
};
use interface::inflow_control_center::{
    Config as CcConfig, ConfigResponse as CcConfigResponse, PoolInfoResponse as CcPoolInfo,
    QueryMsg as CcQueryMsg,
};
use interface::inflow_vault::{
    Config as VaultConfig, ConfigResponse as VaultConfigResponse, ExecuteMsg as VaultExecuteMsg,
    QueryMsg as VaultQueryMsg, WithdrawalQueueInfo, WithdrawalQueueInfoResponse,
};
use prost::Message;
use serde::de::DeserializeOwned;

use crate::chain::{self, proto};
use crate::contract;
use crate::hatom::{
    Allocation, BurnResult, BurnSimulation, HatomExecuteMsg, HatomQueryMsg, MintResult,
    MintSimulation, RateResponse,
};
use crate::msg::{ExecuteMsg, InstantiateMsg, QueryMsg, Settings};

pub const BOND: &str = "uatom";
pub const HATOM: &str = "factory/hatom/uhatom";
pub const SHARES: &str = "factory/vault/shares";
pub const START: u64 = 1_800_000_000;

#[derive(Clone, Default)]
pub struct HatomMock {
    pub holders_value: u128,
    pub supply: u128,
    pub paused: bool,
    pub compound_refused: bool,
    /// Added to `holders_value` by a successful Compound.
    pub compound_gain: u128,
    pub compounds: u32,
    /// The validators a burn pays from (the legs).
    pub burn_legs: Vec<String>,
    pub burn_fee_bps: u128,
    pub burn_rebate_bps: u128,
    pub next_record: u64,
}

#[derive(Clone, Default)]
pub struct VaultMock {
    pub issued: u128,
    /// hATOM deployed outside the vault (counts in the pool, not in the balance).
    pub deployed: u128,
    /// Withdrawals are queued instead of paid.
    pub queue_withdrawals: bool,
    pub non_funded: u128,
    /// The control center's deposit cap, in ATOM.
    pub deposit_cap: u128,
    pub paused: bool,
}

#[derive(Clone, Default)]
pub struct Unbonding {
    pub delegator: String,
    pub validator: String,
    pub completion: Timestamp,
    pub amount: u128,
}

#[derive(Clone, Default)]
pub struct World {
    pub time: Timestamp,
    pub height: u64,
    pub gate: String,
    pub hatom_addr: String,
    pub vault_addr: String,
    pub cc_addr: String,
    pub balances: BTreeMap<(String, String), u128>,
    pub delegations: BTreeMap<(String, String), u128>,
    pub unbonding: Vec<Unbonding>,
    pub unbonding_time_s: u64,
    pub max_entries: usize,
    /// Undelegations from these validators are refused.
    pub refuse_undelegate: HashSet<String>,
    /// Rewards auto-claimed to the delegator by each undelegation and redeem.
    pub auto_claim: u128,
    /// The fraction (bps) of each unbonding entry lost to a slash at maturity.
    pub slash_bps: u128,
    pub hatom: HatomMock,
    pub vault: VaultMock,
}

fn err(s: impl ToString) -> String {
    s.to_string()
}

impl World {
    pub fn bal(&self, addr: &str, denom: &str) -> u128 {
        *self
            .balances
            .get(&(addr.to_string(), denom.to_string()))
            .unwrap_or(&0)
    }

    pub fn mint(&mut self, addr: &str, denom: &str, amount: u128) {
        *self
            .balances
            .entry((addr.to_string(), denom.to_string()))
            .or_default() += amount;
    }

    pub fn burn(&mut self, addr: &str, denom: &str, amount: u128) -> Result<(), String> {
        let b = self
            .balances
            .entry((addr.to_string(), denom.to_string()))
            .or_default();
        if *b < amount {
            return Err(format!(
                "insufficient funds: {addr} has {b}{denom}, needs {amount}"
            ));
        }
        *b -= amount;
        Ok(())
    }

    pub fn send(&mut self, from: &str, to: &str, coins: &[Coin]) -> Result<(), String> {
        for c in coins {
            self.burn(from, &c.denom, c.amount.u128())?;
            self.mint(to, &c.denom, c.amount.u128());
        }
        Ok(())
    }

    // ---- hATOM

    pub fn rate(&self) -> RateResponse {
        let h = &self.hatom;
        RateResponse {
            exchange_rate: Decimal256::from_ratio(h.holders_value, h.supply.max(1)),
            supply: Uint128::new(h.supply),
            holders_value: Uint256::from(h.holders_value),
        }
    }

    fn hatom_value(&self, amount: u128) -> u128 {
        self.rate().value_of(Uint128::new(amount)).u128()
    }

    fn simulate_burn(&self, amount: u128) -> Result<BurnSimulation, String> {
        let h = &self.hatom;
        if h.paused {
            return Err(err("the contract is paused"));
        }
        let value = self.hatom_value(amount);
        let fee = value * h.burn_fee_bps / 10_000;
        let rebate = value * h.burn_rebate_bps / 10_000;
        let out = value - fee + rebate;
        let n = h.burn_legs.len() as u128;
        let allocation = h
            .burn_legs
            .iter()
            .enumerate()
            .map(|(i, v)| Allocation {
                validator: v.clone(),
                amount: Uint128::new(out / n + if i == 0 { out % n } else { 0 }),
            })
            .collect();
        Ok(BurnSimulation {
            value: Uint128::new(value),
            imbalance_fee: Uint128::new(fee),
            imbalance_rebate: Uint128::new(rebate),
            out: Uint128::new(out),
            allocation,
        })
    }

    fn minted_for(&self, amount: u128) -> u128 {
        amount * self.hatom.supply / self.hatom.holders_value
    }

    fn hatom_query(&self, msg: &Binary) -> Result<Binary, String> {
        let h = &self.hatom;
        match from_json(msg).map_err(err)? {
            HatomQueryMsg::Config {} => to_json_binary(&serde_json_like_config(h.paused)),
            HatomQueryMsg::Rate {} => to_json_binary(&self.rate()),
            HatomQueryMsg::SimulateMintNative { amount, .. } => {
                if h.paused {
                    return Err(err("the contract is paused"));
                }
                to_json_binary(&MintSimulation {
                    minted: Uint128::new(self.minted_for(amount.u128())),
                    imbalance_fee: Uint128::zero(),
                    imbalance_rebate: Uint128::zero(),
                    allocation: vec![],
                })
            }
            HatomQueryMsg::SimulateBurn { amount, .. } => {
                to_json_binary(&self.simulate_burn(amount.u128())?)
            }
        }
        .map_err(err)
    }

    fn hatom_execute(
        &mut self,
        sender: &str,
        msg: &Binary,
        funds: &[Coin],
    ) -> Result<Binary, String> {
        let me = self.hatom_addr.clone();
        self.send(sender, &me, funds)?;
        if self.hatom.paused {
            return Err(err("the contract is paused"));
        }
        match from_json(msg).map_err(err)? {
            HatomExecuteMsg::MintFromNative { recipient, .. } => {
                let amount = funds.iter().find(|c| c.denom == BOND).ok_or("no ATOM")?;
                let minted = self.minted_for(amount.amount.u128());
                self.hatom.supply += minted;
                self.hatom.holders_value += amount.amount.u128();
                self.mint(&recipient.unwrap_or(sender.to_string()), HATOM, minted);
                to_json_binary(&MintResult {
                    minted: Uint128::new(minted),
                })
                .map_err(err)
            }
            HatomExecuteMsg::Burn {
                recipient, min_out, ..
            } => {
                let amount = funds.iter().find(|c| c.denom == HATOM).ok_or("no hATOM")?;
                let a = amount.amount.u128();
                let sim = self.simulate_burn(a)?;
                if let Some(m) = min_out {
                    if sim.out < m {
                        return Err(err("below minimum"));
                    }
                }
                self.burn(&me, HATOM, a)?;
                self.hatom.supply -= a;
                self.hatom.holders_value -= sim.out.u128();
                let recipient = recipient.unwrap_or(sender.to_string());
                let mut shares = vec![];
                for alloc in sim.allocation {
                    self.hatom.next_record += 1;
                    let denom = format!("{}/{}", alloc.validator, self.hatom.next_record);
                    self.mint(&recipient, &denom, alloc.amount.u128());
                    shares.push(Coin::new(alloc.amount, denom));
                }
                to_json_binary(&BurnResult { shares }).map_err(err)
            }
            HatomExecuteMsg::Compound {} => {
                if self.hatom.compound_refused {
                    return Err(err("compound refused"));
                }
                self.hatom.compounds += 1;
                self.hatom.holders_value += self.hatom.compound_gain;
                Ok(Binary::default())
            }
        }
    }

    // ---- vault

    fn pool_hatom(&self) -> u128 {
        self.bal(&self.vault_addr, HATOM) + self.vault.deployed
    }

    fn sev(&self, shares: u128) -> u128 {
        if self.vault.issued == 0 {
            return 0;
        }
        (Uint256::from(shares) * Uint256::from(self.pool_hatom())
            / Uint256::from(self.vault.issued))
        .try_into()
        .map(|v: Uint128| v.u128())
        .unwrap_or(u128::MAX)
    }

    fn shares_for_deposit(&self, amount: u128, pool_before: u128) -> u128 {
        if self.vault.issued == 0 || pool_before == 0 {
            amount
        } else {
            amount * self.vault.issued / pool_before
        }
    }

    fn vault_query(&self, msg: &Binary) -> Result<Binary, String> {
        match from_json(msg).map_err(err)? {
            VaultQueryMsg::Config {} => to_json_binary(&VaultConfigResponse {
                config: VaultConfig {
                    deposit_denom: HATOM.to_string(),
                    vault_shares_denom: SHARES.to_string(),
                    control_center_contract: Addr::unchecked(&self.cc_addr),
                    token_info_provider_contract: Some(Addr::unchecked(&self.hatom_addr)),
                    max_withdrawals_per_user: 10,
                },
            }),
            VaultQueryMsg::SharesEquivalentValue { shares } => {
                // As the vault: refused above the shares issued.
                if shares.u128() > self.vault.issued {
                    return Err(format!("invalid shares amount; shares sent: {shares}"));
                }
                to_json_binary(&Uint128::new(self.sev(shares.u128())))
            }
            VaultQueryMsg::ControlCenterPoolInfo {} => {
                return self.cc_query(&to_json_binary(&CcQueryMsg::PoolInfo {}).map_err(err)?)
            }
            VaultQueryMsg::DryRunDeposit { amount } => to_json_binary(&Uint128::new(
                self.shares_for_deposit(amount.u128(), self.pool_hatom()),
            )),
            VaultQueryMsg::WithdrawalQueueInfo {} => to_json_binary(&WithdrawalQueueInfoResponse {
                info: WithdrawalQueueInfo {
                    total_shares_burned: Uint128::zero(),
                    total_withdrawal_amount: Uint128::new(self.vault.non_funded),
                    non_funded_withdrawal_amount: Uint128::new(self.vault.non_funded),
                },
            }),
            VaultQueryMsg::AvailableForDeployment {} => {
                to_json_binary(&Uint128::new(self.bal(&self.vault_addr, HATOM)))
            }
            _ => return Err(err("unsupported vault query")),
        }
        .map_err(err)
    }

    fn cc_query(&self, msg: &Binary) -> Result<Binary, String> {
        match from_json(msg).map_err(err)? {
            CcQueryMsg::Config {} => to_json_binary(&CcConfigResponse {
                config: CcConfig {
                    deposit_cap: Uint128::new(self.vault.deposit_cap),
                },
            }),
            CcQueryMsg::PoolInfo {} => to_json_binary(&CcPoolInfo {
                total_pool_value: Uint128::new(self.hatom_value(self.pool_hatom())),
                total_shares_issued: Uint128::new(self.vault.issued),
            }),
            _ => return Err(err("unsupported control center query")),
        }
        .map_err(err)
    }

    fn vault_execute(
        &mut self,
        sender: &str,
        msg: &Binary,
        funds: &[Coin],
    ) -> Result<Binary, String> {
        let me = self.vault_addr.clone();
        let pool_before = self.pool_hatom();
        self.send(sender, &me, funds)?;
        if self.vault.paused {
            return Err(err("vault paused"));
        }
        match from_json(msg).map_err(err)? {
            VaultExecuteMsg::Deposit { on_behalf_of } => {
                let a = funds.iter().find(|c| c.denom == HATOM).ok_or("no hATOM")?;
                if self.hatom_value(self.pool_hatom()) > self.vault.deposit_cap {
                    return Err(err("deposit cap has been reached"));
                }
                let shares = self.shares_for_deposit(a.amount.u128(), pool_before);
                self.vault.issued += shares;
                self.mint(&on_behalf_of.unwrap_or(sender.to_string()), SHARES, shares);
                Ok(Binary::default())
            }
            VaultExecuteMsg::Withdraw { on_behalf_of } => {
                let s = funds
                    .iter()
                    .find(|c| c.denom == SHARES)
                    .ok_or("no shares")?;
                let s = s.amount.u128();
                let h = self.sev(s);
                self.burn(&me, SHARES, s)?;
                self.vault.issued -= s;
                if self.vault.queue_withdrawals {
                    self.vault.non_funded += h;
                } else {
                    let to = on_behalf_of.unwrap_or(sender.to_string());
                    self.send(&me, &to, &[Coin::new(h, HATOM)])?;
                }
                Ok(Binary::default())
            }
            _ => Err(err("unsupported vault message")),
        }
    }

    // ---- staking

    pub fn live_entries(&self, delegator: &str, validator: &str) -> usize {
        self.unbonding
            .iter()
            .filter(|u| u.delegator == delegator && u.validator == validator)
            .count()
    }

    fn undelegate(
        &mut self,
        delegator: &str,
        validator: &str,
        amount: &Coin,
    ) -> Result<MsgResponse, String> {
        if self.refuse_undelegate.contains(validator) {
            return Err(err("undelegation refused"));
        }
        let completion = self.time.plus_seconds(self.unbonding_time_s);
        let merges = self.unbonding.iter().any(|u| {
            u.delegator == delegator && u.validator == validator && u.completion == completion
        });
        // The SDK checks `max_entries` before merging an entry of the same block.
        if self.live_entries(delegator, validator) >= self.max_entries {
            return Err(err(
                "too many unbonding delegation entries for (delegator, validator) tuple",
            ));
        }
        let key = (delegator.to_string(), validator.to_string());
        let d = self.delegations.get_mut(&key).ok_or("no delegation")?;
        if *d < amount.amount.u128() {
            return Err(err("not enough delegation"));
        }
        *d -= amount.amount.u128();
        if *d == 0 {
            self.delegations.remove(&key);
        }
        if merges {
            let u = self
                .unbonding
                .iter_mut()
                .find(|u| {
                    u.delegator == delegator
                        && u.validator == validator
                        && u.completion == completion
                })
                .unwrap();
            u.amount += amount.amount.u128();
        } else {
            self.unbonding.push(Unbonding {
                delegator: delegator.to_string(),
                validator: validator.to_string(),
                completion,
                amount: amount.amount.u128(),
            });
        }
        self.mint(delegator, BOND, self.auto_claim);
        let r = proto::MsgUndelegateResponse {
            completion_time: Some(proto::Timestamp {
                seconds: completion.seconds() as i64,
                nanos: completion.subsec_nanos() as i32,
            }),
            amount: Some(proto::Coin::from(amount)),
        };
        Ok(MsgResponse {
            type_url: chain::UNDELEGATE_RESPONSE_TYPE_URL.to_string(),
            value: Binary::new(r.encode_to_vec()),
        })
    }

    fn any(&mut self, sender: &str, type_url: &str, value: &Binary) -> Result<MsgResponse, String> {
        if type_url != chain::REDEEM_TYPE_URL {
            return Err(format!("unsupported Any message {type_url}"));
        }
        let m = proto::MsgRedeemTokensForShares::decode(value.as_slice()).map_err(err)?;
        if m.delegator_address != sender {
            return Err(err("redeem signer mismatch"));
        }
        let coin = m.amount.ok_or("no amount")?.to_std().map_err(err)?;
        let (validator, _) = coin.denom.split_once('/').ok_or("not an LSM share")?;
        self.burn(sender, &coin.denom, coin.amount.u128())?;
        *self
            .delegations
            .entry((sender.to_string(), validator.to_string()))
            .or_default() += coin.amount.u128();
        self.mint(sender, BOND, self.auto_claim);
        let r = proto::MsgRedeemTokensForSharesResponse {
            amount: Some(proto::Coin {
                denom: BOND.to_string(),
                amount: coin.amount.to_string(),
            }),
        };
        Ok(MsgResponse {
            type_url: chain::REDEEM_RESPONSE_TYPE_URL.to_string(),
            value: Binary::new(r.encode_to_vec()),
        })
    }

    /// Pays the entries that reached their completion (as the EndBlocker does), less the slash.
    pub fn mature(&mut self) {
        let now = self.time;
        let (done, left): (Vec<_>, Vec<_>) =
            self.unbonding.drain(..).partition(|u| u.completion <= now);
        self.unbonding = left;
        for u in done {
            let paid = u.amount - u.amount * self.slash_bps / 10_000;
            self.mint(&u.delegator, BOND, paid);
        }
    }

    fn query(&self, req: &QueryRequest<Empty>) -> QuerierResult {
        let res: Result<Binary, String> = match req {
            QueryRequest::Bank(BankQuery::Balance { address, denom }) => to_json_binary(
                &BalanceResponse::new(Coin::new(self.bal(address, denom), denom)),
            )
            .map_err(err),
            QueryRequest::Staking(StakingQuery::BondedDenom {}) => {
                to_json_binary(&BondedDenomResponse::new(BOND.to_string())).map_err(err)
            }
            QueryRequest::Staking(StakingQuery::AllDelegations { delegator }) => {
                let delegations = self
                    .delegations
                    .iter()
                    .filter(|((d, _), _)| d == delegator)
                    .map(|((d, v), a)| {
                        Delegation::new(Addr::unchecked(d), v.clone(), Coin::new(*a, BOND))
                    })
                    .collect();
                to_json_binary(&AllDelegationsResponse::new(delegations)).map_err(err)
            }
            QueryRequest::Wasm(WasmQuery::Smart { contract_addr, msg }) => {
                if *contract_addr == self.hatom_addr {
                    self.hatom_query(msg)
                } else if *contract_addr == self.vault_addr {
                    self.vault_query(msg)
                } else if *contract_addr == self.cc_addr {
                    self.cc_query(msg)
                } else {
                    return SystemResult::Err(SystemError::NoSuchContract {
                        addr: contract_addr.clone(),
                    });
                }
            }
            _ => {
                return SystemResult::Err(SystemError::UnsupportedRequest {
                    kind: format!("{req:?}"),
                })
            }
        };
        SystemResult::Ok(match res {
            Ok(b) => ContractResult::Ok(b),
            Err(e) => ContractResult::Err(e),
        })
    }
}

/// hATOM's `Config` carries many more fields than the gate reads: some are added here to check
/// that the gate ignores them.
fn serde_json_like_config(paused: bool) -> serde_json_value::Config {
    serde_json_value::Config {
        denom: HATOM.to_string(),
        paused,
        fee_bps: 0,
        operators: vec![],
    }
}

mod serde_json_value {
    use serde::Serialize;
    #[derive(Serialize)]
    pub struct Config {
        pub denom: String,
        pub paused: bool,
        pub fee_bps: u32,
        pub operators: Vec<String>,
    }
}

pub struct WorldQuerier(pub Rc<RefCell<World>>);

impl Querier for WorldQuerier {
    fn raw_query(&self, bin_request: &[u8]) -> QuerierResult {
        match from_json::<QueryRequest<Empty>>(bin_request) {
            Ok(req) => self.0.borrow().query(&req),
            Err(e) => SystemResult::Err(SystemError::InvalidRequest {
                error: e.to_string(),
                request: Binary::from(bin_request),
            }),
        }
    }
}

pub struct App {
    pub deps: OwnedDeps<MockStorage, MockApi, WorldQuerier>,
    pub world: Rc<RefCell<World>>,
    pub api: MockApi,
    pub gate: Addr,
    pub admin: Addr,
    pub operator: Addr,
    pub keeper: Addr,
    pub profit: Addr,
    /// Attributes of the last transaction (every level).
    pub attrs: Vec<(String, String)>,
}

type Snapshot = (Vec<(Vec<u8>, Vec<u8>)>, World);

impl App {
    pub fn new(settings: Settings) -> App {
        let api = MockApi::default();
        let gate = api.addr_make("gate");
        let hatom = api.addr_make("hatom");
        let vault = api.addr_make("vault");
        let cc = api.addr_make("control_center");
        let world = World {
            time: Timestamp::from_seconds(START),
            height: 1,
            gate: gate.to_string(),
            hatom_addr: hatom.to_string(),
            vault_addr: vault.to_string(),
            cc_addr: cc.to_string(),
            unbonding_time_s: 21 * 86_400,
            max_entries: 7,
            hatom: HatomMock {
                // 1 hATOM = 1.25 ATOM.
                holders_value: 1_250_000_000_000,
                supply: 1_000_000_000_000,
                burn_legs: vec!["valoper_a".into(), "valoper_b".into()],
                ..Default::default()
            },
            vault: VaultMock {
                deposit_cap: u128::MAX / 4,
                ..Default::default()
            },
            ..Default::default()
        };
        let world = Rc::new(RefCell::new(world));
        let deps = OwnedDeps {
            storage: MockStorage::default(),
            api,
            querier: WorldQuerier(world.clone()),
            custom_query_type: std::marker::PhantomData,
        };
        let mut app = App {
            deps,
            world,
            api,
            gate: gate.clone(),
            admin: api.addr_make("admin"),
            operator: api.addr_make("operator"),
            keeper: api.addr_make("keeper"),
            profit: api.addr_make("profit"),
            attrs: vec![],
        };
        // The vault: 1,000,000 hATOM against 1,000,000 shares... at 1.1 hATOM per share.
        {
            let mut w = app.world.borrow_mut();
            let v = w.vault_addr.clone();
            w.mint(&v, HATOM, 1_100_000_000_000);
            w.vault.issued = 1_000_000_000_000;
        }
        let msg = InstantiateMsg {
            admin: app.admin.to_string(),
            operators: vec![app.operator.to_string()],
            keeper: app.keeper.to_string(),
            profit_recipient: app.profit.to_string(),
            hatom: hatom.to_string(),
            vault: vault.to_string(),
            settings,
        };
        let info = MessageInfo {
            sender: app.admin.clone(),
            funds: vec![],
        };
        let env = app.env();
        contract::instantiate(app.deps.as_mut(), env, info, msg).unwrap();
        app
    }

    pub fn env(&self) -> Env {
        let w = self.world.borrow();
        let mut env = mock_env();
        env.block.time = w.time;
        env.block.height = w.height;
        env.contract.address = self.gate.clone();
        env
    }

    pub fn addr(&self, name: &str) -> Addr {
        self.api.addr_make(name)
    }

    pub fn bal(&self, addr: &Addr, denom: &str) -> u128 {
        self.world.borrow().bal(addr.as_str(), denom)
    }

    pub fn fund(&self, addr: &Addr, denom: &str, amount: u128) {
        self.world.borrow_mut().mint(addr.as_str(), denom, amount);
    }

    /// Gives `addr` vault shares the way a depositor gets them (hATOM into the vault).
    pub fn give_shares(&self, addr: &Addr, shares: u128) {
        let mut w = self.world.borrow_mut();
        let h = w.sev(shares);
        let v = w.vault_addr.clone();
        w.mint(&v, HATOM, h);
        w.vault.issued += shares;
        w.mint(addr.as_str(), SHARES, shares);
    }

    /// Moves time (and one block) forward; matured entries are paid.
    pub fn advance(&self, seconds: u64) {
        let mut w = self.world.borrow_mut();
        w.time = w.time.plus_seconds(seconds);
        w.height += 1;
        w.mature();
    }

    pub fn query<T: DeserializeOwned>(&self, msg: QueryMsg) -> T {
        from_json(contract::query(self.deps.as_ref(), self.env(), msg).unwrap()).unwrap()
    }

    pub fn attr(&self, key: &str) -> Option<String> {
        self.attrs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    }

    pub fn attrs_all(&self, key: &str) -> Vec<String> {
        self.attrs
            .iter()
            .filter(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
            .collect()
    }

    fn snapshot(&self) -> Snapshot {
        (
            self.deps
                .storage
                .range(None, None, Order::Ascending)
                .collect(),
            self.world.borrow().clone(),
        )
    }

    fn restore(&mut self, s: Snapshot) {
        let mut storage = MockStorage::default();
        for (k, v) in s.0 {
            storage.set(&k, &v);
        }
        self.deps.storage = storage;
        *self.world.borrow_mut() = s.1;
    }

    /// One transaction: rolled back as a whole on error.
    pub fn execute(
        &mut self,
        sender: &Addr,
        msg: ExecuteMsg,
        funds: &[Coin],
    ) -> Result<(), String> {
        self.attrs.clear();
        let snap = self.snapshot();
        let r = self.run_execute(sender, msg, funds);
        if r.is_err() {
            self.restore(snap);
        }
        r.map(|_| ())
    }

    fn run_execute(
        &mut self,
        sender: &Addr,
        msg: ExecuteMsg,
        funds: &[Coin],
    ) -> Result<Option<Binary>, String> {
        self.world
            .borrow_mut()
            .send(sender.as_str(), self.gate.as_str(), funds)?;
        let info = MessageInfo {
            sender: sender.clone(),
            funds: funds.to_vec(),
        };
        let env = self.env();
        let res =
            contract::execute(self.deps.as_mut(), env, info, msg).map_err(|e| e.to_string())?;
        self.process(res)
    }

    fn process(&mut self, res: Response) -> Result<Option<Binary>, String> {
        for a in &res.attributes {
            self.attrs.push((a.key.clone(), a.value.clone()));
        }
        let mut data = res.data.clone();
        for sub in res.messages {
            let snap =
                matches!(sub.reply_on, ReplyOn::Error | ReplyOn::Always).then(|| self.snapshot());
            let r = self.dispatch(&sub.msg);
            let result = match (r, &sub.reply_on) {
                (Ok(responses), ReplyOn::Success | ReplyOn::Always) =>
                {
                    #[allow(deprecated)]
                    SubMsgResult::Ok(SubMsgResponse {
                        events: vec![],
                        data: None,
                        msg_responses: responses,
                    })
                }
                (Ok(_), _) => continue,
                (Err(e), ReplyOn::Error | ReplyOn::Always) => {
                    self.restore(snap.unwrap());
                    SubMsgResult::Err(e)
                }
                (Err(e), _) => return Err(e),
            };
            let reply = Reply {
                id: sub.id,
                payload: sub.payload.clone(),
                gas_used: 0,
                result,
            };
            let env = self.env();
            let res2 =
                contract::reply(self.deps.as_mut(), env, reply).map_err(|e| e.to_string())?;
            if let Some(d) = self.process(res2)? {
                data = Some(d);
            }
        }
        Ok(data)
    }

    fn dispatch(&mut self, msg: &CosmosMsg) -> Result<Vec<MsgResponse>, String> {
        let gate = self.gate.to_string();
        let exec_response = |data: Binary| MsgResponse {
            type_url: chain::EXECUTE_RESPONSE_TYPE_URL.to_string(),
            value: Binary::new(
                proto::MsgExecuteContractResponse {
                    data: data.to_vec(),
                }
                .encode_to_vec(),
            ),
        };
        match msg {
            CosmosMsg::Bank(BankMsg::Send { to_address, amount }) => {
                self.world.borrow_mut().send(&gate, to_address, amount)?;
                Ok(vec![])
            }
            CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr,
                msg,
                funds,
            }) => {
                let data = if *contract_addr == gate {
                    let m: ExecuteMsg = from_json(msg).map_err(err)?;
                    let g = self.gate.clone();
                    self.run_execute(&g, m, funds)?.unwrap_or_default()
                } else {
                    let mut w = self.world.borrow_mut();
                    if *contract_addr == w.hatom_addr {
                        w.hatom_execute(&gate, msg, funds)?
                    } else if *contract_addr == w.vault_addr {
                        w.vault_execute(&gate, msg, funds)?
                    } else {
                        return Err(format!("no contract {contract_addr}"));
                    }
                };
                Ok(vec![exec_response(data)])
            }
            CosmosMsg::Staking(StakingMsg::Undelegate { validator, amount }) => Ok(vec![self
                .world
                .borrow_mut()
                .undelegate(&gate, validator, amount)?]),
            CosmosMsg::Any(any) => Ok(vec![self.world.borrow_mut().any(
                &gate,
                &any.type_url,
                &any.value,
            )?]),
            m => Err(format!("unsupported message {m:?}")),
        }
    }
}
