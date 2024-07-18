#!/bin/bash
set -eux

WASM_HOME="$HOME"/.wasmd
CHAIN_ID="wasm"
SEED_PHRASE_1="direct island hammer gentle cook hollow obvious promote bracket gravity file alcohol rule frost base hint smart foot soup time purity margin lend pencil"
SEED_PHRASE_2="long physical balcony pool increase detail fire light veteran skull blade physical skirt neglect width matrix dish snake soap amount bottom wash bean life"
WALLET_1_ADDR="wasm1e35997edcs7rc28sttwd436u0e83jw6cpfcyq7"
WALLET_2_ADDR="wasm1dxxqgmzkq6qd2q85mkh320k0g6lhueje29v609"
ATOM_WARS_WASM="./target/wasm32-unknown-unknown/release/atom_wars.wasm"
TRIBUTE_WASM="./target/wasm32-unknown-unknown/release/tribute.wasm"
ATOM_WARS_CONTRACT_ADDR="wasm14hj2tavq8fpesdwxxcu44rty3hh90vhujrvcmstl4zr3txmfvw9s0phg4d"
TRIBUTE_CONTRACT_ADDR="wasm1nc5tatafv6eyq7llkr2gv50ff9e22mnf70qgjlv737ktmt4eswrqr5j2ht"
CHAIN_ID_FLAG="--chain-id $CHAIN_ID"
KEYRING_TEST_FLAG="--keyring-backend test"
TX_FLAG='--gas auto --gas-adjustment 1.3'

# Test Scenario:
# 1. Create 2 proposals
# 2. Have 2 users lock tokens
# 3. Add tributes for both proposals
# 4. Have users vote for different proposals
# 5. (Commented-out, must wait for round end) Have users claim their tributes

# Step 1
EXECUTE='{"create_proposal":{"tranche_id":1,"title":"Proposal 1","description":"Proposal 1 description","covenant_params":{"pool_id":"pool 1","outgoing_channel_id":"channel-1","funding_destination_name":"Osmosis"}}}'
wasmd tx wasm execute $ATOM_WARS_CONTRACT_ADDR "$EXECUTE" --from wallet1 $TX_FLAG $KEYRING_TEST_FLAG $CHAIN_ID_FLAG -y
sleep 5

EXECUTE='{"create_proposal":{"tranche_id":1,"title":"Proposal 2 description","description":"Proposal 2 description","covenant_params":{"pool_id":"pool 2","outgoing_channel_id":"channel-2","funding_destination_name":"Astroport"}}}'
wasmd tx wasm execute $ATOM_WARS_CONTRACT_ADDR "$EXECUTE" --from wallet2 $TX_FLAG $KEYRING_TEST_FLAG $CHAIN_ID_FLAG -y
sleep 5

# Step 2
EXECUTE='{"lock_tokens":{"lock_duration":3600000000000}}'
wasmd tx wasm execute $ATOM_WARS_CONTRACT_ADDR "$EXECUTE" --amount 7000stake --from wallet1 $TX_FLAG $KEYRING_TEST_FLAG $CHAIN_ID_FLAG -y
wasmd tx wasm execute $ATOM_WARS_CONTRACT_ADDR "$EXECUTE" --amount 1000stake --from wallet2 $TX_FLAG $KEYRING_TEST_FLAG $CHAIN_ID_FLAG -y
sleep 5

# Step 3
EXECUTE='{"add_tribute":{"tranche_id":1,"proposal_id":0}}'
wasmd tx wasm execute $TRIBUTE_CONTRACT_ADDR "$EXECUTE" --amount 100000stake --from wallet1 $TX_FLAG $KEYRING_TEST_FLAG $CHAIN_ID_FLAG -y
sleep 5
EXECUTE='{"add_tribute":{"tranche_id":1,"proposal_id":1}}'
wasmd tx wasm execute $TRIBUTE_CONTRACT_ADDR "$EXECUTE" --amount 30000stake --from wallet1 $TX_FLAG $KEYRING_TEST_FLAG $CHAIN_ID_FLAG -y
sleep 5

# Step 4
EXECUTE='{"vote":{"tranche_id":1,"proposal_id":0}}'
wasmd tx wasm execute $ATOM_WARS_CONTRACT_ADDR "$EXECUTE" --from wallet1 $TX_FLAG $KEYRING_TEST_FLAG $CHAIN_ID_FLAG -y
EXECUTE='{"vote":{"tranche_id":1,"proposal_id":1}}'
wasmd tx wasm execute $ATOM_WARS_CONTRACT_ADDR "$EXECUTE" --from wallet2 $TX_FLAG $KEYRING_TEST_FLAG $CHAIN_ID_FLAG -y
sleep 5

# EXECUTE='{"claim_tribute":{"round_id":0,"tranche_id":1,"tribute_id":0,"voter_address":"'$WALLET_1_ADDR'"}}'
# wasmd tx wasm execute $TRIBUTE_CONTRACT_ADDR "$EXECUTE" --from wallet1 $TX_FLAG $KEYRING_TEST_FLAG $CHAIN_ID_FLAG -y

# EXECUTE='{"refund_tribute":{"round_id":0, "tranche_id":1,"tribute_id":1,"voter_address":"'$WALLET_2_ADDR'"}}'
# wasmd tx wasm execute $TRIBUTE_CONTRACT_ADDR "$EXECUTE" --from wallet1 $TX_FLAG $KEYRING_TEST_FLAG $CHAIN_ID_FLAG -y
