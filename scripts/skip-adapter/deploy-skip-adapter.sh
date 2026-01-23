#!/bin/bash

# Deployment script for Skip Adapter and IBC Adapter
# This script automates the deployment and configuration of Skip Adapter and IBC Adapter contracts

set -e

# Change to script directory to ensure all relative paths work correctly
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
cd "$SCRIPT_DIR"

# Color codes for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Default configuration
DEFAULT_CONFIG_FILE="deploy-config.json"

# Parse command-line arguments
CLI_DEPLOYER_WALLET=""
CLI_ADMIN_ADDRESS=""
CLI_KEYRING_BACKEND="test"  # Default value
CONFIG_FILE="$DEFAULT_CONFIG_FILE"

# Check if first argument is a config file (doesn't start with --)
if [[ $# -gt 0 && "$1" != --* ]]; then
    CONFIG_FILE="$1"
    shift
fi

# Parse remaining arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --deployer-wallet)
            CLI_DEPLOYER_WALLET="$2"
            shift 2
            ;;
        --admin-address)
            CLI_ADMIN_ADDRESS="$2"
            shift 2
            ;;
        --keyring-backend)
            CLI_KEYRING_BACKEND="$2"
            shift 2
            ;;
        *)
            echo "Unknown option: $1"
            echo "Usage: $0 [config-file.json] [--deployer-wallet <wallet>] [--admin-address <address>] [--keyring-backend <backend>]"
            exit 1
            ;;
    esac
done

# Paths to WASM files
IBC_ADAPTER_WASM="../../artifacts/ibc_adapter.wasm"
SKIP_ADAPTER_WASM="../../artifacts/skip_adapter.wasm"

echo -e "${GREEN}=== Skip Adapter & IBC Adapter Deployment Script ===${NC}"
echo ""

# Validate config file exists
if [ ! -f "$CONFIG_FILE" ]; then
    echo -e "${RED}Error: Config file '$CONFIG_FILE' not found${NC}"
    echo "Please create a config file or provide a path to an existing one"
    exit 1
fi

# Load configuration
NEUTRON_CHAIN_ID=$(jq -r '.chain_id' $CONFIG_FILE)
NEUTRON_NODE=$(jq -r '.neutron_rpc_node' $CONFIG_FILE)
GAS_PRICE=$(jq -r '.gas_price' $CONFIG_FILE)
GAS_ADJUSTMENT=$(jq -r '.gas_adjustment' $CONFIG_FILE)
DEPLOYER_WALLET=$(jq -r '.deployer_wallet' $CONFIG_FILE)
ADMIN_ADDRESS=$(jq -r '.admin_address' $CONFIG_FILE)
KEYRING_BACKEND=$(jq -r '.keyring_backend' $CONFIG_FILE)
NEUTRON_BINARY=$(jq -r '.neutron_binary' $CONFIG_FILE)
NEUTRON_DIR=$(jq -r '.neutron_dir' $CONFIG_FILE)

# Load skip adapter configuration
DEFAULT_TIMEOUT_NANOS=$(jq -r '.skip_adapter.default_timeout_nanos' $CONFIG_FILE)
MAX_SLIPPAGE_BPS=$(jq -r '.skip_adapter.max_slippage_bps' $CONFIG_FILE)
# Skip contracts are loaded as JSON object from config
SKIP_CONTRACTS_JSON=$(jq -c '.skip_adapter.skip_contracts' $CONFIG_FILE)

# Load IBC adapter configuration
IBC_DEFAULT_TIMEOUT=$(jq -r '.ibc_adapter.default_timeout_seconds' $CONFIG_FILE)
# Chains array is used directly in instantiation

# Load existing code IDs
IBC_ADAPTER_CODE_ID=$(jq -r '.code_ids.ibc_adapter // 0' $CONFIG_FILE)
SKIP_ADAPTER_CODE_ID=$(jq -r '.code_ids.skip_adapter // 0' $CONFIG_FILE)

