This document describes the migration process for the Cosmos Hub Inflow Vault.

## Neutron → Cosmos Hub vault migration

This migration retires the Neutron Inflow Vault and moves all liquidity to the Cosmos Hub native vault.
Users hold bridged Neutron vault shares (`ibc/...` on Cosmos Hub) and must convert them to native
Cosmos Hub vault shares via the `shares-converter` contract.

### Prerequisites

- Cosmos Hub vault (`vault-cosmoshub`) and Control Center already deployed and wired together.
- `shares-converter` contract deployed and configured with the Neutron IBC denom → Cosmos Hub factory denom pair.
- Total outstanding Neutron vault shares and total deployed amount known (query before stopping the vault).

### Step 1 — Stop the Neutron vault

Pause the Neutron vault to prevent new deposits/withdrawals. If there is no pause functionality,
remove the Neutron vault from the Neutron Control Center's subvault registry.

### Step 2 — Retrieve all deployed funds

Withdraw all funds from Neutron adapters back to the Neutron vault, then IBC-transfer the full
ATOM balance from the Neutron vault to the Cosmos Hub vault address.

### Step 3 — Mint migration shares (`MintForMigration`)

Call `MintForMigration` on the **Cosmos Hub vault** from a whitelisted address.
This is a one-shot function — it reverts if called a second time.

```json
{
  "mint_for_migration": {
    "shares_to_mint": "<total_neutron_shares_outstanding>",
    "deployed_amount": "<deployed_uatom_if_any>",
    "conversion_contract": "<shares_converter_address>"
  }
}
```

- `shares_to_mint`: total shares outstanding on the Neutron vault at time of shutdown.
- `deployed_amount`: if any ATOM is still in transit or deployed, pass it here so the Control Center
  accounts for it; otherwise omit or pass `null`.
- `conversion_contract`: address of the deployed `shares-converter` contract.

This mints the shares and sends them directly to the conversion contract. If `deployed_amount` is
provided, it also calls `UpdateDeployedAmount` on the Control Center.

### Step 4 — Migrate vault-cosmoshub to v2

Upload a new version of the vault-cosmoshub code **without** `MintForMigration` in the execute
match and **without** the `MintForMigration` variant in the shared interface. Then migrate the
contract to the new code ID:

```bash
gaiad tx wasm migrate <vault_address> <new_code_id> '{}' \
  --from <admin> --chain-id cosmoshub-devnet-1 ...
```

After migration, `MintForMigration` is no longer callable from any code path.

### Step 5 — Users convert their shares

Users send their bridged Neutron shares (`ibc/C744...`) to the `shares-converter` contract and
receive an equal amount of native Cosmos Hub vault shares:

```bash
gaiad tx wasm execute <shares_converter_address> '{"convert":{}}' \
  --amount <amount>ibc/C744... \
  --from <user> --chain-id cosmoshub-devnet-1 ...
```

---

## Cleanup after migration (v2 code)

When preparing the v2 vault-cosmoshub code, remove the following:

1. **`packages/interface/src/inflow_vault.rs`** — remove the `MintForMigration` variant from `ExecuteMsg`.
2. **`contracts/inflow/vault/src/contract.rs`** — remove the `ExecuteMsg::MintForMigration { .. } => Err(ContractError::Unauthorized)` arm.
3. **`contracts/inflow/vault-cosmoshub/src/contract.rs`** — remove the `MintForMigration` arm and the `mint_for_migration` function.
4. **`contracts/inflow/vault-cosmoshub/src/error.rs`** — remove the `MigrationAlreadyExecuted` variant.
5. **`contracts/inflow/vault-cosmoshub/src/state.rs`** — `MIGRATION_MINTED` can be kept (harmless orphan) or removed. If removed, also drop it from the `use` import in `contract.rs`.
