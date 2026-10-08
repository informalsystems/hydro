- Record the high-water mark of the Inflow control center as the share price after the fee shares are
  minted, so a vault no longer has to re-earn the dilution of its own fee mint before fees accrue again.
  `accrue_fees` and `submit_deployed_amount` report it in a new `high_water_mark_price` attribute;
  `current_share_price` and `fee_share_price` still carry the price before the mint.
  ([\#454](https://github.com/informalsystems/hydro/pull/454))