# Load existing contract addresses
IBC_ADAPTER_ADDRESS=$(jq -r '.contracts.ibc_adapter // ""' $CONFIG_FILE)
SKIP_ADAPTER_ADDRESS=$(jq -r '.contracts.skip_adapter // ""' $CONFIG_FILE)

# Setup CLI flags
NEUTRON_CHAIN_ID_FLAG="--chain-id $NEUTRON_CHAIN_ID"
KEYRING_TEST_FLAG="--keyring-backend $KEYRING_BACKEND"
TX_FLAG="--gas auto --gas-adjustment $GAS_ADJUSTMENT"
NEUTRON_NODE_FLAG="--node $NEUTRON_NODE"
NEUTRON_TX_FLAGS="$TX_FLAG --gas-prices $GAS_PRICE --chain-id $NEUTRON_CHAIN_ID $NEUTRON_NODE_FLAG $KEYRING_TEST_FLAG -y"
NEUTRON_CLI="$NEUTRON_BINARY --home $NEUTRON_DIR"

# Derive deployer address from wallet
DEPLOYER_ADDRESS=$($NEUTRON_CLI keys show $DEPLOYER_WALLET $KEYRING_TEST_FLAG --output json | jq -r .address)

echo "Configuration:"
echo "  Config File: $CONFIG_FILE"
echo "  Deployer Wallet: $DEPLOYER_WALLET"
echo "  Deployer Address: $DEPLOYER_ADDRESS"
echo "  Admin Address: $ADMIN_ADDRESS"
echo "  Node: $NEUTRON_NODE"
echo "  Chain ID: $NEUTRON_CHAIN_ID"
echo "  Gas Price: $GAS_PRICE"
echo ""

# ============================================================================
# Helper Functions
# ============================================================================

# Retry command function
retry_command() {
    set +e
    local output
    local status
    local max_attempts=${2:-0} # Optional second parameter for max attempts (0 = infinite)
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

# Function to update config file
update_config() {
    local key=$1
    local value=$2
    local tmp_file=$(mktemp)
    jq "$key = \"$value\"" "$CONFIG_FILE" > "$tmp_file" && mv "$tmp_file" "$CONFIG_FILE"
}

# Function to update config file with number
update_config_number() {
    local key=$1
    local value=$2
    local tmp_file=$(mktemp)
    jq "$key = $value" "$CONFIG_FILE" > "$tmp_file" && mv "$tmp_file" "$CONFIG_FILE"
}

# ============================================================================
# Contract Upload Functions
# ============================================================================

# Function to store (upload) a contract
store_contract() {
    local contract_name=$1
    local wasm_path=$2
    local config_key=$3

    printf "Storing $contract_name wasm" >&2

    $NEUTRON_CLI tx wasm store "$wasm_path" --from "$DEPLOYER_WALLET" $NEUTRON_TX_FLAGS --output json &> "./store_${contract_name}_res.json"
    TX_HASH=$(grep -o '{.*}' "./store_${contract_name}_res.json" | jq -r '.txhash')
    TX_RESULT=$(retry_command "$NEUTRON_CLI q tx $TX_HASH $NEUTRON_NODE_FLAG --output json" 60)
    CODE_ID=$(echo "$TX_RESULT" | jq -r '.events[] | select(.type == "store_code") | .attributes[] | select(.key == "code_id") | .value')

    echo "$contract_name contract stored with code ID: $CODE_ID" >&2

    # Update config with code ID
    update_config_number "$config_key" "$CODE_ID"

    echo "$CODE_ID"
}

# ============================================================================
# Contract Instantiation Functions
# ============================================================================

# Function to instantiate IBC Adapter
instantiate_ibc_adapter() {
    echo -e "${BLUE}Instantiating IBC Adapter...${NC}"

    # Build initial_chains array from config
    INITIAL_CHAINS=$(jq -c '[.ibc_adapter.chains[] | {chain_id: .chain_id, channel_from_neutron: .channel_from_neutron, allowed_recipients: []}]' $CONFIG_FILE)

    # Build initial_tokens array from config
    INITIAL_TOKENS=$(jq -c '.ibc_adapter.tokens // []' $CONFIG_FILE)

    # Build instantiate message
    INIT_MSG=$(jq -n \
        --arg admin "$ADMIN_ADDRESS" \
        --argjson timeout "$IBC_DEFAULT_TIMEOUT" \
        --argjson chains "$INITIAL_CHAINS" \
        --argjson tokens "$INITIAL_TOKENS" \
        '{
            admins: [$admin],
            initial_depositors: [{address: $admin, capabilities: null}],
            default_timeout_seconds: $timeout,
            initial_chains: $chains,
            initial_tokens: $tokens,
            initial_executors: []
        }'
    )

    echo "Instantiate message:"
    echo "$INIT_MSG" | jq .
    echo ""

    printf "Instantiating IBC Adapter"
    $NEUTRON_CLI tx wasm instantiate "$IBC_ADAPTER_CODE_ID" "$INIT_MSG" \
        --from "$DEPLOYER_WALLET" \
        --label "Inflow IBC Adapter" \
        --admin "$ADMIN_ADDRESS" \
        $NEUTRON_TX_FLAGS \
        --output json &> ./instantiate_ibc_adapter_res.json

    TX_HASH=$(grep -o '{.*}' ./instantiate_ibc_adapter_res.json | jq -r '.txhash')
    TX_RESULT=$(retry_command "$NEUTRON_CLI q tx $TX_HASH $NEUTRON_NODE_FLAG --output json" 60)
    IBC_ADAPTER_ADDRESS=$(echo "$TX_RESULT" | jq -r '.events[] | select(.type == "instantiate") | .attributes[] | select(.key == "_contract_address") | .value')

    echo -e "${GREEN}IBC Adapter instantiated at: $IBC_ADAPTER_ADDRESS${NC}"

    # Update config
    update_config ".contracts.ibc_adapter" "$IBC_ADAPTER_ADDRESS"
}

