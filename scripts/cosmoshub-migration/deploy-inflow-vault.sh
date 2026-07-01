#!/bin/bash

# Deploy a minimal Inflow stack (Control Center + Vault) to any CosmWasm-enabled chain.
# Usage: ./deploy-inflow-vault.sh <config-file.json>
# Example (Neutron devnet):     ./deploy-inflow-vault.sh deploy-config-neutron.json
# Example (Cosmos Hub devnet):  ./deploy-inflow-vault.sh deploy-config-cosmoshub.json

set -e

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
cd "$SCRIPT_DIR"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

CONFIG_FILE="${1:?Usage: $0 <config-file.json>}"

if [ ! -f "$CONFIG_FILE" ]; then
    echo -e "${RED}Error: config file '$CONFIG_FILE' not found${NC}"
    exit 1
fi

CONTROL_CENTER_WASM="../../artifacts/control_center.wasm"

# ============================================================================
# Load configuration
# ============================================================================

BINARY=$(jq -r '.binary' "$CONFIG_FILE")
BINARY_HOME=$(jq -r '.binary_home // empty' "$CONFIG_FILE")
CHAIN_ID=$(jq -r '.chain_id' "$CONFIG_FILE")
NODE=$(jq -r '.rpc_node' "$CONFIG_FILE")
GAS_PRICE=$(jq -r '.gas_price' "$CONFIG_FILE")
GAS_ADJUSTMENT=$(jq -r '.gas_adjustment' "$CONFIG_FILE")
DEPLOYER_WALLET=$(jq -r '.deployer_wallet' "$CONFIG_FILE")
ADMIN_ADDRESS=$(jq -r '.admin_address' "$CONFIG_FILE")
# admin_wallet: keyring name for signing admin-only txs.
# If empty, the script will print the JSON and wait for manual confirmation.
ADMIN_WALLET=$(jq -r '.admin_wallet // empty' "$CONFIG_FILE")
# whitelist: array of addresses whitelisted in the Control Center and Vault.
# Falls back to [admin_address] if not set in config.
WHITELIST_JSON=$(jq -c 'if (.whitelist | length) > 0 then .whitelist else [.admin_address] end' "$CONFIG_FILE")
KEYRING_BACKEND=$(jq -r '.keyring_backend' "$CONFIG_FILE")

DEPOSIT_DENOM=$(jq -r '.deposit_denom // empty' "$CONFIG_FILE")
DEPOSIT_DENOM_TRACE=$(jq -r '.deposit_denom_trace // empty' "$CONFIG_FILE")
DEPOSIT_CAP=$(jq -r '.deposit_cap' "$CONFIG_FILE")
MAX_WITHDRAWALS=$(jq -r '.max_withdrawals' "$CONFIG_FILE")
VAULT_SUBDENOM=$(jq -r '.vault_subdenom' "$CONFIG_FILE")
TF_CREATION_FEE=$(jq -r '.tokenfactory_creation_fee // empty' "$CONFIG_FILE")
TOKEN_METADATA=$(jq -c '.token_metadata' "$CONFIG_FILE")

CONTROL_CENTER_CODE_ID=$(jq -r '.code_ids.control_center // empty' "$CONFIG_FILE")
VAULT_CODE_ID=$(jq -r '.code_ids.vault // empty' "$CONFIG_FILE")

VAULT_WASM=$(jq -r '.vault_wasm // empty' "$CONFIG_FILE")
if [ -z "$VAULT_WASM" ]; then
    echo -e "${RED}Error: vault_wasm must be set in config${NC}"
    exit 1
fi

# Build CLI command (optionally with --home)
if [ -n "$BINARY_HOME" ]; then
    CLI="$BINARY --home $BINARY_HOME"
else
    CLI="$BINARY"
fi

NODE_FLAG="--node $NODE"
KEYRING_FLAG="--keyring-backend $KEYRING_BACKEND"
TX_FLAGS="--gas auto --gas-adjustment $GAS_ADJUSTMENT --gas-prices $GAS_PRICE --chain-id $CHAIN_ID $NODE_FLAG $KEYRING_FLAG -y"

# ============================================================================
# Resolve IBC denom from trace (Neutron only)
# ============================================================================

if [ -n "$DEPOSIT_DENOM_TRACE" ] && [ -z "$DEPOSIT_DENOM" ]; then
    echo "Resolving IBC denom for trace: $DEPOSIT_DENOM_TRACE"
    DENOM_HASH=$($CLI q ibc-transfer denom-hash "$DEPOSIT_DENOM_TRACE" $NODE_FLAG --output json | jq -r '.hash')
    DEPOSIT_DENOM="ibc/$DENOM_HASH"
    echo "Resolved deposit denom: $DEPOSIT_DENOM"
    # Persist to config so subsequent runs skip the query
    TMP=$(mktemp)
    jq --arg d "$DEPOSIT_DENOM" '.deposit_denom = $d' "$CONFIG_FILE" > "$TMP" && mv "$TMP" "$CONFIG_FILE"
