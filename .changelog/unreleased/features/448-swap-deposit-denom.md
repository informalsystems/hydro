- Add the `SwapDepositDenom` execute message to the Cosmos Hub Inflow vault, allowing operators
  to swap the deposit denom (e.g. `USDC.noble` to `USDC.injective`). The swap is whitelist-gated
  and reverts unless the vault and all registered adapters hold no funds in the current deposit
  denom.
  ([\#448](https://github.com/informalsystems/hydro/pull/448))
