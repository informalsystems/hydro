- Record the high-water mark of the EVM `InflowVault` as the share price after the fee shares are
  minted, so a vault no longer has to re-earn the dilution of its own fee mint before fees accrue again.
  Add the one-shot, whitelist-gated `resetHighWaterMark()` reinitializer, which lowers the mark of an
  already deployed vault to its current share price during the upgrade and does nothing when the mark
  is not above the share price.
  `FeesAccrued.sharePrice` still carries the price before the mint, so `highWaterMarkPrice()` is now
  lower than it after an accrual: read the mark from the contract instead of deriving it from the event.
  ([\#453](https://github.com/informalsystems/hydro/pull/453))