fi

if [ -z "$DEPOSIT_DENOM" ]; then
    echo -e "${RED}Error: deposit_denom could not be resolved. Set deposit_denom or deposit_denom_trace in config.${NC}"
    exit 1
fi

DEPLOYER_ADDRESS=$($CLI keys show "$DEPLOYER_WALLET" $KEYRING_FLAG --output json | jq -r .address)

echo -e "${GREEN}=== Inflow Vault Deployment ===${NC}"
echo ""
echo "  Config:    $CONFIG_FILE"
echo "  Binary:    $BINARY"
echo "  Chain ID:  $CHAIN_ID"
echo "  Node:      $NODE"
echo "  Deployer:  $DEPLOYER_ADDRESS"
echo "  Admin:     $ADMIN_ADDRESS"
echo "  Denom:     $DEPOSIT_DENOM"
if [ -n "$TF_CREATION_FEE" ]; then
    echo "  TF Fee:    $TF_CREATION_FEE (sent with vault instantiate)"
fi
echo ""

# ============================================================================
# Helper functions
# ============================================================================

retry_command() {
    set +e
    local output
    local status
    local max_attempts=${2:-0}
    local attempt=1

    while true; do
        output=$(eval "$1" 2>&1)
        status=$?

        if [ $status -eq 0 ]; then
            echo "" >&2
            echo "$output"
            set -e
            return 0
        fi

        if [ $max_attempts -gt 0 ] && [ $attempt -ge $max_attempts ]; then
            echo "Error: Maximum retry attempts ($max_attempts) reached" >&2
            echo "$output" >&2
            set -e
            return $status
        fi

        printf "." >&2
        sleep 1
        ((attempt++))
    done
}

update_config() {
    local key=$1
    local value=$2
    local tmp_file=$(mktemp)
    jq "$key = \"$value\"" "$CONFIG_FILE" > "$tmp_file" && mv "$tmp_file" "$CONFIG_FILE"
}

# Execute an admin-only tx.
# If admin_wallet is set in config, signs with that keyring key.
# Otherwise prints the message and waits for manual confirmation.
exec_admin_tx() {
    local label="$1" contract="$2" msg="$3" res_file="$4"

    if [ -n "$ADMIN_WALLET" ]; then
        $CLI tx wasm execute "$contract" "$msg" \
            --from "$ADMIN_WALLET" \
            $TX_FLAGS --output json \
            &> "$res_file"
        TX_HASH=$(grep -o '{.*}' "$res_file" | jq -r '.txhash')
        retry_command "$CLI q tx $TX_HASH $NODE_FLAG --output json" 60 > /dev/null
    else
        echo -e "${YELLOW}admin_wallet not set — execute the following on ${label} (${contract}):${NC}"
        echo ""
        echo "$msg" | jq .
        echo ""
        read -p "Press Enter once the transaction is confirmed..."
    fi
}

# ============================================================================
# Store (upload) functions
# ============================================================================

store_contract() {
    local contract_name=$1
    local wasm_path=$2
    local config_key=$3

    printf "Storing $contract_name wasm"

    $CLI tx wasm store "$wasm_path" --from "$DEPLOYER_WALLET" $TX_FLAGS --output json \
        &> "./store_${contract_name}_res.json"

    TX_HASH=$(grep -o '{.*}' "./store_${contract_name}_res.json" | jq -r '.txhash')
    TX_RESULT=$(retry_command "$CLI q tx $TX_HASH $NODE_FLAG --output json" 60)
    CODE_ID=$(echo "$TX_RESULT" | jq -r '.events[] | select(.type == "store_code") | .attributes[] | select(.key == "code_id") | .value')

    echo "$contract_name stored with code ID: $CODE_ID"
    update_config ".code_ids.$config_key" "$CODE_ID"
    echo "$CODE_ID"
}

CONTROL_CENTER_CODE_REDEPLOYED=false
VAULT_CODE_REDEPLOYED=false

