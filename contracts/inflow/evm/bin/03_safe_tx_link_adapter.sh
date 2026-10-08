#!/usr/bin/env bash
set -euo pipefail

# Generates a Safe Transaction Builder batch that completes the vault <-> adapter
# handshake, which bin/02 deliberately leaves undone when WIRE_MODE=safe:
#   1. vault.registerAdapter(ADAPTER_NAME, ADAPTER, automated=false, tracked=true)
#   2. adapter.registerDepositor(VAULT, 0x)
#   3. adapter.setDepositorEnabled(VAULT, true)
#
# Step 3 is redundant, since registerDepositor already enables the depositor, but it
# is kept so the batch states the intended end state explicitly.
#
# tracked=true means the reserve balance counts toward deployedAmount. Read the
# ReserveAdapter NatSpec: every reserve transfer needs a compensating
# submitDeployedAmount call.
#
# Import the generated file in the Safe UI under Apps -> Transaction Builder.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/common.sh
source "$SCRIPT_DIR/lib/common.sh"

usage() {
  cat <<USAGE
Usage: $(basename "$0") [--env <target>] VAULT_ADDRESS ADAPTER_ADDRESS

  --env <target>  read .env.<target> instead of .env

VAULT_ADDRESS defaults to VAULT_ADDRESS from the config file.
USAGE
}

parse_args "$@"
cd "$EVM_ROOT"
load_env
check_chain

VAULT="${ARGS[0]:-${VAULT_ADDRESS:-}}"
ADAPTER="${ARGS[1]:-}"
[[ -n "$VAULT" && -n "$ADAPTER" ]] || { usage; exit 1; }

require_vars ADMIN_SAFE
ADAPTER_NAME="${ADAPTER_NAME:-reserve}"

require_contract "$VAULT" "vault"
require_contract "$ADAPTER" "adapter"
require_contract "$ADMIN_SAFE" "safe"

# The Safe signs tx 1 as a vault-whitelisted caller and tx 2 and 3 as the adapter admin.
require_whitelisted "$VAULT" "$ADMIN_SAFE" "Safe"

if cast call "$VAULT" 'getAdapterByName(string)((address,bool,bool,string))' "$ADAPTER_NAME" \
     --rpc-url "$RPC_URL" >/dev/null 2>&1; then
  die "vault already has an adapter named '$ADAPTER_NAME'; unregister it before re-linking"
fi

DATA_REGISTER_ADAPTER=$(cast calldata "registerAdapter(string,address,bool,bool)" "$ADAPTER_NAME" "$ADAPTER" false true)
DATA_REGISTER_DEPOSITOR=$(cast calldata "registerDepositor(address,bytes)" "$VAULT" "0x")
DATA_ENABLE_DEPOSITOR=$(cast calldata "setDepositorEnabled(address,bool)" "$VAULT" true)

OUT="$(safe_tx_dir)/${ENV_NAME:-default}-link-adapter.json"

cat > "$OUT" <<JSON
{
  "version": "1.0",
  "chainId": "$CHAIN_ID",
  "createdAt": $(date +%s)000,
  "meta": {
    "name": "Link ReserveAdapter to InflowVault",
    "description": "Both directions of the vault/adapter handshake: (1) tell the vault about the adapter, (2 and 3) tell the adapter to accept the vault as a depositor.",
    "txBuilderVersion": "1.16.4",
    "createdFromSafeAddress": "$ADMIN_SAFE",
    "createdFromOwnerAddress": ""
  },
  "transactions": [
    { "to": "$VAULT",   "value": "0", "data": "$DATA_REGISTER_ADAPTER",   "contractMethod": null, "contractInputsValues": null },
    { "to": "$ADAPTER", "value": "0", "data": "$DATA_REGISTER_DEPOSITOR", "contractMethod": null, "contractInputsValues": null },
    { "to": "$ADAPTER", "value": "0", "data": "$DATA_ENABLE_DEPOSITOR",   "contractMethod": null, "contractInputsValues": null }
  ]
}
JSON

cat <<SUMMARY

  Wrote $OUT

    Safe    : $ADMIN_SAFE (chain $CHAIN_ID)
    Vault   : $VAULT
    Adapter : $ADAPTER (as "$ADAPTER_NAME")

  Import it in the Safe UI (Apps -> Transaction Builder) and execute, then verify:

    cast call $VAULT 'getAdapterByName(string)((address,bool,bool,string))' $ADAPTER_NAME --rpc-url \$RPC_URL
    cast call $ADAPTER 'depositors()(address[])' --rpc-url \$RPC_URL

SUMMARY
