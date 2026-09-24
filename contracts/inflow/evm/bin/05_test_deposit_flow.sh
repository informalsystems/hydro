#!/usr/bin/env bash
set -euo pipefail

# Exercises deposit and redeem against a live InflowVault.
#
# ERC-4626 deposits are permissionless and bounded only by depositCap; only the
# operator surface is whitelist-gated. deposit() pulls via transferFrom, so the vault
# is approved first, and shares mint to the depositor.
#
# With SKIP_REDEEM=true this doubles as a way to seed a vault before rehearsing the
# withdraw and deposit-for-deployment flow.
#
# Uses cast rather than forge script because Arc's USDC precompiles only behave
# correctly on Arc's own EVM, and they need more gas than default estimation gives.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/common.sh
source "$SCRIPT_DIR/lib/common.sh"

usage() {
  cat <<USAGE
Usage: $(basename "$0") [--env <target>] [--yes] [AMOUNT]

  --env <target>  read .env.<target> instead of .env
  --yes           skip the confirmation prompt

AMOUNT is in the asset's base units and defaults to DEPOSIT_AMOUNT from the config file.
Set SKIP_REDEEM=true to keep the shares, which seeds the vault instead of round-tripping.
Set ADAPTER_ADDRESS to also report the adapter's balance and position.
Set EXPECTED_DEPLOYER to refuse to run from any other account.
USAGE
}

parse_args "$@"
cd "$EVM_ROOT"
load_env
check_chain
resolve_private_key

require_vars ASSET VAULT_ADDRESS
AMOUNT="${ARGS[0]:-${DEPOSIT_AMOUNT:-}}"
[[ -n "$AMOUNT" ]] || die "provide an amount as an argument or set DEPOSIT_AMOUNT"
GAS_LIMIT="${GAS_LIMIT:-5000000}"
SKIP_REDEEM="${SKIP_REDEEM:-false}"

if [[ -n "${EXPECTED_DEPLOYER:-}" ]]; then
  # Bash 3.2 has no ${var,,}, so case-fold through tr.
  lower() { printf '%s' "$1" | tr '[:upper:]' '[:lower:]'; }
  [[ "$(lower "$DEPLOYER")" == "$(lower "$EXPECTED_DEPLOYER")" ]] ||
    die "signing account $DEPLOYER is not EXPECTED_DEPLOYER $EXPECTED_DEPLOYER"
fi

vcall() { cast call "$@" --rpc-url "$RPC_URL"; }

# cast call prints "value [human-readable]", so keep only the first field.
read_num() { vcall "$@" | awk '{print $1}'; }

vsend() {
  local to="$1" sig="$2"; shift 2
  echo "  tx: $(cast send "$to" "$sig" "$@" "${SIGNER_ARGS[@]}" --rpc-url "$RPC_URL" \
    --gas-limit "$GAS_LIMIT" --json | jq -r '.transactionHash')"
}

snapshot() {
  echo "=== $1 ==="
  echo "  caller asset balance   : $(vcall "$ASSET" "balanceOf(address)(uint256)" "$DEPLOYER")"
  echo "  vault asset balance    : $(vcall "$ASSET" "balanceOf(address)(uint256)" "$VAULT_ADDRESS")"
  echo "  caller vault shares    : $(vcall "$VAULT_ADDRESS" "balanceOf(address)(uint256)" "$DEPLOYER")"
  echo "  vault totalAssets      : $(vcall "$VAULT_ADDRESS" "totalAssets()(uint256)")"
  echo "  availableForDeployment : $(vcall "$VAULT_ADDRESS" "availableForDeployment()(uint256)")"
  if [[ -n "${ADAPTER_ADDRESS:-}" ]]; then
    echo "  adapter asset balance  : $(vcall "$ASSET" "balanceOf(address)(uint256)" "$ADAPTER_ADDRESS")"
    echo "  adapter position(vault): $(vcall "$ADAPTER_ADDRESS" "depositorPosition(address,address)(uint256)" "$VAULT_ADDRESS" "$ASSET")"
  fi
}

BALANCE=$(read_num "$ASSET" 'balanceOf(address)(uint256)' "$DEPLOYER")
HEADROOM=$(read_num "$VAULT_ADDRESS" 'maxDeposit(address)(uint256)' "$DEPLOYER")

cat <<SUMMARY

  Deposit flow on chain $CHAIN_ID

    Caller               : $DEPLOYER
    Vault                : $VAULT_ADDRESS
    Asset                : $ASSET
    Adapter              : ${ADAPTER_ADDRESS:-not set, adapter reporting skipped}
    Amount               : $AMOUNT
    Caller balance       : $BALANCE
    Deposit cap headroom : $HEADROOM
    Redeem afterwards    : $([[ "$SKIP_REDEEM" == "true" ]] && echo "no, shares are kept" || echo yes)

SUMMARY

(( BALANCE >= AMOUNT )) || die "caller balance is below the deposit amount"
(( HEADROOM >= AMOUNT )) || die "deposit cap headroom is below the amount; raise it from the admin Safe"

confirm run

snapshot "BEFORE DEPOSIT"

ALLOWANCE=$(read_num "$ASSET" 'allowance(address,address)(uint256)' "$DEPLOYER" "$VAULT_ADDRESS")
if (( ALLOWANCE < AMOUNT )); then
  echo ""
  echo "=== approve vault for $AMOUNT ==="
  vsend "$ASSET" "approve(address,uint256)" "$VAULT_ADDRESS" "$AMOUNT"
fi

echo ""
echo "=== deposit $AMOUNT ==="
vsend "$VAULT_ADDRESS" "deposit(uint256,address)" "$AMOUNT" "$DEPLOYER"

echo ""
snapshot "AFTER DEPOSIT"

if [[ "$SKIP_REDEEM" == "true" ]]; then
  echo ""
  echo "Shares kept (SKIP_REDEEM=true)."
  exit 0
fi

SHARES=$(read_num "$VAULT_ADDRESS" "balanceOf(address)(uint256)" "$DEPLOYER")
if [[ "$SHARES" == "0" ]]; then
  echo ""
  echo "No shares to redeem."
  exit 0
fi

echo ""
echo "=== redeem $SHARES shares ==="
vsend "$VAULT_ADDRESS" "redeem(uint256,address,address)" "$SHARES" "$DEPLOYER" "$DEPLOYER"

echo ""
snapshot "AFTER REDEEM"
