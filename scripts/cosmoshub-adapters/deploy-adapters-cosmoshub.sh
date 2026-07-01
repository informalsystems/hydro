#!/bin/bash

# Deploy and register adapters on Cosmos Hub.
# One config per risk level; adapters are shared across all vaults in the same tier.
#
# Usage: ./deploy-adapters-cosmoshub.sh <config-file.json>
# Example: ./deploy-adapters-cosmoshub.sh deploy-config-adapters-cosmoshub-risk2-mainnet.json
#
# Prerequisites:
#   - artifacts/cctp_adapter_cosmoshub.wasm      (built via `make compile`)
#   - artifacts/skip_adapter_cosmoshub.wasm      (built via `make compile`)
#   - artifacts/basic_adapter_cosmoshub.wasm     (built via `make compile`)
#   - Vaults already deployed (addresses in config file)

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
# If empty, the script prints the JSON and waits for manual confirmation.
ADMIN_WALLET=$(jq -r '.admin_wallet // empty' "$CONFIG_FILE")
KEYRING=$(jq -r '.keyring_backend' "$CONFIG_FILE")

# whitelist: becomes the admins array passed to each adapter at instantiation.
WHITELIST_JSON=$(jq -c 'if (.whitelist | length) > 0 then .whitelist else [.admin_address] end' "$CONFIG_FILE")

# vaults: required array of {name, address, adapters[]} objects
VAULTS_JSON=$(jq -c '.vaults // empty' "$CONFIG_FILE")
if [ -z "$VAULTS_JSON" ] || [ "$VAULTS_JSON" = "null" ]; then
    echo -e "${RED}Error: 'vaults' array is required in config${NC}"
    exit 1
fi

# Derive which adapters are needed from the vaults list
DEPLOY_CCTP=$(jq 'any(.vaults[].adapters[]; . == "cctp")' "$CONFIG_FILE")
DEPLOY_SKIP=$(jq 'any(.vaults[].adapters[]; . == "skip")' "$CONFIG_FILE")
DEPLOY_RESERVE=$(jq 'any(.vaults[].adapters[]; . == "reserve")' "$CONFIG_FILE")

# CCTP adapter config
CCTP_USDC_DENOM=$(jq -r '.cctp_adapter.usdc_denom // empty' "$CONFIG_FILE")
CCTP_NOBLE_CHANNEL=$(jq -r '.cctp_adapter.noble_transfer_channel_id // empty' "$CONFIG_FILE")
CCTP_TIMEOUT=$(jq -r '.cctp_adapter.ibc_default_timeout_seconds // 600' "$CONFIG_FILE")
CCTP_INITIAL_CHAINS=$(jq -c '.cctp_adapter.initial_chains // []' "$CONFIG_FILE")
CCTP_INITIAL_EXECUTORS=$(jq -c '.cctp_adapter.initial_executors // []' "$CONFIG_FILE")
CCTP_VAULT_NAME=$(jq -r '.cctp_adapter.vault_registration.name // "cctp-adapter"' "$CONFIG_FILE")
CCTP_ALLOC_MODE=$(jq -r '.cctp_adapter.vault_registration.allocation_mode // "manual"' "$CONFIG_FILE")
CCTP_TRACKING=$(jq -r '.cctp_adapter.vault_registration.deployment_tracking // "tracked"' "$CONFIG_FILE")

