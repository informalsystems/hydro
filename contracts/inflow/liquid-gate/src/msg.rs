use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Coin, Timestamp, Uint128};

use crate::hatom::Allocation;
use crate::state::{Batch, Booking, Config, Totals};

/// One point of a curve: the value `bps` (a discount or a bonus) at the buffer fill `f_bps`.
#[cw_serde]
#[derive(Copy)]
pub struct CurvePoint {
    pub f_bps: u32,
    pub bps: u32,
}

/// The settings of SPEC §2, each checked against its bounds (`Settings::validate`).
#[cw_serde]
pub struct Settings {
    /// ATOM at which the buffer counts as full (`f = 1`).
    pub buffer_target: Uint128,
    /// The instant discount as a function of the buffer fill.
    pub exit_curve: Vec<CurvePoint>,
    /// Floor of the instant discount.
    pub d_min_bps: u32,
    /// Floor of the booking discount.
    pub d_book_min_bps: u32,
    /// The swap entry bonus as a function of the buffer fill.
    pub entry_curve: Vec<CurvePoint>,
    /// The chain's `unbonding_time` (bounds and quotes only).
    pub unbonding_time_s: u64,
    /// The chain's staking `max_entries`.
    pub max_entries: u32,
    /// `T` in the booking curve.
    pub book_horizon_s: u64,
    /// Slash margin on batch capacity.
    pub max_slash_bps: u32,
    /// Largest exit, in ATOM value, per transaction.
    pub exit_cap_tx: Uint128,
    /// Largest total exit value per window.
    pub exit_cap_window: Uint128,
    pub window_s: u64,
    /// Largest share inventory, in ATOM value.
    pub inventory_cap: Uint128,
    pub price_jump_bps: u32,
    pub price_drift_bps_per_day: u32,
    pub reset_tolerance_bps: u32,
    /// Least time between two batches of the same validator.
    pub batch_interval_s: u64,
    pub batch_target_min_bps: u32,
    pub batch_target_max_bps: u32,
    /// `max_legs` passed to hATOM's `Burn`, and the most legs a burn may have.
    pub burn_max_legs: u32,
    /// Largest imbalance fee an `Unwind`'s burn may pay, in bps of its value.
    pub unwind_max_fee_bps: u32,
    /// Most undelegations in one `ProcessBatch`.
    pub undelegate_max_validators: u32,
}

/// `UpdateConfig`: every field optional; the result is checked as a whole.
#[cw_serde]
#[derive(Default)]
pub struct SettingsUpdate {
    pub profit_recipient: Option<String>,
    pub buffer_target: Option<Uint128>,
    pub exit_curve: Option<Vec<CurvePoint>>,
    pub d_min_bps: Option<u32>,
    pub d_book_min_bps: Option<u32>,
    pub entry_curve: Option<Vec<CurvePoint>>,
    pub unbonding_time_s: Option<u64>,
    pub max_entries: Option<u32>,
    pub book_horizon_s: Option<u64>,
    pub max_slash_bps: Option<u32>,
    pub exit_cap_tx: Option<Uint128>,
    pub exit_cap_window: Option<Uint128>,
    pub window_s: Option<u64>,
    pub inventory_cap: Option<Uint128>,
    pub price_jump_bps: Option<u32>,
    pub price_drift_bps_per_day: Option<u32>,
    pub reset_tolerance_bps: Option<u32>,
    pub batch_interval_s: Option<u64>,
    pub batch_target_min_bps: Option<u32>,
    pub batch_target_max_bps: Option<u32>,
    pub burn_max_legs: Option<u32>,
    pub unwind_max_fee_bps: Option<u32>,
    pub undelegate_max_validators: Option<u32>,
}

#[cw_serde]
pub struct InstantiateMsg {
    pub admin: String,
    pub operators: Vec<String>,
    pub keeper: String,
    pub profit_recipient: String,
    /// The hATOM contract.
    pub hatom: String,
    /// The Inflow vault contract (deposit denom: hATOM).
    pub vault: String,
    pub settings: Settings,
}

