#!/bin/bash

# Deploy and register CCTP Adapter + Skip Adapter + Reserve Adapter on Cosmos Hub devnet.
# All adapters are registered on the existing Inflow vault after instantiation.
#
# Usage: ./deploy-adapters-cosmoshub.sh <config-file.json>
# Example: ./deploy-adapters-cosmoshub.sh deploy-config.json
#
# Prerequisites:
#   - artifacts/cctp_adapter_cosmoshub.wasm      (built via `make compile`)
#   - artifacts/skip_adapter_cosmoshub.wasm      (built via `make compile`)
#   - artifacts/basic_adapter_cosmoshub.wasm     (built via `make compile`)
#   - Vault already deployed (address in config file)
#   - TODO fields in config file filled in

set -eo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

if [ -z "$1" ]; then
    echo -e "${RED}Error: config file argument required${NC}"
    echo ""
    echo "Usage: $0 <config-file.json>"
    exit 1
fi

CONFIG_FILE="$1"

if [ ! -f "$CONFIG_FILE" ]; then
    echo -e "${RED}Error: config file '$CONFIG_FILE' not found${NC}"
    exit 1
fi

CCTP_ADAPTER_WASM="../../artifacts/cctp_adapter_cosmoshub.wasm"
SKIP_ADAPTER_WASM="../../artifacts/skip_adapter_cosmoshub.wasm"
RESERVE_ADAPTER_WASM="../../artifacts/basic_adapter_cosmoshub.wasm"

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
KEYRING=$(jq -r '.keyring_backend' "$CONFIG_FILE")
VAULT_ADDRESS=$(jq -r '.vault_address' "$CONFIG_FILE")
SKIP_VAULT_REGISTRATION=$(jq -r '.skip_vault_registration // false' "$CONFIG_FILE")

# CCTP adapter config
CCTP_USDC_DENOM=$(jq -r '.cctp_adapter.usdc_denom' "$CONFIG_FILE")
CCTP_NOBLE_CHANNEL=$(jq -r '.cctp_adapter.noble_transfer_channel_id' "$CONFIG_FILE")
CCTP_TIMEOUT=$(jq -r '.cctp_adapter.ibc_default_timeout_seconds' "$CONFIG_FILE")
CCTP_INITIAL_CHAINS=$(jq -c '.cctp_adapter.initial_chains' "$CONFIG_FILE")
CCTP_INITIAL_EXECUTORS=$(jq -c '.cctp_adapter.initial_executors // []' "$CONFIG_FILE")
CCTP_VAULT_NAME=$(jq -r '.cctp_adapter.vault_registration.name // "cctp-adapter"' "$CONFIG_FILE")
CCTP_ALLOC_MODE=$(jq -r '.cctp_adapter.vault_registration.allocation_mode // "manual"' "$CONFIG_FILE")
CCTP_TRACKING=$(jq -r '.cctp_adapter.vault_registration.deployment_tracking // "tracked"' "$CONFIG_FILE")

# Skip adapter config
SKIP_CONTRACTS=$(jq -c '.skip_adapter.skip_contracts' "$CONFIG_FILE")
SKIP_TIMEOUT=$(jq -r '.skip_adapter.default_timeout_nanos' "$CONFIG_FILE")
SKIP_SLIPPAGE=$(jq -r '.skip_adapter.max_slippage_bps' "$CONFIG_FILE")
SKIP_INITIAL_EXECUTORS=$(jq -c '.skip_adapter.initial_executors // []' "$CONFIG_FILE")
SKIP_VAULT_NAME=$(jq -r '.skip_adapter.vault_registration.name // "skip-adapter"' "$CONFIG_FILE")
SKIP_ALLOC_MODE=$(jq -r '.skip_adapter.vault_registration.allocation_mode // "manual"' "$CONFIG_FILE")
SKIP_TRACKING=$(jq -r '.skip_adapter.vault_registration.deployment_tracking // "tracked"' "$CONFIG_FILE")

