This document describes the migration process for the existing Inflow smart contracts.

## v3.6.4 -> v3.6.5

1. Instantiate a new Control Center contract and provide the address of the existing Inflow contract to it during instantiation. Each of the existing Inflow contracts (i.e. for ATOM, BTC and USDC) will be tied to a separate instance of the Control Center contract.
2. Execute `submit_deployed_amount()` against newly instantiated Control Center contract to copy the value currently held by the Inflow contract instance.
3. Migrate existing Inflow contract instances by providing the address of the newly instantiated Control Center contract through the `MigrateMsg`.