deploy_control_center_code() {
    if [ -n "$CONTROL_CENTER_CODE_ID" ] && [ "$CONTROL_CENTER_CODE_ID" != "null" ]; then
        echo -e "${YELLOW}Existing Control Center code ID: $CONTROL_CENTER_CODE_ID${NC}"
        read -p "Redeploy Control Center? (y/N): " -n 1 -r; echo
        if [[ $REPLY =~ ^[Yy]$ ]]; then
            CONTROL_CENTER_CODE_ID=$(store_contract "control_center" "$CONTROL_CENTER_WASM" "control_center")
            CONTROL_CENTER_CODE_REDEPLOYED=true
        fi
    else
        echo "No existing Control Center code ID. Deploying..."
        CONTROL_CENTER_CODE_ID=$(store_contract "control_center" "$CONTROL_CENTER_WASM" "control_center")
        CONTROL_CENTER_CODE_REDEPLOYED=true
    fi
    echo ""
}

deploy_vault_code() {
    if [ -n "$VAULT_CODE_ID" ] && [ "$VAULT_CODE_ID" != "null" ]; then
        echo -e "${YELLOW}Existing Vault code ID: $VAULT_CODE_ID${NC}"
        read -p "Redeploy Vault? (y/N): " -n 1 -r; echo
        if [[ $REPLY =~ ^[Yy]$ ]]; then
            VAULT_CODE_ID=$(store_contract "vault" "$VAULT_WASM" "vault")
            VAULT_CODE_REDEPLOYED=true
        fi
    else
        echo "No existing Vault code ID. Deploying..."
        VAULT_CODE_ID=$(store_contract "vault" "$VAULT_WASM" "vault")
        VAULT_CODE_REDEPLOYED=true
    fi
    echo ""
}

# ============================================================================
# Instantiation functions
# ============================================================================

instantiate_control_center() {
    CONTROL_CENTER_NEWLY_INSTANTIATED=false
    EXISTING=$(jq -r '.contracts.control_center // empty' "$CONFIG_FILE")
    if [ -n "$EXISTING" ] && [ "$EXISTING" != "null" ] && [ "$CONTROL_CENTER_CODE_REDEPLOYED" != "true" ]; then
        echo -e "${YELLOW}Existing Control Center: $EXISTING${NC}"
        read -p "Reinstantiate Control Center? (y/N): " -n 1 -r; echo
        if [[ ! $REPLY =~ ^[Yy]$ ]]; then
            CONTROL_CENTER_ADDRESS="$EXISTING"
            echo "Using existing Control Center at: $CONTROL_CENTER_ADDRESS"
            return
        fi
    fi

    printf "Instantiating Control Center"

    INIT_MSG=$(jq -n \
        --argjson whitelist "$WHITELIST_JSON" \
        --arg deposit_cap "$DEPOSIT_CAP" \
        '{
            subvaults: [],
            whitelist: $whitelist,
            deposit_cap: $deposit_cap
        }')

    $CLI tx wasm instantiate "$CONTROL_CENTER_CODE_ID" "$INIT_MSG" \
        --admin "$ADMIN_ADDRESS" \
        --label "Inflow Control Center" \
        --from "$DEPLOYER_WALLET" \
        $TX_FLAGS \
        --output json &> ./instantiate_control_center_res.json

    TX_HASH=$(grep -o '{.*}' ./instantiate_control_center_res.json | jq -r '.txhash')
    TX_RESULT=$(retry_command "$CLI q tx $TX_HASH $NODE_FLAG --output json" 60)
    export CONTROL_CENTER_ADDRESS=$(echo "$TX_RESULT" | jq -r '.events[] | select(.type == "instantiate") | .attributes[] | select(.key == "_contract_address") | .value')

    echo "Control Center instantiated at: $CONTROL_CENTER_ADDRESS"
    update_config ".contracts.control_center" "$CONTROL_CENTER_ADDRESS"
    CONTROL_CENTER_NEWLY_INSTANTIATED=true
}