# Reserve adapter config
RESERVE_VAULT_NAME=$(jq -r '.reserve_adapter.vault_registration.name // "reserve-adapter"' "$CONFIG_FILE")
RESERVE_ALLOC_MODE=$(jq -r '.reserve_adapter.vault_registration.allocation_mode // "manual"' "$CONFIG_FILE")
RESERVE_TRACKING=$(jq -r '.reserve_adapter.vault_registration.deployment_tracking // "tracked"' "$CONFIG_FILE")

# Existing code IDs / contract addresses (null → fresh deploy)
CCTP_CODE_ID=$(jq -r '.code_ids.cctp_adapter // empty' "$CONFIG_FILE")
SKIP_CODE_ID=$(jq -r '.code_ids.skip_adapter // empty' "$CONFIG_FILE")
RESERVE_CODE_ID=$(jq -r '.code_ids.reserve_adapter // empty' "$CONFIG_FILE")
CCTP_ADDRESS=$(jq -r '.contracts.cctp_adapter // empty' "$CONFIG_FILE")
SKIP_ADDRESS=$(jq -r '.contracts.skip_adapter // empty' "$CONFIG_FILE")
RESERVE_ADDRESS=$(jq -r '.contracts.reserve_adapter // empty' "$CONFIG_FILE")

if [ -n "$BINARY_HOME" ]; then
    CLI="$BINARY --home $BINARY_HOME"
else
    CLI="$BINARY"
fi

NODE_FLAG="--node $NODE"
KEYRING_FLAG="--keyring-backend $KEYRING"
TX_FLAGS="--gas auto --gas-adjustment $GAS_ADJUSTMENT --gas-prices $GAS_PRICE --chain-id $CHAIN_ID $NODE_FLAG $KEYRING_FLAG -y"

DEPLOYER_ADDRESS=$($CLI keys show "$DEPLOYER_WALLET" $KEYRING_FLAG --output json | jq -r .address)

echo -e "${GREEN}=== Cosmos Hub Adapter Deployment ===${NC}"
echo ""
echo "  Config:    $CONFIG_FILE"
echo "  Chain:     $CHAIN_ID"
echo "  Node:      $NODE"
echo "  Deployer:  $DEPLOYER_ADDRESS"
echo "  Admin:     $ADMIN_ADDRESS"
echo "  Vault:     $VAULT_ADDRESS"
echo ""

# ============================================================================
# Guard: warn about unfilled TODO placeholders
# ============================================================================

TODO_COUNT=$(grep -c '"TODO_' "$CONFIG_FILE" || true)
if [ "$TODO_COUNT" -gt 0 ]; then
    echo -e "${YELLOW}Warning: $TODO_COUNT TODO placeholder(s) remain in $CONFIG_FILE${NC}"
    echo "  Fill them in before running a real deployment."
    echo ""
fi

# ============================================================================
# Helper functions
# ============================================================================

retry_command() {
    set +e
    local output status max_attempts=${2:-0} attempt=1
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
    local key="$1" value="$2"
    local tmp; tmp=$(mktemp)
    jq "$key = \"$value\"" "$CONFIG_FILE" > "$tmp" && mv "$tmp" "$CONFIG_FILE"
}

update_config_number() {
    local key="$1" value="$2"
    local tmp; tmp=$(mktemp)
    jq "$key = $value" "$CONFIG_FILE" > "$tmp" && mv "$tmp" "$CONFIG_FILE"
}

