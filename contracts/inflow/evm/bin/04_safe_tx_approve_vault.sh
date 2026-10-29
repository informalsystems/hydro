#!/usr/bin/env bash
set -euo pipefail

# Generates a Safe Transaction Builder batch approving the vault to spend the admin
# Safe's asset balance.
#
# depositFromDeployment pulls with safeTransferFrom(asset, msg.sender, vault, amount),
# so without this approval it reverts. The default is unlimited, which grants nothing
# beyond what the vault is already trusted with; pass an amount to bound it anyway.
#
# Import the generated file in the Safe UI under Apps -> Transaction Builder.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/common.sh
source "$SCRIPT_DIR/lib/common.sh"

MAX_UINT256="115792089237316195423570985008687907853269984665640564039457584007913129639935"

usage() {
  cat <<USAGE
Usage: $(basename "$0") [--env <target>] [AMOUNT]

  --env <target>  read .env.<target> instead of .env

AMOUNT is in the asset's base units and defaults to an unlimited approval.
USAGE
}

parse_args "$@"
cd "$EVM_ROOT"
load_env
check_chain

require_vars ADMIN_SAFE ASSET VAULT_ADDRESS
AMOUNT="${ARGS[0]:-$MAX_UINT256}"

require_contract "$ASSET" "asset"
require_contract "$VAULT_ADDRESS" "vault"
require_contract "$ADMIN_SAFE" "safe"

DATA_APPROVE=$(cast calldata "approve(address,uint256)" "$VAULT_ADDRESS" "$AMOUNT")

OUT="$(safe_tx_dir)/${ENV_NAME:-default}-approve-vault.json"

cat > "$OUT" <<JSON
{
  "version": "1.0",
  "chainId": "$CHAIN_ID",
  "createdAt": $(date +%s)000,
  "meta": {
    "name": "Approve InflowVault to spend admin Safe asset balance",
    "description": "Required for depositFromDeployment, which pulls the asset from the Safe.",
    "txBuilderVersion": "1.16.4",
    "createdFromSafeAddress": "$ADMIN_SAFE",
    "createdFromOwnerAddress": ""
  },
  "transactions": [
    { "to": "$ASSET", "value": "0", "data": "$DATA_APPROVE", "contractMethod": null, "contractInputsValues": null }
  ]
}
JSON

cat <<SUMMARY

  Wrote $OUT

    Safe   : $ADMIN_SAFE (chain $CHAIN_ID)
    Asset  : $ASSET
    Vault  : $VAULT_ADDRESS
    Amount : $([[ "$AMOUNT" == "$MAX_UINT256" ]] && echo "unlimited" || echo "$AMOUNT")

  Import it in the Safe UI (Apps -> Transaction Builder) and execute, then verify:

    cast call $ASSET 'allowance(address,address)(uint256)' $ADMIN_SAFE $VAULT_ADDRESS --rpc-url \$RPC_URL

SUMMARY