instantiate_vault() {
    VAULT_NEWLY_INSTANTIATED=false
    EXISTING=$(jq -r '.contracts.vault // empty' "$CONFIG_FILE")
    if [ -n "$EXISTING" ] && [ "$EXISTING" != "null" ] && [ "$VAULT_CODE_REDEPLOYED" != "true" ]; then
        echo -e "${YELLOW}Existing Vault: $EXISTING${NC}"
        read -p "Reinstantiate Vault? (y/N): " -n 1 -r; echo
        if [[ ! $REPLY =~ ^[Yy]$ ]]; then
            VAULT_ADDRESS="$EXISTING"
            echo "Using existing Vault at: $VAULT_ADDRESS"
            return
        fi
    fi

    printf "Instantiating Vault"

    INIT_MSG=$(jq -n \
        --argjson whitelist "$WHITELIST_JSON" \
        --arg control_center "$CONTROL_CENTER_ADDRESS" \
        --arg deposit_denom "$DEPOSIT_DENOM" \
        --argjson max_withdrawals "$MAX_WITHDRAWALS" \
        --arg subdenom "$VAULT_SUBDENOM" \
        --argjson token_metadata "$TOKEN_METADATA" \
        '{
            whitelist: $whitelist,
            control_center_contract: $control_center,
            deposit_denom: $deposit_denom,
            max_withdrawals_per_user: $max_withdrawals,
            subdenom: $subdenom,
            token_metadata: $token_metadata
        }')

    # On Cosmos Hub, the vault's MsgCreateDenom submessage requires a TokenFactory
    # creation fee. Send the fee as --amount so the contract can pay it.
    AMOUNT_FLAG=""
    if [ -n "$TF_CREATION_FEE" ]; then
        AMOUNT_FLAG="--amount $TF_CREATION_FEE"
    fi

    $CLI tx wasm instantiate "$VAULT_CODE_ID" "$INIT_MSG" \
        --admin "$ADMIN_ADDRESS" \
        --label "Inflow ATOM Vault" \
        --from "$DEPLOYER_WALLET" \
        $AMOUNT_FLAG \
        $TX_FLAGS \
        --output json &> ./instantiate_vault_res.json

    TX_HASH=$(grep -o '{.*}' ./instantiate_vault_res.json | jq -r '.txhash')
    TX_RESULT=$(retry_command "$CLI q tx $TX_HASH $NODE_FLAG --output json" 60)
    export VAULT_ADDRESS=$(echo "$TX_RESULT" | jq -r '.events[] | select(.type == "instantiate") | .attributes[] | select(.key == "_contract_address") | .value')

    echo "Vault instantiated at: $VAULT_ADDRESS"
    update_config ".contracts.vault" "$VAULT_ADDRESS"
    VAULT_NEWLY_INSTANTIATED=true
}

# ============================================================================
# Post-instantiation wiring
# ============================================================================

register_subvault() {
    printf "Registering Vault as subvault in Control Center"

    ADD_SUBVAULT_MSG=$(cat <<EOF
{
  "add_subvault": {
    "address": "$VAULT_ADDRESS"
  }
}
EOF
)

    exec_admin_tx "Control Center" "$CONTROL_CENTER_ADDRESS" "$ADD_SUBVAULT_MSG" \
        "./add_subvault_res.json"

    echo "Vault registered as subvault"
}

# ============================================================================
# Main
# ============================================================================

main() {
    CONTROL_CENTER_NEWLY_INSTANTIATED=false
    VAULT_NEWLY_INSTANTIATED=false

    echo -e "${BLUE}=== Step 1: Upload contract code ===${NC}"
    echo ""
    deploy_control_center_code
    deploy_vault_code

    CONTROL_CENTER_CODE_ID=$(jq -r '.code_ids.control_center' "$CONFIG_FILE")
    VAULT_CODE_ID=$(jq -r '.code_ids.vault' "$CONFIG_FILE")

    echo -e "${BLUE}=== Step 2: Instantiate contracts ===${NC}"
    echo ""
    instantiate_control_center
    echo ""
    instantiate_vault
    echo ""

    if [ "$CONTROL_CENTER_NEWLY_INSTANTIATED" = true ] || [ "$VAULT_NEWLY_INSTANTIATED" = true ]; then
        echo -e "${BLUE}=== Step 3: Register vault as subvault ===${NC}"
        echo ""
        register_subvault
        echo ""
    else
        echo -e "${YELLOW}=== Step 3: Skipping subvault registration (no new contracts) ===${NC}"
        echo ""
    fi

    echo -e "${GREEN}=== Deployment complete ===${NC}"
    echo ""
    echo "Code IDs:"
    echo "  Control Center: $CONTROL_CENTER_CODE_ID"
    echo "  Vault:          $VAULT_CODE_ID"
    echo ""
    echo "Contract addresses:"
    echo "  Control Center: $CONTROL_CENTER_ADDRESS"
    echo "  Vault:          $VAULT_ADDRESS"
    echo ""
    echo "Configuration saved to: $CONFIG_FILE"
    echo ""
    echo "Verification commands:"
    echo "  $CLI q wasm contract-state smart $CONTROL_CENTER_ADDRESS '{\"config\":{}}' $NODE_FLAG -o json"
    echo "  $CLI q wasm contract-state smart $VAULT_ADDRESS '{\"config\":{}}' $NODE_FLAG -o json"
    echo "  $CLI q wasm contract-state smart $CONTROL_CENTER_ADDRESS '{\"subvaults\":{}}' $NODE_FLAG -o json"
}

main