# Execute an admin-only tx.
# Args: contract_label contract_address msg res_file [ignore_pattern]
#   contract_label:  human-readable name shown when prompting for manual execution
#   contract_address: the contract to execute on
#   msg:             JSON message
#   res_file:        file to write tx output to
#   ignore_pattern:  optional — if error output matches, skip with a warning instead of failing
exec_admin_tx() {
    local label="$1" contract="$2" msg="$3" res_file="$4" ignore_pattern="${5:-}"

    if [ -n "$ADMIN_WALLET" ]; then
        set +e
        $CLI tx wasm execute "$contract" "$msg" \
            --from "$ADMIN_WALLET" \
            $TX_FLAGS --output json \
            &> "$res_file"
        local tx_exit=$?
        set -e

        if [ -n "$ignore_pattern" ] && grep -q "$ignore_pattern" "$res_file" 2>/dev/null; then
            echo -e "${YELLOW}Skipped (already done: $ignore_pattern)${NC}"
            return 0
        fi

        if [ $tx_exit -ne 0 ]; then
            echo -e "${RED}Admin tx failed${NC}"
            cat "$res_file"
            return 1
        fi

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

store_contract() {
    local name="$1" wasm="$2" config_key="$3"

    if [ ! -f "$wasm" ]; then
        echo -e "${RED}Error: wasm artifact not found: $wasm${NC}"
        echo "  Run 'make compile' from the repo root to build contracts."
        exit 1
    fi

    printf "Storing %s wasm" "$name" >&2
    $CLI tx wasm store "$wasm" --from "$DEPLOYER_WALLET" $TX_FLAGS --output json \
        &> "./store_${name}_res.json"

    TX_HASH=$(grep -o '{.*}' "./store_${name}_res.json" | jq -r '.txhash')
    TX_RESULT=$(retry_command "$CLI q tx $TX_HASH $NODE_FLAG --output json" 60)
    CODE_ID=$(echo "$TX_RESULT" | jq -r '.events[] | select(.type == "store_code") | .attributes[] | select(.key == "code_id") | .value')

    echo "$name stored with code ID: $CODE_ID" >&2
    update_config_number "$config_key" "$CODE_ID"
    echo "$CODE_ID"
}

CCTP_CODE_REDEPLOYED=false
SKIP_CODE_REDEPLOYED=false
RESERVE_CODE_REDEPLOYED=false

CCTP_INSTANTIATED=false
SKIP_INSTANTIATED=false
RESERVE_INSTANTIATED=false

# ============================================================================
# Step 1: Upload contract code
# ============================================================================

echo -e "${BLUE}=== Step 1: Upload Contract Code ===${NC}"
echo ""

if [ -z "$CCTP_CODE_ID" ] || [ "$CCTP_CODE_ID" = "null" ]; then
    CCTP_CODE_ID=$(store_contract "cctp_adapter" "$CCTP_ADAPTER_WASM" ".code_ids.cctp_adapter")
    CCTP_CODE_REDEPLOYED=true
else
    echo "CCTP Adapter already uploaded (code ID: $CCTP_CODE_ID)"
    read -p "Upload new version? (y/N): " -n 1 -r; echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        CCTP_CODE_ID=$(store_contract "cctp_adapter" "$CCTP_ADAPTER_WASM" ".code_ids.cctp_adapter")
        CCTP_CODE_REDEPLOYED=true
    fi
fi

if [ -z "$SKIP_CODE_ID" ] || [ "$SKIP_CODE_ID" = "null" ]; then
    SKIP_CODE_ID=$(store_contract "skip_adapter" "$SKIP_ADAPTER_WASM" ".code_ids.skip_adapter")
    SKIP_CODE_REDEPLOYED=true
else
    echo "Skip Adapter already uploaded (code ID: $SKIP_CODE_ID)"
    read -p "Upload new version? (y/N): " -n 1 -r; echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        SKIP_CODE_ID=$(store_contract "skip_adapter" "$SKIP_ADAPTER_WASM" ".code_ids.skip_adapter")
        SKIP_CODE_REDEPLOYED=true
    fi
fi

if [ -z "$RESERVE_CODE_ID" ] || [ "$RESERVE_CODE_ID" = "null" ]; then
    RESERVE_CODE_ID=$(store_contract "reserve_adapter" "$RESERVE_ADAPTER_WASM" ".code_ids.reserve_adapter")
    RESERVE_CODE_REDEPLOYED=true
else
    echo "Reserve Adapter already uploaded (code ID: $RESERVE_CODE_ID)"
    read -p "Upload new version? (y/N): " -n 1 -r; echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        RESERVE_CODE_ID=$(store_contract "reserve_adapter" "$RESERVE_ADAPTER_WASM" ".code_ids.reserve_adapter")
        RESERVE_CODE_REDEPLOYED=true
    fi
fi

echo ""

# ============================================================================
# Step 2: Instantiate CCTP Adapter
# ============================================================================

echo -e "${BLUE}=== Step 2: Instantiate CCTP Adapter ===${NC}"
echo ""

instantiate_cctp_adapter() {
    # Build initial_chains array from config, mapping to the expected InstantiateMsg shape
    local initial_chains
    initial_chains=$(jq -c '[.cctp_adapter.initial_chains[] | {
        chain_config: {
            chain_id: .chain_id,
            bridging_config: .bridging_config
        },
        initial_allowed_destination_addresses: .initial_allowed_destination_addresses
    }]' "$CONFIG_FILE")

    local init_msg
    init_msg=$(jq -n \
        --arg admin "$ADMIN_ADDRESS" \
        --arg vault "$VAULT_ADDRESS" \
        --arg denom "$CCTP_USDC_DENOM" \
        --arg channel "$CCTP_NOBLE_CHANNEL" \
        --argjson timeout "$CCTP_TIMEOUT" \
        --argjson chains "$initial_chains" \
        --argjson executors "$CCTP_INITIAL_EXECUTORS" \
        '{
            admins: [$admin],
            denom: $denom,
            noble_transfer_channel_id: $channel,
            ibc_default_timeout_seconds: $timeout,
            initial_depositors: [
                { address: $vault, capabilities: { can_withdraw: true } }
            ],
            initial_chains: $chains,
            initial_executors: $executors
        }'
    )

    echo "Instantiate message:"
    echo "$init_msg" | jq .
    echo ""

    printf "Instantiating CCTP Adapter"
    $CLI tx wasm instantiate "$CCTP_CODE_ID" "$init_msg" \
        --from "$DEPLOYER_WALLET" \
        --label "Inflow CCTP Adapter" \
        --admin "$ADMIN_ADDRESS" \
        $TX_FLAGS --output json \
        &> ./instantiate_cctp_adapter_res.json

    TX_HASH=$(grep -o '{.*}' ./instantiate_cctp_adapter_res.json | jq -r '.txhash')
    TX_RESULT=$(retry_command "$CLI q tx $TX_HASH $NODE_FLAG --output json" 60)
    CCTP_ADDRESS=$(echo "$TX_RESULT" | jq -r '.events[] | select(.type == "instantiate") | .attributes[] | select(.key == "_contract_address") | .value')

    echo -e "${GREEN}CCTP Adapter instantiated at: $CCTP_ADDRESS${NC}"
    update_config ".contracts.cctp_adapter" "$CCTP_ADDRESS"
    CCTP_INSTANTIATED=true
}