#[cw_serde]
#[allow(clippy::large_enum_variant)]
pub enum ExecuteMsg {
    // ---- Trades (anyone).
    /// ATOM in: mint hATOM and deposit it into the vault for `recipient`. `min_out` in shares.
    EnterDirect {
        recipient: Option<String>,
        min_out: Option<Uint128>,
    },
    /// ATOM in: shares from the inventory, with the entry bonus. `min_out` in shares.
    EnterSwap {
        recipient: Option<String>,
        min_out: Option<Uint128>,
    },
    /// Shares in: ATOM now from the free buffer, less the exit discount. `min_out` in ATOM.
    ExitInstant {
        recipient: Option<String>,
        min_out: Option<Uint128>,
    },
    /// Shares in: ATOM owed at the maturity of batch `batch_id`. `min_out` in ATOM.
    BookExit {
        batch_id: u64,
        recipient: Option<String>,
        min_out: Option<Uint128>,
    },
    /// Anyone: pays the listed bookings of matured batches to their owners. Ids of batches not
    /// matured yet (`not_matured`) and unknown or already claimed ids (`not_found`) are skipped.
    ClaimBookings {
        booking_ids: Vec<u64>,
    },

    // ---- Batch steps (anyone).
    /// Withdraw shares from the vault, burn the hATOM, redeem the LSM shares.
    Unwind {
        shares: Option<Uint128>,
    },
    /// Undelegate the gate's delegations into a new batch.
    ProcessBatch {},

    // ---- Roles.
    UpdateConfig {
        settings: SettingsUpdate,
    },
    SetOperators {
        operators: Vec<String>,
    },
    SetKeeper {
        keeper: String,
    },
    TransferAdmin {
        admin: String,
    },
    AcceptAdmin {},
    /// Operator or admin: refuses `EnterSwap`, `EnterDirect`, `ExitInstant`, `BookExit`, `Unwind`
    /// and `ProcessBatch`. Never `ClaimBookings`: a pause never blocks what users are owed.
    Pause {},
    /// Admin only.
    Unpause {},
    /// Admin or operator: `price` is the share price (ATOM per 10^6 share units) they verified.
    ResetPriceReference {
        price: Uint128,
    },
    /// Keeper or admin: the shares to unwind per interval.
    SetBatchTarget {
        shares: Uint128,
    },
    /// Admin: ATOM from the free buffer and shares from the inventory, to `profit_recipient`.
    /// Shares are checked only against the inventory and leave at their average cost basis,
    /// whatever their value: withdrawing them below cost is allowed on purpose (it is Hydro's
    /// own inventory).
    WithdrawProfit {
        atom: Option<Uint128>,
        shares: Option<Uint128>,
    },
    /// Admin: a stray balance to `profit_recipient`.
    Sweep {
        denom: String,
    },

    /// Internal: the final self-call of every operation that changes balances (SPEC §9). Only
    /// the contract itself may call it (`Unauthorized` otherwise).
    AssertInvariants {
        swap: Option<SwapCheck>,
    },
}

/// What a swap entry received against the cost basis of the shares it gave (invariant 4).
#[cw_serde]
pub struct SwapCheck {
    pub received: Uint128,
    pub cost_out: Uint128,
}

#[cw_serde]
pub enum EntryRoute {
    Direct,
    Swap,
}

#[cw_serde]
pub enum ExitRoute {
    Instant,
    Booked { batch_id: u64 },
    Staked,
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(Config)]
    Config {},
    #[returns(QuoteEntryResponse)]
    QuoteEntry { amount: Uint128, route: EntryRoute },
    #[returns(QuoteExitResponse)]
    QuoteExit { shares: Uint128, route: ExitRoute },
    /// Batches in flight (not matured yet).
    #[returns(BatchesResponse)]
    Batches {
        start_after: Option<u64>,
        limit: Option<u32>,
    },
    #[returns(BookingsResponse)]
    Bookings {
        owner: Option<String>,
        batch_id: Option<u64>,
        start_after: Option<u64>,
        limit: Option<u32>,
    },
    #[returns(PositionResponse)]
    Position {},
    /// Live unbonding entries per validator.
    #[returns(EntriesResponse)]
    Entries { validator: Option<String> },
}

