//! Maturity without a settle message (SPEC §7): a cursor over the batches ordered by completion
//! time reserves each batch's bookings the moment its ATOM can be in the balance.

use cosmwasm_std::{Env, Order, QuerierWrapper, StdResult, Storage, Timestamp, Uint128};

use cw_storage_plus::Bound;

use crate::pricing::balance;
use crate::state::{Batch, Config, Matured, BATCHES, BY_COMPLETION, MATURED};

/// The chain pays a matured entry in the EndBlocker of the first block whose time reaches it,
/// so the next blocks see the ATOM.
pub const MATURITY_MARGIN_S: u64 = 60;

pub fn completion_key(b: &Batch) -> (u64, u64) {
    (b.completion.nanos(), b.id)
}

/// Whether a batch's ATOM is past maturity (`completion + maturity_margin ≤ now`).
pub fn is_past_maturity(completion: Timestamp, now: Timestamp) -> bool {
    completion.plus_seconds(MATURITY_MARGIN_S) <= now
}

/// The cursor and `reserved` as of `now`, without saving.
pub fn matured_view(storage: &dyn Storage, now: Timestamp) -> StdResult<Matured> {
    let mut m = MATURED.may_load(storage)?.unwrap_or_default();
    let start = m.cursor.map(Bound::exclusive);
    for item in BY_COMPLETION.keys(storage, start, None, Order::Ascending) {
        let (completion_nanos, id) = item?;
        if !is_past_maturity(Timestamp::from_nanos(completion_nanos), now) {
            break;
        }
        let b = BATCHES.load(storage, id)?;
        m.reserved += b.booked.saturating_sub(b.claimed);
        m.cursor = Some((completion_nanos, id));
    }
    Ok(m)
}

/// Advances the cursor and saves it.
pub fn advance(storage: &mut dyn Storage, now: Timestamp) -> StdResult<Matured> {
    let m = matured_view(storage, now)?;
    MATURED.save(storage, &m)?;
    Ok(m)
}

/// Whether the batch is at or before the cursor.
pub fn is_matured(m: &Matured, b: &Batch) -> bool {
    matches!(m.cursor, Some(c) if completion_key(b) <= c)
}

/// `free = bank balance (ATOM) - reserved`, less `exclude` (ATOM that arrived with the message).
pub fn free_buffer(
    q: &QuerierWrapper,
    env: &Env,
    cfg: &Config,
    m: &Matured,
    exclude: Uint128,
) -> StdResult<Uint128> {
    let bal = balance(q, &env.contract.address, &cfg.bond_denom)?;
    Ok(bal.saturating_sub(exclude).saturating_sub(m.reserved))
}