if [ -z "$CCTP_ADDRESS" ] || [ "$CCTP_ADDRESS" = "null" ] || [ "$CCTP_CODE_REDEPLOYED" = "true" ]; then
    instantiate_cctp_adapter
else
    echo "CCTP Adapter already instantiated at: $CCTP_ADDRESS"
    read -p "Reinstantiate? (y/N): " -n 1 -r; echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        instantiate_cctp_adapter
    fi
fi

echo ""

# ============================================================================
# Step 3: Instantiate Skip Adapter
# ============================================================================

echo -e "${BLUE}=== Step 3: Instantiate Skip Adapter ===${NC}"
echo ""

instantiate_skip_adapter() {
    local init_msg
    init_msg=$(jq -n \
        --arg admin "$ADMIN_ADDRESS" \
        --arg vault "$VAULT_ADDRESS" \
        --argjson skip_contracts "$SKIP_CONTRACTS" \
        --argjson timeout "$SKIP_TIMEOUT" \
        --argjson slippage "$SKIP_SLIPPAGE" \
        --argjson executors "$SKIP_INITIAL_EXECUTORS" \
        '{
            admins: [$admin],
            skip_contracts: $skip_contracts,
            default_timeout_nanos: $timeout,
            max_slippage_bps: $slippage,
            executors: $executors,
            initial_routes: [],
            initial_depositors: [$vault]
        }'
    )

    echo "Instantiate message:"
    echo "$init_msg" | jq .
    echo ""

    printf "Instantiating Skip Adapter"
    $CLI tx wasm instantiate "$SKIP_CODE_ID" "$init_msg" \
        --from "$DEPLOYER_WALLET" \
        --label "Inflow Skip Swap Adapter" \
        --admin "$ADMIN_ADDRESS" \
        $TX_FLAGS --output json \
        &> ./instantiate_skip_adapter_res.json

    TX_HASH=$(grep -o '{.*}' ./instantiate_skip_adapter_res.json | jq -r '.txhash')
    TX_RESULT=$(retry_command "$CLI q tx $TX_HASH $NODE_FLAG --output json" 60)
    SKIP_ADDRESS=$(echo "$TX_RESULT" | jq -r '.events[] | select(.type == "instantiate") | .attributes[] | select(.key == "_contract_address") | .value')

    echo -e "${GREEN}Skip Adapter instantiated at: $SKIP_ADDRESS${NC}"
    update_config ".contracts.skip_adapter" "$SKIP_ADDRESS"
    SKIP_INSTANTIATED=true
}

