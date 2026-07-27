#!/usr/bin/env bash
set -euo pipefail

# Deploys InflowVault (UUPS upgradeable proxy) to Base mainnet.
# Wraps script/DeployInflowVault.s.sol — see that file for full details.
# Run from contracts/inflow/evm/
# Reads MNEMONIC/PRIVATE_KEY and admin addresses from .env, overrides Arc-specific values.

# ── Config ────────────────────────────────────────────────────────────────────

RPC_URL="${BASE_RPC_URL:-https://base-rpc.publicnode.com}"
# Basescan API key — get one free at https://basescan.org/myapikey
# Set in .env or export before running. If absent, --verify is skipped.
ETHERSCAN_API_KEY="${ETHERSCAN_API_KEY:-}"

# ── Load secrets ──────────────────────────────────────────────────────────────

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ENV_FILE="$SCRIPT_DIR/../.env"
if [[ -f "$ENV_FILE" ]]; then
  # shellcheck disable=SC1090
  source "$ENV_FILE"
fi

if [[ -z "${PRIVATE_KEY:-}" ]]; then
  if [[ -n "${MNEMONIC:-}" ]]; then
    PRIVATE_KEY=$(cast wallet private-key --mnemonic "$MNEMONIC")
  else
    echo "Error: set PRIVATE_KEY or MNEMONIC in .env or env"
    exit 1
  fi
fi

MY_ADDRESS=$(cast wallet address --private-key "$PRIVATE_KEY")
echo "Deployer: $MY_ADDRESS"

# ── Base-specific overrides ───────────────────────────────────────────────────
# These replace the Arc testnet defaults from .env

export ASSET="0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"        # USDC on Base mainnet
export VAULT_NAME="${VAULT_NAME:-Inflow USDC EVM Risk 2}"
export VAULT_SYMBOL="${VAULT_SYMBOL:-iflUSDC-EVM-R2}"
export DEPOSIT_CAP="${DEPOSIT_CAP:-1000000000000}"                # 1 000 000 USDC (6 decimals)
export MAX_WITHDRAWALS_PER_USER="${MAX_WITHDRAWALS_PER_USER:-10}"
export INITIAL_ADMIN="${INITIAL_ADMIN:-$MY_ADDRESS}"
# INITIAL_DEPLOYED_AMOUNT_ADMIN: set this to your test Safe address on Base.
export INITIAL_DEPLOYED_AMOUNT_ADMIN="${INITIAL_DEPLOYED_AMOUNT_ADMIN:-$INITIAL_ADMIN}"
export FEE_RATE="${FEE_RATE:-0}"
export FEE_RECIPIENT="${FEE_RECIPIENT:-}"
export RPC_URL

echo ""
echo "Deploying InflowVault to Base mainnet"
echo "  Asset (USDC):          $ASSET"
echo "  Vault name:            $VAULT_NAME"
echo "  Vault symbol:          $VAULT_SYMBOL"
echo "  Deposit cap:           $DEPOSIT_CAP"
echo "  Admin:                 $INITIAL_ADMIN"
echo "  Deployed amount admin: $INITIAL_DEPLOYED_AMOUNT_ADMIN"
echo "  Fee rate:              $FEE_RATE"
echo ""

# ── Deploy ────────────────────────────────────────────────────────────────────

VERIFY_FLAGS=()
if [[ -n "${ETHERSCAN_API_KEY}" ]]; then
  echo "  Etherscan verification: enabled"
  VERIFY_FLAGS=(--verify --etherscan-api-key "$ETHERSCAN_API_KEY")
else
  echo "  Etherscan verification: skipped (set ETHERSCAN_API_KEY to enable)"
fi
echo ""

forge script script/DeployInflowVault.s.sol \
  --rpc-url "$RPC_URL" \
  --broadcast \
  --private-key "$PRIVATE_KEY" \
  "${VERIFY_FLAGS[@]}" \
  -vvvv
