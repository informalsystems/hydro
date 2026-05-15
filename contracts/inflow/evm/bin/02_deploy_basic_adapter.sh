#!/usr/bin/env bash
set -euo pipefail

# Arc testnet deployment script for BasicInflowAdapter (UUPS upgradeable proxy).
# Wraps script/DeployBasicAdapter.s.sol — see that file for full details.
# Run from contracts/inflow/evm/
# Required env: VAULT_ADDRESS (or first arg), PRIVATE_KEY or MNEMONIC

# ── Config ────────────────────────────────────────────────────────────────────

RPC_URL="https://rpc.testnet.arc.network"

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

# ── Vault address ─────────────────────────────────────────────────────────────

VAULT_ADDRESS="${1:-${VAULT_ADDRESS:-}}"
if [[ -z "$VAULT_ADDRESS" ]]; then
  echo "Error: provide VAULT_ADDRESS as first arg or env var"
  echo "Usage: VAULT_ADDRESS=0x... bash bin/02_deploy_basic_adapter.sh"
  exit 1
fi
echo "Vault:    $VAULT_ADDRESS"

# ── Export env vars expected by DeployBasicAdapter.s.sol ─────────────────────

export VAULT_ADDRESS
export ADAPTER_ADMIN="${ADAPTER_ADMIN:-$MY_ADDRESS}"
export RPC_URL

# ── Deploy ────────────────────────────────────────────────────────────────────

echo ""
forge script script/DeployBasicAdapter.s.sol \
  --rpc-url "$RPC_URL" \
  --broadcast \
  --private-key "$PRIVATE_KEY" \
  -vvvv