if [ -z "$SKIP_ADDRESS" ] || [ "$SKIP_ADDRESS" = "null" ] || [ "$SKIP_CODE_REDEPLOYED" = "true" ]; then
    instantiate_skip_adapter
else
    echo "Skip Adapter already instantiated at: $SKIP_ADDRESS"
    read -p "Reinstantiate? (y/N): " -n 1 -r; echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        instantiate_skip_adapter
    fi
fi

echo ""

# ============================================================================
# Step 4: Instantiate Reserve Adapter
# ============================================================================

echo -e "${BLUE}=== Step 4: Instantiate Reserve Adapter ===${NC}"
echo ""

instantiate_reserve_adapter() {
    local init_msg
    init_msg=$(jq -n \
        --arg admin "$ADMIN_ADDRESS" \
        --arg vault "$VAULT_ADDRESS" \
        '{
            admins: [$admin],
            initial_depositors: [$vault]
        }'
    )

    echo "Instantiate message:"
    echo "$init_msg" | jq .
    echo ""

    printf "Instantiating Reserve Adapter"
    $CLI tx wasm instantiate "$RESERVE_CODE_ID" "$init_msg" \
        --from "$DEPLOYER_WALLET" \
        --label "Inflow Reserve Adapter" \
        --admin "$ADMIN_ADDRESS" \
        $TX_FLAGS --output json \
        &> ./instantiate_reserve_adapter_res.json

    TX_HASH=$(grep -o '{.*}' ./instantiate_reserve_adapter_res.json | jq -r '.txhash')
    TX_RESULT=$(retry_command "$CLI q tx $TX_HASH $NODE_FLAG --output json" 60)
    RESERVE_ADDRESS=$(echo "$TX_RESULT" | jq -r '.events[] | select(.type == "instantiate") | .attributes[] | select(.key == "_contract_address") | .value')

    echo -e "${GREEN}Reserve Adapter instantiated at: $RESERVE_ADDRESS${NC}"
    update_config ".contracts.reserve_adapter" "$RESERVE_ADDRESS"
    RESERVE_INSTANTIATED=true
}

if [ -z "$RESERVE_ADDRESS" ] || [ "$RESERVE_ADDRESS" = "null" ] || [ "$RESERVE_CODE_REDEPLOYED" = "true" ]; then
    instantiate_reserve_adapter
else
    echo "Reserve Adapter already instantiated at: $RESERVE_ADDRESS"
    read -p "Reinstantiate? (y/N): " -n 1 -r; echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        instantiate_reserve_adapter
    fi