#[cw_serde]
pub struct MigrateMsg {}

#[cw_serde]
#[derive(Default)]
pub struct QuoteEntryResponse {
    /// False with the reason when the route cannot be taken now.
    pub available: bool,
    pub reason: Option<String>,
    /// Vault shares out.
    pub shares: Uint128,
    /// `swap`: the averaged entry bonus.
    pub bonus_bps: Option<u32>,
    /// `direct`: the hATOM minted and the mint's imbalance fee or rebate.
    pub hatom_minted: Option<Uint128>,
    pub imbalance_fee: Option<Uint128>,
    pub imbalance_rebate: Option<Uint128>,
    /// `direct`: whether the vault's deposit cap allows the deposit.
    pub within_deposit_cap: Option<bool>,
}

#[cw_serde]
#[derive(Default)]
pub struct QuoteExitResponse {
    pub available: bool,
    pub reason: Option<String>,
    /// ATOM out (`staked`: the bond-denom value of the LSM shares).
    pub atom: Uint128,
    /// The shares' value `V` (instant, booked).
    pub value: Option<Uint128>,
    /// `instant`: the averaged discount; `booked`: `d(t)`.
    pub discount_bps: Option<u32>,
    /// `booked`: the batch's maturity and its capacity left.
    pub maturity: Option<Timestamp>,
    pub capacity_left: Option<Uint128>,
    /// `staked`: the hATOM the vault pays, the LSM shares of the burn, its fee or bonus, and
    /// whether the vault would pay the withdrawal at once (conservative: from
    /// `AvailableForDeployment`, which leaves out what automated adapters could pay).
    pub hatom: Option<Uint128>,
    pub lsm_shares: Option<Vec<Allocation>>,
    pub imbalance_fee: Option<Uint128>,
    pub imbalance_rebate: Option<Uint128>,
    pub instant_withdrawal: Option<bool>,
}

#[cw_serde]
pub struct BatchInfo {
    pub id: u64,
    pub created: Timestamp,
    pub completion: Timestamp,
    pub amount: Uint128,
    pub capacity: Uint128,
    pub booked: Uint128,
    pub capacity_left: Uint128,
}

impl From<&Batch> for BatchInfo {
    fn from(b: &Batch) -> Self {
        BatchInfo {
            id: b.id,
            created: b.created,
            completion: b.completion,
            amount: b.amount,
            capacity: b.capacity,
            booked: b.booked,
            capacity_left: b.capacity.saturating_sub(b.booked),
        }
    }
}

#[cw_serde]
pub struct BatchesResponse {
    pub batches: Vec<BatchInfo>,
}

#[cw_serde]
pub struct BookingsResponse {
    pub bookings: Vec<Booking>,
}

#[cw_serde]
pub struct PositionResponse {
    pub atom_balance: Uint128,
    pub free: Uint128,
    pub reserved: Uint128,
    pub inventory_shares: Uint128,
    /// `V(inventory_shares)`.
    pub inventory_value: Uint128,
    pub inventory_cost: Uint128,
    /// The gate's delegations not yet undelegated.
    pub delegated: Vec<Coin>,
    pub batches: Vec<BatchInfo>,
    pub totals: Totals,
    pub price_ref: Option<Uint128>,
    pub price_ref_time: Option<Timestamp>,
    pub batch_target: Uint128,
    pub unwound_in_interval: Uint128,
}

#[cw_serde]
pub struct ValidatorEntries {
    pub validator: String,
    /// `(completion_time, batch_id)` of each live entry.
    pub entries: Vec<(Timestamp, u64)>,
}

#[cw_serde]
pub struct EntriesResponse {
    pub validators: Vec<ValidatorEntries>,
}
