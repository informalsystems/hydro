#!/usr/bin/env bash
# Shared helpers for the bin/ deployment scripts.
#
# Every script takes `--env <target>` and reads exactly one config file,
# .env.<target> (or .env when --env is omitted). Nothing else is sourced, so a
# staging run can never pick up a production value, and the scripts themselves
# carry no per-environment code.
#
# Written for Bash 3.2, which is what macOS ships.

EVM_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

ENV_NAME=""
DRY_RUN=0
ASSUME_YES=0
ARGS=()

die() {
  echo "Error: $*" >&2
  exit 1
}

# Consumes --env/--dry-run/--help; anything else lands in ARGS.
parse_args() {
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --env)
        [[ $# -ge 2 ]] || die "--env needs a target name"
        ENV_NAME="$2"
        shift 2
        ;;
      --env=*)
        ENV_NAME="${1#--env=}"
        shift
        ;;
      --dry-run)
        DRY_RUN=1
        shift
        ;;
      --yes)
        ASSUME_YES=1
        shift
        ;;
      -h|--help)
        usage
        exit 0
        ;;
      --)
        shift
        while [[ $# -gt 0 ]]; do ARGS+=("$1"); shift; done
        ;;
      -*)
        die "unknown option: $1"
        ;;
      *)
        ARGS+=("$1")
        shift
        ;;
    esac
  done
}

load_env() {
  local file="$EVM_ROOT/.env${ENV_NAME:+.$ENV_NAME}"
  [[ -f "$file" ]] || die "$file not found. Copy .env.example and fill it in."
  set -a
  # shellcheck disable=SC1090
  source "$file"
  set +a
  ENV_FILE="$file"
  echo "Config:   $(basename "$file")"
}

require_vars() {
  local v
  for v in "$@"; do
    [[ -n "${!v:-}" ]] || die "$v is not set in $(basename "${ENV_FILE:-the environment}")"
  done
}

# Sets SIGNER_ARGS for forge/cast and DEPLOYER to the signing address.
resolve_signer() {
  if [[ -n "${PRIVATE_KEY:-}" ]]; then
    SIGNER_ARGS=(--private-key "$PRIVATE_KEY")
  elif [[ -n "${MNEMONIC:-}" ]]; then
    PRIVATE_KEY=$(cast wallet private-key --mnemonic "$MNEMONIC")
    SIGNER_ARGS=(--private-key "$PRIVATE_KEY")
  elif [[ -n "${ACCOUNT_NAME:-}" ]]; then
    SIGNER_ARGS=(--account "$ACCOUNT_NAME")
  else
    die "set PRIVATE_KEY, MNEMONIC or ACCOUNT_NAME in $(basename "$ENV_FILE")"
  fi
  DEPLOYER=$(cast wallet address "${SIGNER_ARGS[@]}")
  export DEPLOYER
}

# Same, but materializes PRIVATE_KEY so a script sending many transactions does
# not prompt for the keystore password on each one.
resolve_private_key() {
  resolve_signer
  if [[ -z "${PRIVATE_KEY:-}" ]]; then
    PRIVATE_KEY=$(cast wallet private-key --account "$ACCOUNT_NAME")
    SIGNER_ARGS=(--private-key "$PRIVATE_KEY")
  fi
}

check_chain() {
  require_vars RPC_URL EXPECTED_CHAIN_ID
  local actual
  actual=$(cast chain-id --rpc-url "$RPC_URL")
  [[ "$actual" == "$EXPECTED_CHAIN_ID" ]] ||
    die "RPC is chain $actual, but EXPECTED_CHAIN_ID is $EXPECTED_CHAIN_ID"
  CHAIN_ID="$actual"
}

require_contract() {
  local addr="$1" label="$2" code
  code=$(cast code "$addr" --rpc-url "$RPC_URL")
  [[ "${#code}" -gt 2 ]] || die "$label $addr has no code on chain ${CHAIN_ID:-?}"
}

require_whitelisted() {
  local vault="$1" addr="$2" label="$3"
  [[ "$(cast call "$vault" 'whitelist(address)(bool)' "$addr" --rpc-url "$RPC_URL")" == "true" ]] ||
    die "$label $addr is not vault-whitelisted"
}

confirm() {
  local phrase="$1" answer
  [[ "$ASSUME_YES" == "1" ]] && return 0
  read -r -p "  Type '$phrase' to continue: " answer
  [[ "$answer" == "$phrase" ]] || die "aborted"
}

# Generated Safe batches land here. out/ is gitignored: these are per-deployment
# artifacts, reproducible from the script plus the config file.
safe_tx_dir() {
  mkdir -p "$EVM_ROOT/out/safe-tx"
  echo "$EVM_ROOT/out/safe-tx"
}