fi

echo ""

# ============================================================================
# Step 5: Register vault as depositor on pre-existing adapters
#
# Skipped automatically for adapters that were just instantiated in this run
# (vault is already in initial_depositors). Only runs for adapters that were
# reused from a prior deployment and may not have the vault registered.
# ============================================================================

register_depositor_on_adapter() {
    local adapter_name="$1" adapter_address="$2" metadata_b64="$3"

    echo "Registering vault as depositor on $adapter_name..."

    local exec_msg
    if [ -z "$metadata_b64" ] || [ "$metadata_b64" = "null" ]; then
        exec_msg=$(jq -n \
            --arg vault "$VAULT_ADDRESS" \
            '{
                standard_action: {
                    register_depositor: {
                        depositor_address: $vault,
                        metadata: null
                    }
                }
            }'
        )
    else
        exec_msg=$(jq -n \
            --arg vault "$VAULT_ADDRESS" \
            --arg meta "$metadata_b64" \
            '{
                standard_action: {
                    register_depositor: {
                        depositor_address: $vault,
                        metadata: $meta
                    }
                }
            }'
        )
    fi

    exec_admin_tx "$adapter_name" "$adapter_address" "$exec_msg" \
        "./register_vault_depositor_${adapter_name}_res.json" \
        "Depositor already registered"

    echo -e "${GREEN}Vault registered as depositor on $adapter_name${NC}"
}

PREEXISTING_ADAPTERS=()
[ "$CCTP_INSTANTIATED" = "false" ] && PREEXISTING_ADAPTERS+=("cctp")
[ "$SKIP_INSTANTIATED" = "false" ] && PREEXISTING_ADAPTERS+=("skip")
[ "$RESERVE_INSTANTIATED" = "false" ] && PREEXISTING_ADAPTERS+=("reserve")