# Function to instantiate Skip Adapter
instantiate_skip_adapter() {
    echo -e "${BLUE}Instantiating Skip Adapter...${NC}"

    # Check if we have required config values
    if [ -z "$SKIP_CONTRACTS_JSON" ] || [ "$SKIP_CONTRACTS_JSON" == "null" ]; then
        echo -e "${YELLOW}Warning: skip_contracts is not configured${NC}"
        SKIP_CONTRACTS_JSON="{}"
    fi

    # Build instantiate message using jq for proper JSON formatting
    INIT_MSG=$(jq -n \
        --arg admin "$ADMIN_ADDRESS" \
        --arg ibc_adapter "$IBC_ADAPTER_ADDRESS" \
        --argjson timeout "$DEFAULT_TIMEOUT_NANOS" \
        --argjson slippage "$MAX_SLIPPAGE_BPS" \
        --argjson skip_contracts "$SKIP_CONTRACTS_JSON" \
        '{
            admins: [$admin],
            skip_contracts: $skip_contracts,
            ibc_adapter: $ibc_adapter,
            default_timeout_nanos: $timeout,
            max_slippage_bps: $slippage,
            executors: [],
            initial_routes: [],
            initial_depositors: [$admin]
        }'
    )

    echo "Instantiate message:"
    echo "$INIT_MSG" | jq .
    echo ""

    printf "Instantiating Skip Adapter"
    $NEUTRON_CLI tx wasm instantiate "$SKIP_ADAPTER_CODE_ID" "$INIT_MSG" \
        --from "$DEPLOYER_WALLET" \
        --label "Inflow Skip Swap Adapter" \
        --admin "$ADMIN_ADDRESS" \
        $NEUTRON_TX_FLAGS \
        --output json &> ./instantiate_skip_adapter_res.json

    TX_HASH=$(grep -o '{.*}' ./instantiate_skip_adapter_res.json | jq -r '.txhash')
    TX_RESULT=$(retry_command "$NEUTRON_CLI q tx $TX_HASH $NEUTRON_NODE_FLAG --output json" 60)
    SKIP_ADAPTER_ADDRESS=$(echo "$TX_RESULT" | jq -r '.events[] | select(.type == "instantiate") | .attributes[] | select(.key == "_contract_address") | .value')

    echo -e "${GREEN}Skip Adapter instantiated at: $SKIP_ADAPTER_ADDRESS${NC}"

    # Update config
    update_config ".contracts.skip_adapter" "$SKIP_ADAPTER_ADDRESS"
}