# Skip adapter config
SKIP_CONTRACTS=$(jq -c '.skip_adapter.skip_contracts // {}' "$CONFIG_FILE")
SKIP_TIMEOUT=$(jq -r '.skip_adapter.default_timeout_nanos // 1800000000000' "$CONFIG_FILE")
SKIP_SLIPPAGE=$(jq -r '.skip_adapter.max_slippage_bps // 100' "$CONFIG_FILE")
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
echo "  Adapters:  CCTP=$DEPLOY_CCTP  Skip=$DEPLOY_SKIP  Reserve=$DEPLOY_RESERVE"
echo ""
echo "  Vaults:"
jq -r '.vaults[] | "    \(.name): \(.address) [\(.adapters | join(", "))]"' "$CONFIG_FILE"
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
# If admin_wallet is set in config, signs with that keyring key.
# Otherwise prints the message and waits for manual confirmation.
exec_admin_tx() {
    local label="$1" contract="$2" msg="$3" res_file="$4"

    if [ -n "$ADMIN_WALLET" ]; then
        set +e
        $CLI tx wasm execute "$contract" "$msg" \
            --from "$ADMIN_WALLET" \
            $TX_FLAGS --output json \
            &> "$res_file"
        local tx_exit=$?
        set -e

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
        read -p "Press Enter once the transaction is confirmed..." < /dev/tty
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

# ============================================================================
# Step 1: Upload contract code
# ============================================================================

echo -e "${BLUE}=== Step 1: Upload Contract Code ===${NC}"
echo ""

if [ "$DEPLOY_CCTP" = "true" ]; then
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
fi

if [ "$DEPLOY_SKIP" = "true" ]; then
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
fi

if [ "$DEPLOY_RESERVE" = "true" ]; then
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
fi

echo ""

# ============================================================================
# Step 2: Instantiate CCTP Adapter
# ============================================================================

if [ "$DEPLOY_CCTP" = "true" ]; then
    echo -e "${BLUE}=== Step 2: Instantiate CCTP Adapter ===${NC}"
    echo ""

    instantiate_cctp_adapter() {
        local initial_chains cctp_depositors init_msg
        initial_chains=$(jq -c '[.cctp_adapter.initial_chains[] | {
            chain_config: {
                chain_id: .chain_id,
                bridging_config: .bridging_config
            },
            initial_allowed_destination_addresses: .initial_allowed_destination_addresses
        }]' "$CONFIG_FILE")

        # All vaults that list "cctp" become initial depositors
        cctp_depositors=$(jq -c '[.vaults[] | select(.adapters[] | . == "cctp") | {address: .address, capabilities: {can_withdraw: true}}]' "$CONFIG_FILE")

        init_msg=$(jq -n \
            --argjson admins "$WHITELIST_JSON" \
            --arg denom "$CCTP_USDC_DENOM" \
            --arg channel "$CCTP_NOBLE_CHANNEL" \
            --argjson timeout "$CCTP_TIMEOUT" \
            --argjson chains "$initial_chains" \
            --argjson executors "$CCTP_INITIAL_EXECUTORS" \
            --argjson depositors "$cctp_depositors" \
            '{
                admins: $admins,
                denom: $denom,
                noble_transfer_channel_id: $channel,
                ibc_default_timeout_seconds: $timeout,
                initial_depositors: $depositors,
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
fi

# ============================================================================
# Step 3: Instantiate Skip Adapter
# ============================================================================

if [ "$DEPLOY_SKIP" = "true" ]; then
    echo -e "${BLUE}=== Step 3: Instantiate Skip Adapter ===${NC}"
    echo ""

    instantiate_skip_adapter() {
        local skip_depositors init_msg
        # All vaults that list "skip" become initial depositors (plain addresses)
        skip_depositors=$(jq -c '[.vaults[] | select(.adapters[] | . == "skip") | .address]' "$CONFIG_FILE")

        init_msg=$(jq -n \
            --argjson admins "$WHITELIST_JSON" \
            --argjson skip_contracts "$SKIP_CONTRACTS" \
            --argjson timeout "$SKIP_TIMEOUT" \
            --argjson slippage "$SKIP_SLIPPAGE" \
            --argjson executors "$SKIP_INITIAL_EXECUTORS" \
            --argjson depositors "$skip_depositors" \
            '{
                admins: $admins,
                skip_contracts: $skip_contracts,
                default_timeout_nanos: $timeout,
                max_slippage_bps: $slippage,
                executors: $executors,
                initial_routes: [],
                initial_depositors: $depositors
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
fi

# ============================================================================
# Step 4: Instantiate Reserve Adapter
# ============================================================================

if [ "$DEPLOY_RESERVE" = "true" ]; then
    echo -e "${BLUE}=== Step 4: Instantiate Reserve Adapter ===${NC}"
    echo ""

    instantiate_reserve_adapter() {
        local reserve_depositors init_msg
        # All vaults that list "reserve" become initial depositors (plain addresses)
        reserve_depositors=$(jq -c '[.vaults[] | select(.adapters[] | . == "reserve") | .address]' "$CONFIG_FILE")

        init_msg=$(jq -n \
            --argjson admins "$WHITELIST_JSON" \
            --argjson depositors "$reserve_depositors" \
            '{
                admins: $admins,
                initial_depositors: $depositors
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
fi

# ============================================================================
# Step 5: Register adapters on each vault (requires vault admin — DAODAO multisig)
# ============================================================================

echo -e "${BLUE}=== Step 5: Register Adapters on Vaults ===${NC}"
echo ""
echo "Note: vault admin (DAODAO multisig) is required for this step."
if [ -z "$ADMIN_WALLET" ]; then
    echo "admin_wallet is not set — each message will be printed for manual submission."
fi
echo ""

get_adapter_address() {
    case "$1" in
        cctp)    echo "$CCTP_ADDRESS" ;;
        skip)    echo "$SKIP_ADDRESS" ;;
        reserve) echo "$RESERVE_ADDRESS" ;;
    esac
}
get_adapter_vault_name() {
    case "$1" in
        cctp)    echo "$CCTP_VAULT_NAME" ;;
        skip)    echo "$SKIP_VAULT_NAME" ;;
        reserve) echo "$RESERVE_VAULT_NAME" ;;
    esac
}
get_adapter_alloc_mode() {
    case "$1" in
        cctp)    echo "$CCTP_ALLOC_MODE" ;;
        skip)    echo "$SKIP_ALLOC_MODE" ;;
        reserve) echo "$RESERVE_ALLOC_MODE" ;;
    esac
}
get_adapter_tracking() {
    case "$1" in
        cctp)    echo "$CCTP_TRACKING" ;;
        skip)    echo "$SKIP_TRACKING" ;;
        reserve) echo "$RESERVE_TRACKING" ;;
    esac
}

