#!/usr/bin/env bash
set -euo pipefail

# Deploys InflowVault (UUPS upgradeable proxy) to Arc mainnet (chain 5042).
# RPC URL is read from ARC_MAINNET_RPC_URL in .env — never hardcode it.
# Run from contracts/inflow/evm/
# Reads config from .env — see .env.example for all variables.
#
# Signing — set exactly one of these in .env (in order of preference):
#   ACCOUNT_NAME  cast keystore account name (safest — no secrets in shell history)
#                 One-time setup: cast wallet import <name> --interactive
#   PRIVATE_KEY   raw hex key (appears in process table — avoid on shared machines)
#   MNEMONIC      24-word phrase (derived to PRIVATE_KEY at runtime — same caveat)

# ── Load config ───────────────────────────────────────────────────────────────

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ENV_FILE="$SCRIPT_DIR/../.env"
if [[ -f "$ENV_FILE" ]]; then
  # shellcheck disable=SC1090
  source "$ENV_FILE"
fi

RPC_URL="${ARC_MAINNET_RPC_URL:?Error: ARC_MAINNET_RPC_URL not set in .env}"

# ── Resolve signing method ────────────────────────────────────────────────────

SIGN_FLAGS=()
if [[ -n "${ACCOUNT_NAME:-}" ]]; then
  # Keystore: password prompted interactively, nothing sensitive in history
  SIGN_FLAGS=(--account "$ACCOUNT_NAME")
  MY_ADDRESS=$(cast wallet address --account "$ACCOUNT_NAME" 2>/dev/null \
    || cast wallet address --account "$ACCOUNT_NAME" --show-address)
elif [[ -n "${PRIVATE_KEY:-}" ]]; then
  SIGN_FLAGS=(--private-key "$PRIVATE_KEY")
  MY_ADDRESS=$(cast wallet address --private-key "$PRIVATE_KEY")
elif [[ -n "${MNEMONIC:-}" ]]; then
  PRIVATE_KEY=$(cast wallet private-key --mnemonic "$MNEMONIC")
  SIGN_FLAGS=(--private-key "$PRIVATE_KEY")
  MY_ADDRESS=$(cast wallet address --private-key "$PRIVATE_KEY")
else
  echo "Error: set ACCOUNT_NAME (recommended), PRIVATE_KEY, or MNEMONIC in .env"
  echo "  Safest: cast wallet import arc-mainnet-deployer --interactive"
  echo "          then set ACCOUNT_NAME=arc-mainnet-deployer in .env"
  exit 1
fi

echo "Deployer: $MY_ADDRESS"

# ── Arc mainnet overrides ─────────────────────────────────────────────────────

export ASSET="0x3600000000000000000000000000000000000000"   # USDC on Arc mainnet
export VAULT_NAME="${VAULT_NAME:-Inflow USDC Arc R1}"
export VAULT_SYMBOL="${VAULT_SYMBOL:-iflUSDC-ARC-R1}"
export DEPOSIT_CAP="${DEPOSIT_CAP:-1000000000000}"          # 1 000 000 USDC (6 decimals)
export MAX_WITHDRAWALS_PER_USER="${MAX_WITHDRAWALS_PER_USER:-10}"
export INITIAL_ADMIN="${INITIAL_ADMIN:-$MY_ADDRESS}"
export INITIAL_DEPLOYED_AMOUNT_ADMIN="${INITIAL_DEPLOYED_AMOUNT_ADMIN:-$INITIAL_ADMIN}"
export FEE_RATE="${FEE_RATE:-0}"
export FEE_RECIPIENT="${FEE_RECIPIENT:-}"
export RPC_URL

echo ""
echo "Deploying InflowVault to Arc mainnet (chain 5042)"
echo "  Asset (USDC):          $ASSET"
echo "  Vault name:            $VAULT_NAME"
echo "  Vault symbol:          $VAULT_SYMBOL"
echo "  Deposit cap:           $DEPOSIT_CAP"
echo "  Admin:                 $INITIAL_ADMIN"
echo "  Deployed amount admin: $INITIAL_DEPLOYED_AMOUNT_ADMIN"
echo "  Fee rate:              $FEE_RATE"
echo ""

# ── Deploy ────────────────────────────────────────────────────────────────────

forge script script/DeployInflowVault.s.sol \
  --rpc-url "$RPC_URL" \
  --broadcast \
  "${SIGN_FLAGS[@]}" \
  -vvvv
