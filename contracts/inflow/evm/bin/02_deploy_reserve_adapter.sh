#!/usr/bin/env bash
set -euo pipefail

# Deploys ReserveAdapter (implementation + ERC1967 proxy) to the target named by --env.
# Wraps script/DeployReserveAdapter.s.sol.
#
# WIRE_MODE in the config file decides whether the vault handshake happens here or
# from the admin Safe afterwards (bin/03). Both the shell and the forge script run
# the same preflight, so a misconfigured target fails before any gas is spent.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/common.sh
source "$SCRIPT_DIR/lib/common.sh"

usage() {
  cat <<USAGE
Usage: $(basename "$0") [--env <target>] [--dry-run] [--yes]

  --env <target>  read .env.<target> instead of .env
  --dry-run       simulate without broadcasting
  --yes           skip the confirmation prompt
USAGE
}

parse_args "$@"
cd "$EVM_ROOT"
load_env
check_chain
resolve_signer

require_vars VAULT_ADDRESS
export VAULT_ADDRESS EXPECTED_CHAIN_ID
export WIRE_MODE="${WIRE_MODE:-safe}"
export ADAPTER_NAME="${ADAPTER_NAME:-reserve}"

case "$WIRE_MODE" in
  safe)
    export ADAPTER_ADMIN="${ADAPTER_ADMIN:-${ADMIN_SAFE:-}}"
    [[ -n "$ADAPTER_ADMIN" ]] || die "set ADMIN_SAFE (or ADAPTER_ADMIN) in $(basename "$ENV_FILE")"
    WIRING_ACTOR="$ADAPTER_ADMIN"
    WIRING_LABEL="admin Safe, via bin/03"
    ;;
  deployer)
    export ADAPTER_ADMIN="${ADAPTER_ADMIN:-$DEPLOYER}"
    WIRING_ACTOR="$DEPLOYER"
    WIRING_LABEL="deployer, in this broadcast"
    ;;
  *)
    die "WIRE_MODE must be 'safe' or 'deployer', got '$WIRE_MODE'"
    ;;
esac

require_contract "$VAULT_ADDRESS" "vault"
require_whitelisted "$VAULT_ADDRESS" "$WIRING_ACTOR" "wiring address"

if cast call "$VAULT_ADDRESS" 'getAdapterByName(string)((address,bool,bool,string))' "$ADAPTER_NAME" \
     --rpc-url "$RPC_URL" >/dev/null 2>&1; then
  die "vault already has an adapter named '$ADAPTER_NAME'; unregister it first"
fi

# Not necessarily ETH: on Arc the gas token is USDC at 18 decimals.
GAS_BALANCE=$(cast from-wei "$(cast balance "$DEPLOYER" --rpc-url "$RPC_URL")")

cat <<SUMMARY

  Deploy ReserveAdapter to chain $CHAIN_ID

    Deployer      : $DEPLOYER
    Gas balance   : $GAS_BALANCE (native gas token)
    Vault         : $VAULT_ADDRESS
    Adapter name  : $ADAPTER_NAME (registered automated=false, tracked=true)
    Adapter admin : $ADAPTER_ADMIN
    Wired by      : $WIRING_LABEL

SUMMARY

if [[ "$DRY_RUN" == "1" ]]; then
  echo "  DRY RUN, nothing will be broadcast."
  forge script script/DeployReserveAdapter.s.sol --rpc-url "$RPC_URL" "${SIGNER_ARGS[@]}" -vvv
  exit 0
fi

confirm deploy

forge script script/DeployReserveAdapter.s.sol \
  --rpc-url "$RPC_URL" \
  --broadcast \
  "${SIGNER_ARGS[@]}" \
  -vvv
