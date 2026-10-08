#!/usr/bin/env bash
set -euo pipefail

# Deploys InflowVault (implementation + ERC1967 proxy) to the target named by --env.
# Wraps script/DeployInflowVault.s.sol.

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

require_vars ASSET VAULT_NAME VAULT_SYMBOL DEPOSIT_CAP MAX_WITHDRAWALS_PER_USER INITIAL_ADMIN

export ASSET VAULT_NAME VAULT_SYMBOL DEPOSIT_CAP MAX_WITHDRAWALS_PER_USER INITIAL_ADMIN
export INITIAL_DEPLOYED_AMOUNT_ADMIN="${INITIAL_DEPLOYED_AMOUNT_ADMIN:-$INITIAL_ADMIN}"
export FEE_RATE="${FEE_RATE:-0}"
export FEE_RECIPIENT="${FEE_RECIPIENT:-}"

[[ "$FEE_RATE" == "0" || -n "$FEE_RECIPIENT" ]] || die "FEE_RECIPIENT is required when FEE_RATE > 0"

require_contract "$ASSET" "ASSET"

VERIFY_ARGS=()
if [[ -n "${ETHERSCAN_API_KEY:-}" ]]; then
  VERIFY_ARGS=(--verify --etherscan-api-key "$ETHERSCAN_API_KEY")
fi

cat <<SUMMARY

  Deploy InflowVault to chain $CHAIN_ID

    Deployer              : $DEPLOYER
    Asset                 : $ASSET
    Share token           : $VAULT_NAME ($VAULT_SYMBOL)
    Deposit cap           : $DEPOSIT_CAP
    Max withdrawals/user  : $MAX_WITHDRAWALS_PER_USER
    Whitelist admin       : $INITIAL_ADMIN
    Deployed-amount admin : $INITIAL_DEPLOYED_AMOUNT_ADMIN
    Fee rate / recipient  : $FEE_RATE / ${FEE_RECIPIENT:-none}
    Verification          : $([[ ${#VERIFY_ARGS[@]} -gt 0 ]] && echo enabled || echo "skipped (set ETHERSCAN_API_KEY)")

SUMMARY

if [[ "$DRY_RUN" == "1" ]]; then
  echo "  DRY RUN, nothing will be broadcast."
  forge script script/DeployInflowVault.s.sol --rpc-url "$RPC_URL" "${SIGNER_ARGS[@]}" -vvv
  exit 0
fi

confirm deploy

forge script script/DeployInflowVault.s.sol \
  --rpc-url "$RPC_URL" \
  --broadcast \
  "${SIGNER_ARGS[@]}" \
  ${VERIFY_ARGS[@]+"${VERIFY_ARGS[@]}"} \
  -vvv