if [ ${#PREEXISTING_ADAPTERS[@]} -gt 0 ]; then
    echo -e "${BLUE}=== Step 5: Register Vault as Depositor on Pre-existing Adapters ===${NC}"
    echo ""
    echo "Pre-existing adapters (not instantiated this run): ${PREEXISTING_ADAPTERS[*]}"
    read -p "Register vault as depositor on these adapters? (y/N): " -n 1 -r; echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        CCTP_CAPS_B64=$(printf '{"can_withdraw":true}' | base64 | tr -d '\n')
        [[ " ${PREEXISTING_ADAPTERS[*]} " =~ " cctp " ]] && register_depositor_on_adapter "cctp_adapter" "$CCTP_ADDRESS" "$CCTP_CAPS_B64"
        [[ " ${PREEXISTING_ADAPTERS[*]} " =~ " skip " ]] && register_depositor_on_adapter "skip_adapter" "$SKIP_ADDRESS" "null"
        [[ " ${PREEXISTING_ADAPTERS[*]} " =~ " reserve " ]] && register_depositor_on_adapter "reserve_adapter" "$RESERVE_ADDRESS" "null"
    else
        echo "Skipped."
    fi
    echo ""
fi

echo ""

# ============================================================================
# Step 6: Register adapters on the vault
# ============================================================================

if [ "$SKIP_VAULT_REGISTRATION" = "true" ]; then
    echo -e "${YELLOW}=== Step 6: Skipping vault registration (skip_vault_registration=true) ===${NC}"
    echo "  Adapters will not be registered on a vault."
    echo "  vault_address is used as the initial depositor for direct testing."
else
    echo -e "${BLUE}=== Step 6: Register Adapters on Vault ===${NC}"
    echo ""

    register_adapter_on_vault() {
        local name="$1" address="$2" allocation_mode="$3" deployment_tracking="$4" res_file="$5"

        echo "Registering '$name' on vault..."

        local exec_msg
        exec_msg=$(jq -n \
            --arg name "$name" \
            --arg address "$address" \
            --arg allocation_mode "$allocation_mode" \
            --arg deployment_tracking "$deployment_tracking" \
            '{
                register_adapter: {
                    name: $name,
                    address: $address,
                    allocation_mode: $allocation_mode,
                    deployment_tracking: $deployment_tracking
                }
            }'
        )

        echo "$exec_msg" | jq .

        $CLI tx wasm execute "$VAULT_ADDRESS" "$exec_msg" \
            --from "$DEPLOYER_WALLET" \
            $TX_FLAGS --output json \
            &> "./${res_file}"

        TX_HASH=$(grep -o '{.*}' "./${res_file}" | jq -r '.txhash')
        retry_command "$CLI q tx $TX_HASH $NODE_FLAG --output json" 60 > /dev/null

        echo -e "${GREEN}'$name' registered on vault${NC}"
        echo ""
    }

    register_adapter_on_vault "$CCTP_VAULT_NAME" "$CCTP_ADDRESS" "$CCTP_ALLOC_MODE" "$CCTP_TRACKING" \
        "register_cctp_adapter_res.json"

    register_adapter_on_vault "$SKIP_VAULT_NAME" "$SKIP_ADDRESS" "$SKIP_ALLOC_MODE" "$SKIP_TRACKING" \
        "register_skip_adapter_res.json"

    register_adapter_on_vault "$RESERVE_VAULT_NAME" "$RESERVE_ADDRESS" "$RESERVE_ALLOC_MODE" "$RESERVE_TRACKING" \
        "register_reserve_adapter_res.json"
fi

# ============================================================================
# Step 7: Register initial routes on Skip Adapter (optional)
# ============================================================================

echo -e "${BLUE}=== Step 7: Register Routes on Skip Adapter ===${NC}"
echo ""

ROUTE_KEYS=$(jq -r '.skip_adapter.initial_routes | keys[]' "$CONFIG_FILE" 2>/dev/null || echo "")

if [ -z "$ROUTE_KEYS" ]; then
    echo "No initial_routes defined in config. Skipping."
else
    echo "Routes to register: $ROUTE_KEYS"
    read -p "Register routes now? (y/N): " -n 1 -r; echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        for route_key in $ROUTE_KEYS; do
            echo "Registering route: $route_key"

            local_route=$(jq -c ".skip_adapter.initial_routes.$route_key" "$CONFIG_FILE")
            route_id=$(echo "$local_route" | jq -r '.route_id')

            # Build the UnifiedRoute object — receiver "" on return path becomes skip adapter address
            route_obj=$(echo "$local_route" | jq -c \
                --arg skip_addr "$SKIP_ADDRESS" \
                '{
                    venue: .venue,
                    denom_in: .denom_in,
                    denom_out: .denom_out,
                    operations: .operations,
                    swap_venue_name: .swap_venue_name,
                    forward_path: .forward_path,
                    return_path: [.return_path[] | {
                        chain_id,
                        channel,
                        receiver: (if .receiver == "" then $skip_addr else .receiver end)
                    }],
                    recover_address: (if .recover_address == "" or .recover_address == null then null else .recover_address end),
                    enabled: true
                }'
            )

            exec_msg=$(jq -n \
                --arg route_id "$route_id" \
                --argjson route "$route_obj" \
                '{
                    custom_action: {
                        register_route: {
                            route_id: $route_id,
                            route: $route
                        }
                    }
                }'
            )

            echo "$exec_msg" | jq .

            exec_admin_tx "Skip Adapter" "$SKIP_ADDRESS" "$exec_msg" \
                "./register_route_${route_key}_res.json"

            echo -e "${GREEN}Route '$route_id' registered${NC}"
            echo ""
        done
    else
        echo -e "${YELLOW}Skipping route registration. Register manually via:${NC}"
        echo "  gaiad tx wasm execute \$SKIP_ADDRESS '{\"custom_action\":{\"register_route\":{...}}}' ..."
    fi
fi

echo ""

# ============================================================================
# Summary
# ============================================================================