# ============================================================================
# Post-Instantiation Configuration Functions
# ============================================================================

# Function to register skip-adapter as executor on ibc-adapter
register_skip_adapter_as_executor() {
    echo -e "${BLUE}Registering Skip Adapter as executor on IBC Adapter...${NC}"

    EXEC_MSG=$(cat <<EOF
{
  "custom_action": {
    "add_executor": {
      "executor_address": "$SKIP_ADAPTER_ADDRESS",
      "capabilities": {
        "can_set_memo": true
      }
    }
  }
}
EOF
)

    echo "Execute message:"
    echo "$EXEC_MSG" | jq .
    echo ""

    printf "Registering executor"
    $NEUTRON_CLI tx wasm execute "$IBC_ADAPTER_ADDRESS" "$EXEC_MSG" \
        --from "$DEPLOYER_WALLET" \
        $NEUTRON_TX_FLAGS \
        --output json &> ./register_executor_res.json

    TX_HASH=$(grep -o '{.*}' ./register_executor_res.json | jq -r '.txhash')
    retry_command "$NEUTRON_CLI q tx $TX_HASH $NEUTRON_NODE_FLAG --output json" 60 > /dev/null

    echo -e "${GREEN}Skip Adapter registered as executor${NC}"
}

# Function to register a route on skip-adapter
register_route() {
    local route_key=$1  # e.g., "osmosis_atom_statom"

    echo -e "${BLUE}Registering route: $route_key${NC}"

    # Extract route config from JSON
    local route_config=$(jq -r ".routes.$route_key" $CONFIG_FILE)
    local route_id=$(echo "$route_config" | jq -r '.route_id')
    local venue=$(echo "$route_config" | jq -r '.venue')
    local denom_in=$(echo "$route_config" | jq -r '.denom_in')
    local denom_out=$(echo "$route_config" | jq -r '.denom_out')
    local swap_venue_name=$(echo "$route_config" | jq -r '.swap_venue_name')
    local recover_address=$(echo "$route_config" | jq -r '.recover_address // ""')

    # Check for required fields
    if [ -z "$denom_in" ] || [ "$denom_in" == "" ]; then
        echo -e "${YELLOW}Skipping $route_key: denom_in is not configured${NC}"
        return
    fi

    # Build operations array (strip comments from path hops)
    local operations=$(echo "$route_config" | jq -c '.operations')

    # Build forward_path (strip comments)
    local forward_path=$(echo "$route_config" | jq -c '[.forward_path[] | {chain_id, channel, receiver}]')

    # Build return_path (strip comments and auto-fill empty receivers with skip-adapter address)
    local return_path=$(echo "$route_config" | jq -c \
        --arg skip_adapter "$SKIP_ADAPTER_ADDRESS" \
        '[.return_path[] | {chain_id, channel, receiver: (if .receiver == "" then $skip_adapter else .receiver end)}]')

    # Build the UnifiedRoute object
    local route_obj=$(jq -n \
        --arg venue "$venue" \
        --arg denom_in "$denom_in" \
        --arg denom_out "$denom_out" \
        --argjson operations "$operations" \
        --arg swap_venue_name "$swap_venue_name" \
        --argjson forward_path "$forward_path" \
        --argjson return_path "$return_path" \
        --arg recover_address "$recover_address" \
        '{
            venue: $venue,
            denom_in: $denom_in,
            denom_out: $denom_out,
            operations: $operations,
            swap_venue_name: $swap_venue_name,
            forward_path: $forward_path,
            return_path: $return_path,
            recover_address: (if $recover_address == "" then null else $recover_address end),
            enabled: true
        }'
    )

    # Build execute message
    local EXEC_MSG=$(jq -n \
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

    echo "Execute message:"
    echo "$EXEC_MSG" | jq .
    echo ""

    printf "Registering route $route_id"
    $NEUTRON_CLI tx wasm execute "$SKIP_ADAPTER_ADDRESS" "$EXEC_MSG" \
        --from "$DEPLOYER_WALLET" \
        $NEUTRON_TX_FLAGS \
        --output json &> "./register_route_${route_key}_res.json"

    TX_HASH=$(grep -o '{.*}' "./register_route_${route_key}_res.json" | jq -r '.txhash')
    retry_command "$NEUTRON_CLI q tx $TX_HASH $NEUTRON_NODE_FLAG --output json" 60 > /dev/null

    echo -e "${GREEN}Route $route_id registered${NC}"
}