while IFS= read -r vault; do
    vault_name=$(echo "$vault" | jq -r '.name')
    vault_addr=$(echo "$vault" | jq -r '.address')
    echo "--- Vault: $vault_name ($vault_addr) ---"

    adapters_list=$(echo "$vault" | jq -r '.adapters[]')
    for adapter in $adapters_list; do
        adapter_addr=$(get_adapter_address "$adapter")
        adapter_vault_name=$(get_adapter_vault_name "$adapter")
        adapter_alloc_mode=$(get_adapter_alloc_mode "$adapter")
        adapter_tracking=$(get_adapter_tracking "$adapter")

        echo "  Registering '$adapter_vault_name' on vault..."

        exec_msg=$(jq -n \
            --arg name "$adapter_vault_name" \
            --arg address "$adapter_addr" \
            --arg allocation_mode "$adapter_alloc_mode" \
            --arg deployment_tracking "$adapter_tracking" \
            '{
                register_adapter: {
                    name: $name,
                    address: $address,
                    allocation_mode: $allocation_mode,
                    deployment_tracking: $deployment_tracking
                }
            }')

        exec_admin_tx "$vault_name vault" "$vault_addr" "$exec_msg" \
            "./register_${adapter}_on_${vault_name}_res.json"

        echo -e "${GREEN}  '$adapter_vault_name' registered on $vault_name${NC}"
    done

    echo ""
done < <(jq -c '.vaults[]' "$CONFIG_FILE")

# ============================================================================
# Step 6: Register initial routes on Skip Adapter (optional)
# ============================================================================

if [ "$DEPLOY_SKIP" = "true" ]; then
    echo -e "${BLUE}=== Step 6: Register Routes on Skip Adapter ===${NC}"
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
            echo -e "${YELLOW}Skipping route registration.${NC}"
        fi
    fi
    echo ""
fi

# ============================================================================
# Summary
# ============================================================================

echo -e "${GREEN}=== Deployment Summary ===${NC}"
echo ""
echo "Chain:   $CHAIN_ID  ($NODE)"
echo ""
echo "Contract Addresses:"
[ "$DEPLOY_CCTP" = "true" ]    && echo "  CCTP Adapter:    $CCTP_ADDRESS"
[ "$DEPLOY_SKIP" = "true" ]    && echo "  Skip Adapter:    $SKIP_ADDRESS"
[ "$DEPLOY_RESERVE" = "true" ] && echo "  Reserve Adapter: $RESERVE_ADDRESS"
echo ""
echo "Code IDs:"
[ "$DEPLOY_CCTP" = "true" ]    && echo "  CCTP Adapter:    $CCTP_CODE_ID"
[ "$DEPLOY_SKIP" = "true" ]    && echo "  Skip Adapter:    $SKIP_CODE_ID"
[ "$DEPLOY_RESERVE" = "true" ] && echo "  Reserve Adapter: $RESERVE_CODE_ID"
echo ""
echo "Vaults served:"
jq -r '.vaults[] | "  \(.name): \(.adapters | join(", "))"' "$CONFIG_FILE"
echo ""
echo "State saved to: $CONFIG_FILE"
echo ""
echo -e "${GREEN}Done!${NC}"