echo -e "${GREEN}=== Deployment Summary ===${NC}"
echo ""
echo "Chain:   $CHAIN_ID  ($NODE)"
echo "Vault:   $VAULT_ADDRESS"
echo ""
echo "Contract Addresses:"
echo "  CCTP Adapter:    $CCTP_ADDRESS"
echo "  Skip Adapter:    $SKIP_ADDRESS"
echo "  Reserve Adapter: $RESERVE_ADDRESS"
echo ""
echo "Code IDs:"
echo "  CCTP Adapter:    $CCTP_CODE_ID"
echo "  Skip Adapter:    $SKIP_CODE_ID"
echo "  Reserve Adapter: $RESERVE_CODE_ID"
echo ""
echo "State saved to: $CONFIG_FILE"
echo ""
echo "Next steps:"
echo "  1. Fill in any remaining TODO values in $CONFIG_FILE"
echo "  2. Register EVM destination addresses on CCTP Adapter:"
echo "     gaiad tx wasm execute \$CCTP_ADDRESS '{\"custom_action\":{\"add_allowed_destination_address\":{\"chain_id\":\"ethereum\",\"address\":\"0x...\"}}}' ..."
echo "  3. Verify deployment with query commands below"
echo ""
echo -e "${GREEN}Done!${NC}"

# ============================================================================
# Verification queries (reference — not executed automatically)
# ============================================================================
#
# CCTP_ADAPTER=<address from above>
# SKIP_ADAPTER=<address from above>
# RESERVE_ADAPTER=<address from above>
# VAULT=cosmos1436kxs0w2es6xlqpp9rd35e3d0cjnw4sv8j3a7483sgks29jqwgsks67u5
# NODE=https://rpc-dev-cosmoshub.moonkitt.com
#
# --- CCTP ADAPTER ---
#
# Config:
# gaiad query wasm contract-state smart $CCTP_ADAPTER '{"custom_query":{"all_chains":{}}}' --node $NODE
#
# Depositors:
# gaiad query wasm contract-state smart $CCTP_ADAPTER '{"standard_query":{"registered_depositors":{}}}' --node $NODE
#
# Executors:
# gaiad query wasm contract-state smart $CCTP_ADAPTER '{"custom_query":{"executors":{}}}' --node $NODE
#
# Allowed destinations for Ethereum:
# gaiad query wasm contract-state smart $CCTP_ADAPTER '{"custom_query":{"allowed_destination_addresses":{"chain_id":"ethereum"}}}' --node $NODE
#
# --- SKIP ADAPTER ---
#
# Config (admins, skip_contracts, timeouts):
# gaiad query wasm contract-state smart $SKIP_ADAPTER '{"standard_query":{"config":{}}}' --node $NODE
#
# All routes:
# gaiad query wasm contract-state smart $SKIP_ADAPTER '{"custom_query":{"all_routes":{}}}' --node $NODE
#
# Executors:
# gaiad query wasm contract-state smart $SKIP_ADAPTER '{"custom_query":{"executors":{}}}' --node $NODE
#
# Depositors:
# gaiad query wasm contract-state smart $SKIP_ADAPTER '{"standard_query":{"registered_depositors":{}}}' --node $NODE
#
# --- RESERVE ADAPTER ---
#
# Config (admins):
# gaiad query wasm contract-state smart $RESERVE_ADAPTER '{"standard_query":{"config":{}}}' --node $NODE
#
# Depositors:
# gaiad query wasm contract-state smart $RESERVE_ADAPTER '{"standard_query":{"registered_depositors":{}}}' --node $NODE
#
# Available balance (uatom):
# gaiad query wasm contract-state smart $RESERVE_ADAPTER '{"standard_query":{"available_for_withdraw":{"depositor_address":"<vault>","denom":"uatom"}}}' --node $NODE
#
# --- VAULT ---
#
# Registered adapters:
# gaiad query wasm contract-state smart $VAULT '{"adapters":{}}' --node $NODE