# ============================================================================
# Main Execution Flow
# ============================================================================

main() {
    echo -e "${GREEN}Starting deployment process...${NC}"
    echo ""

    # Step 1: Store contracts
    echo -e "${BLUE}=== Step 1: Uploading Contract Code ===${NC}"

    if [ "$IBC_ADAPTER_CODE_ID" == "0" ] || [ "$IBC_ADAPTER_CODE_ID" == "null" ]; then
        IBC_ADAPTER_CODE_ID=$(store_contract "ibc_adapter" "$IBC_ADAPTER_WASM" ".code_ids.ibc_adapter")
    else
        echo "IBC Adapter already uploaded with code ID: $IBC_ADAPTER_CODE_ID"
        read -p "Upload new version? (y/n): " -n 1 -r
        echo
        if [[ $REPLY =~ ^[Yy]$ ]]; then
            IBC_ADAPTER_CODE_ID=$(store_contract "ibc_adapter" "$IBC_ADAPTER_WASM" ".code_ids.ibc_adapter")
        fi
    fi

    if [ "$SKIP_ADAPTER_CODE_ID" == "0" ] || [ "$SKIP_ADAPTER_CODE_ID" == "null" ]; then
        SKIP_ADAPTER_CODE_ID=$(store_contract "skip_adapter" "$SKIP_ADAPTER_WASM" ".code_ids.skip_adapter")
    else
        echo "Skip Adapter already uploaded with code ID: $SKIP_ADAPTER_CODE_ID"
        read -p "Upload new version? (y/n): " -n 1 -r
        echo
        if [[ $REPLY =~ ^[Yy]$ ]]; then
            SKIP_ADAPTER_CODE_ID=$(store_contract "skip_adapter" "$SKIP_ADAPTER_WASM" ".code_ids.skip_adapter")
        fi
    fi
    echo ""

    # Step 2: Instantiate IBC Adapter
    echo -e "${BLUE}=== Step 2: Instantiating IBC Adapter ===${NC}"
    if [ -z "$IBC_ADAPTER_ADDRESS" ] || [ "$IBC_ADAPTER_ADDRESS" == "null" ] || [ "$IBC_ADAPTER_ADDRESS" == "" ]; then
        instantiate_ibc_adapter
    else
        echo "IBC Adapter already instantiated at: $IBC_ADAPTER_ADDRESS"
        read -p "Instantiate new instance? (y/n): " -n 1 -r
        echo
        if [[ $REPLY =~ ^[Yy]$ ]]; then
            instantiate_ibc_adapter
        fi
    fi
    echo ""

    # Step 3: Instantiate Skip Adapter
    echo -e "${BLUE}=== Step 3: Instantiating Skip Adapter ===${NC}"
    if [ -z "$SKIP_ADAPTER_ADDRESS" ] || [ "$SKIP_ADAPTER_ADDRESS" == "null" ] || [ "$SKIP_ADAPTER_ADDRESS" == "" ]; then
        instantiate_skip_adapter
    else
        echo "Skip Adapter already instantiated at: $SKIP_ADAPTER_ADDRESS"
        read -p "Instantiate new instance? (y/n): " -n 1 -r
        echo
        if [[ $REPLY =~ ^[Yy]$ ]]; then
            instantiate_skip_adapter
        fi
    fi
    echo ""

    # Step 4: Register skip-adapter as executor
    echo -e "${BLUE}=== Step 4: Configuring IBC Adapter ===${NC}"
    read -p "Register Skip Adapter as executor on IBC Adapter? (y/n): " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        register_skip_adapter_as_executor
    else
        echo -e "${YELLOW}Skipping executor registration.${NC}"
    fi
    echo ""

    # Step 5: Route Registration
    echo -e "${BLUE}=== Step 5: Route Registration ===${NC}"
    echo ""
    echo "Routes to register:"
    echo "  1. osmosis-atom-statom: ATOM → stATOM via Osmosis (Neutron → Cosmos Hub → Osmosis, return via Stride)"
    echo "  2. osmosis-statom-atom: stATOM → ATOM via Osmosis (Neutron → Stride → Osmosis, return via Cosmos Hub)"
    echo "  3. osmosis-atom-datom:  ATOM → dATOM via Osmosis (Neutron → Cosmos Hub → Osmosis, direct return)"
    echo "  4. neutron-atom-datom:  ATOM → dATOM on Neutron (local swap)"
    echo ""

    read -p "Register routes now? (y/n): " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        # Get list of route keys from config
        ROUTE_KEYS=$(jq -r '.routes | keys[]' $CONFIG_FILE)

        for route_key in $ROUTE_KEYS; do
            register_route "$route_key"
            echo ""
        done
    else
        echo -e "${YELLOW}Skipping route registration. You can register routes manually later.${NC}"
        echo ""
        echo "To register a route manually, execute:"
        echo '  neutrond tx wasm execute $SKIP_ADAPTER_ADDRESS '"'"'{"custom_action":{"register_route":{...}}}'"'"' ...'
    fi
    echo ""

    # Display summary
    echo -e "${GREEN}=== Deployment Summary ===${NC}"
    echo ""
    echo "Contract Addresses:"
    echo "  IBC Adapter:  $IBC_ADAPTER_ADDRESS"
    echo "  Skip Adapter: $SKIP_ADAPTER_ADDRESS"
    echo ""
    echo "Code IDs:"
    echo "  IBC Adapter:  $IBC_ADAPTER_CODE_ID"
    echo "  Skip Adapter: $SKIP_ADAPTER_CODE_ID"
    echo ""
    echo "Configuration:"
    echo "  Admin:        $ADMIN_ADDRESS"
    echo "  Deployer:     $DEPLOYER_ADDRESS"
    echo ""
    echo -e "${GREEN}Deployment complete!${NC}"
    echo ""
    echo "Next steps:"
    echo "1. Fill in missing values in $CONFIG_FILE (denoms, operations, receiver addresses)"
    echo "2. If routes were not registered, run the script again or register manually"
    echo "3. Test by depositing ATOM and executing swaps on each route"
}

