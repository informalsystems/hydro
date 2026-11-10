//! Storage (SPEC §7).

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Timestamp, Uint128};
use cw_storage_plus::{Item, Map};

use crate::msg::Settings;

#[cw_serde]
pub struct Config {
    /// The gate's admin (the Hydro DAO); not the wasm admin, which migrates the code.
    pub admin: Addr,
    pub pending_admin: Option<Addr>,
    pub operators: Vec<Addr>,
    pub keeper: Addr,
    pub profit_recipient: Addr,
    pub hatom: Addr,
    pub vault: Addr,
    /// Read at instantiate: hATOM's denom (the vault's deposit denom), the vault's share denom
    /// and the chain's bond denom.
    pub hatom_denom: String,
    pub share_denom: String,
    pub bond_denom: String,
    pub paused: bool,
    pub settings: Settings,
}

/// Shares the gate holds for swap entries and unwinding, and the ATOM it paid for them.
#[cw_serde]
#[derive(Default)]
pub struct Inventory {
    pub shares: Uint128,
    pub cost: Uint128,
}

#[cw_serde]
pub struct Batch {
    pub id: u64,
    pub created: Timestamp,
    /// The latest `completion_time` of its undelegations.
    pub completion: Timestamp,
    /// The ATOM its undelegations returned.
    pub amount: Uint128,
    /// `⌊amount × (10,000 - max_slash_bps) / 10,000⌋`, fixed at creation.
    pub capacity: Uint128,
    pub booked: Uint128,
    /// Bookings of this batch paid so far.
    pub claimed: Uint128,
}

#[cw_serde]
pub struct Booking {
    pub id: u64,
    pub batch_id: u64,
    pub owner: Addr,
    /// ATOM owed at the batch's maturity.
    pub amount: Uint128,
}

/// `cursor`: the last `(completion, id)` of `BY_COMPLETION` whose maturity has passed;
/// `reserved`: the unpaid bookings of every batch up to the cursor.
#[cw_serde]
#[derive(Default)]
pub struct Matured {
    pub cursor: Option<(u64, u64)>,
    pub reserved: Uint128,
}

/// Running totals (SPEC §8): raw units, only ever increased, for reporting only.
#[cw_serde]
#[derive(Default)]
pub struct Totals {
    pub discount_earned: Uint128,
    pub bonus_paid: Uint128,
    pub rewards_received: Uint128,
    pub burn_bonus_received: Uint128,
    pub burn_fee_paid: Uint128,
    pub unbonded: Uint128,
    pub withdrawn_atom: Uint128,
    pub withdrawn_shares: Uint128,
}

#[cw_serde]
pub struct PriceRef {
    /// `P`: ATOM per 10^6 share units.
    pub price: Uint128,
    pub time: Timestamp,
}

/// The rolling exit window.
#[cw_serde]
pub struct ExitWindow {
    pub start: Timestamp,
    pub used: Uint128,
}

/// An `EnterDirect` between its execute and its mint reply.
#[cw_serde]
pub struct PendingDirect {
    pub recipient: Addr,
    pub min_out: Option<Uint128>,
}

/// An `Unwind` between its execute and its final self-call.
#[cw_serde]
pub struct PendingUnwind {
    pub shares: Uint128,
    /// The gate's ATOM balance at the start.
    pub atom_before: Uint128,
    /// `SharesEquivalentValue(shares)`, read after the Compound.
    pub expected: Uint128,
    /// The gate's hATOM balance just before the withdrawal.
    pub hatom_before: Uint128,
}

/// A `ProcessBatch` between its execute and its final self-call.
#[cw_serde]
pub struct PendingBatch {
    pub batch_id: u64,
    pub atom_before: Uint128,
    /// The validators undelegated from, in submessage order (reply id offset).
    pub validators: Vec<String>,
    /// `(validator, completion_time, amount)` of each successful undelegation.
    pub done: Vec<(String, Timestamp, Uint128)>,
}

pub const CONFIG: Item<Config> = Item::new("config");
/// Set by the operations that send submessages with replies, cleared by their final self-call.
pub const OP_GUARD: Item<()> = Item::new("op_guard");
pub const INVENTORY: Item<Inventory> = Item::new("inventory");
pub const BATCHES: Map<u64, Batch> = Map::new("batches");
/// Index of `BATCHES` by `(completion in nanoseconds, id)`.
pub const BY_COMPLETION: Map<(u64, u64), ()> = Map::new("by_completion");
pub const BOOKINGS: Map<u64, Booking> = Map::new("bookings");
pub const BOOKINGS_BY_BATCH: Map<(u64, u64), ()> = Map::new("bookings_by_batch");
pub const BOOKINGS_BY_OWNER: Map<(&Addr, u64), ()> = Map::new("bookings_by_owner");
/// The gate's own unbonding entries: `(completion_time, batch_id)` per validator.
pub const ENTRIES: Map<&str, Vec<(Timestamp, u64)>> = Map::new("entries");
pub const MATURED: Item<Matured> = Item::new("matured");
pub const TOTALS: Item<Totals> = Item::new("totals");
pub const PRICE_REF: Item<PriceRef> = Item::new("price_ref");
pub const EXIT_WINDOW: Item<ExitWindow> = Item::new("exit_window");
pub const BATCH_TARGET: Item<Uint128> = Item::new("batch_target");
pub const UNWOUND_IN_INTERVAL: Item<Uint128> = Item::new("unwound_in_interval");
/// Creation time of the latest batch.
pub const LAST_BATCH_TIME: Item<Timestamp> = Item::new("last_batch_time");
pub const NEXT_BATCH_ID: Item<u64> = Item::new("next_batch_id");
pub const NEXT_BOOKING_ID: Item<u64> = Item::new("next_booking_id");
pub const PENDING_DIRECT: Item<PendingDirect> = Item::new("pending_direct");
pub const PENDING_UNWIND: Item<PendingUnwind> = Item::new("pending_unwind");
pub const PENDING_BATCH: Item<PendingBatch> = Item::new("pending_batch");