# Run main function
main

# ============================================================================
# TESTING & VERIFICATION QUERIES
# ============================================================================
#
# Use these JSON queries to verify deployment and test the contract.
# Replace contract addresses and amounts as needed.
#
# SKIP_ADAPTER=neutron1rq9w6v4sdc8e4nusycata77ad7zl3acxxna9jrw7wunezq9ltppqx4mfy9
# IBC_ADAPTER=neutron1q9ma567k5z0wezwtwq69a385dnzug7qe3n3nr2gsma7es8rrjvmsem7lvr
# ADMIN=neutron134xnhryf2v2qkvp9ahnxtgqmq83hc868yeh0sl
# ATOM_DENOM=ibc/C4CFF46FD6DE35CA4CF4CE031E643C8FDC9BA4B99AE598E9B0ED98FE3A2319F9
# DATOM_DENOM=factory/neutron1k6hr0f83e7un2wjf29cspk7j69jrnskk65k3ek2nj9dztrlzpj6q00rtsa/udatom
# STATOM_DENOM=ibc/B7864B03E1B9FD4F049243E92ABD691586F682137037A9F3FCA5222815620B3C
#
# --- SKIP ADAPTER VERIFICATION QUERIES ---
#
# 1. Check config (addresses, timeouts, slippage):
# neutrond query wasm contract-state smart $SKIP_ADAPTER '{"standard_query":{"config":{}}}'
#
# 2. Check executors:
# neutrond query wasm contract-state smart $SKIP_ADAPTER '{"custom_query":{"executors":{}}}'
#
# 3. Check all registered depositors:
# neutrond query wasm contract-state smart $SKIP_ADAPTER '{"standard_query":{"registered_depositors":{}}}'
#
# 4. Check enabled depositors only:
# neutrond query wasm contract-state smart $SKIP_ADAPTER '{"standard_query":{"registered_depositors":{"enabled":true}}}'
#
# 5. Check all routes:
# neutrond query wasm contract-state smart $SKIP_ADAPTER '{"custom_query":{"all_routes":{}}}'
#
# 6. Check local Neutron routes only:
# neutrond query wasm contract-state smart $SKIP_ADAPTER '{"custom_query":{"all_routes":{"venue":"neutron_astroport"}}}'
#
# 7. Check cross-chain Osmosis routes only:
# neutrond query wasm contract-state smart $SKIP_ADAPTER '{"custom_query":{"all_routes":{"venue":"osmosis"}}}'
#
# 8. Check specific route (neutron-atom-datom):
# neutrond query wasm contract-state smart $SKIP_ADAPTER '{"custom_query":{"route":{"route_id":"neutron-atom-datom"}}}'
#
# 9. Check available for deposit:
# neutrond query wasm contract-state smart $SKIP_ADAPTER '{"standard_query":{"available_for_deposit":{"depositor_address":"'$ADMIN'","denom":"'$ATOM_DENOM'"}}}'
#
# 10. Check available for withdraw:
# neutrond query wasm contract-state smart $SKIP_ADAPTER '{"standard_query":{"available_for_withdraw":{"depositor_address":"'$ADMIN'","denom":"'$ATOM_DENOM'"}}}'
#
# 11. Check contract ATOM balance:
# neutrond query bank balances $SKIP_ADAPTER --denom $ATOM_DENOM
#
# 12. Check contract dATOM balance:
# neutrond query bank balances $SKIP_ADAPTER --denom $DATOM_DENOM
#
# --- IBC ADAPTER VERIFICATION QUERIES ---
#
# 13. Check IBC adapter config:
# neutrond query wasm contract-state smart $IBC_ADAPTER '{"standard_query":{"config":{}}}'
#
# 14. Check IBC adapter executors:
# neutrond query wasm contract-state smart $IBC_ADAPTER '{"custom_query":{"executors":{}}}'
#
# 15. Check IBC adapter registered depositors:
# neutrond query wasm contract-state smart $IBC_ADAPTER '{"standard_query":{"registered_depositors":{}}}'
#
# 16. Check IBC adapter enabled depositors only:
# neutrond query wasm contract-state smart $IBC_ADAPTER '{"standard_query":{"registered_depositors":{"enabled":true}}}'
#
# 17. Check IBC adapter chains:
# neutrond query wasm contract-state smart $IBC_ADAPTER '{"custom_query":{"all_chains":{}}}'
#
# 18. Check specific chain config (Osmosis):
# neutrond query wasm contract-state smart $IBC_ADAPTER '{"custom_query":{"chain_config":{"chain_id":"osmosis-1"}}}'
#
# 19. Check IBC adapter tokens:
# neutrond query wasm contract-state smart $IBC_ADAPTER '{"custom_query":{"all_tokens":{}}}'
#
# --- EXECUTE MESSAGES ---
#
# 20. Deposit 1 ATOM (1000000 uatom):
# neutrond tx wasm execute $SKIP_ADAPTER '{"standard_action":{"deposit":{}}}' --from $ADMIN --amount 1000000$ATOM_DENOM --gas auto --gas-adjustment 1.3 --gas-prices 0.0053untrn --chain-id neutron-1 --node https://rpc-lb.neutron.org/ --keyring-backend test -y
#
# 21. Execute local swap (ATOM -> dATOM on Neutron, 1 ATOM with 5% slippage):
# neutrond tx wasm execute $SKIP_ADAPTER '{"custom_action":{"execute_swap":{"params":{"route_id":"neutron-atom-datom","amount_in":"1000000","min_amount_out":"950000"}}}}' --from $ADMIN --gas auto --gas-adjustment 1.3 --gas-prices 0.0053untrn --chain-id neutron-1 --node https://rpc-lb.neutron.org/ --keyring-backend test -y
#
# 22. Withdraw dATOM from Skip Adapter (adjust amount based on swap output):
# neutrond tx wasm execute $SKIP_ADAPTER '{"standard_action":{"withdraw":{"coin":{"denom":"'$DATOM_DENOM'","amount":"950000"}}}}' --from $ADMIN --gas auto --gas-adjustment 1.3 --gas-prices 0.0053untrn --chain-id neutron-1 --node https://rpc-lb.neutron.org/ --keyring-backend test -y
#
# --- CROSS-CHAIN SWAP TEST (OSMOSIS) ---
#
# 23. Deposit 1 ATOM to IBC Adapter:
# neutrond tx wasm execute $IBC_ADAPTER '{"standard_action":{"deposit":{}}}' --from $ADMIN --amount 1000000$ATOM_DENOM --gas auto --gas-adjustment 1.3 --gas-prices 0.0053untrn --chain-id neutron-1 --node https://rpc-lb.neutron.org/ --keyring-backend test -y
#
# 24. Check IBC Adapter ATOM balance:
# neutrond query bank balance $IBC_ADAPTER $ATOM_DENOM
#
# 25. Execute cross-chain swap (ATOM -> stATOM via Osmosis, 1 ATOM with 5% slippage):
# neutrond tx wasm execute $SKIP_ADAPTER '{"custom_action":{"execute_swap":{"params":{"route_id":"osmosis-atom-statom","amount_in":"1000000","min_amount_out":"950000"}}}}' --from $ADMIN --gas auto --gas-adjustment 1.3 --gas-prices 0.0053untrn --chain-id neutron-1 --node https://rpc-lb.neutron.org/ --keyring-backend test -y
#
# 26. Wait for IBC transfers to complete (check periodically, may take 1-5 minutes):
# neutrond query bank balance $SKIP_ADAPTER $STATOM_DENOM
#
# 27. Withdraw stATOM from Skip Adapter (adjust amount based on swap output):
# neutrond tx wasm execute $SKIP_ADAPTER '{"standard_action":{"withdraw":{"coin":{"denom":"'$STATOM_DENOM'","amount":"950000"}}}}' --from $ADMIN --gas auto --gas-adjustment 1.3 --gas-prices 0.0053untrn --chain-id neutron-1 --node https://rpc-lb.neutron.org/ --keyring-backend test -y
#
# --- RECOMMENDED TESTING FLOW ---
#
# LOCAL SWAP TEST (Neutron):
# 1. Run queries 1-12 to verify Skip Adapter (config, executors, depositors, routes, balances)
# 2. Run queries 13-19 to verify IBC Adapter (config, executors, depositors, chains)
# 3. Deposit ATOM to Skip Adapter using command 20
# 4. Verify deposit with queries 9 and 11
# 5. Execute local swap (ATOM -> dATOM on Neutron) using command 21
# 6. Check dATOM received with query 12
# 7. Withdraw dATOM from Skip Adapter using command 22
#
# CROSS-CHAIN SWAP TEST (Osmosis):
# 1. Deposit ATOM to IBC Adapter using command 23
# 2. Verify deposit with command 24
# 3. Execute cross-chain swap (ATOM -> stATOM via Osmosis) using command 25
# 4. Wait for IBC transfers to complete (1-5 minutes), check with command 26
# 5. Withdraw stATOM from Skip Adapter using command 27
