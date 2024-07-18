#!/bin/bash
set -eux

WASM_HOME="$HOME"/.wasmd
CHAIN_ID="wasm"
SEED_PHRASE_1="direct island hammer gentle cook hollow obvious promote bracket gravity file alcohol rule frost base hint smart foot soup time purity margin lend pencil"
SEED_PHRASE_2="long physical balcony pool increase detail fire light veteran skull blade physical skirt neglect width matrix dish snake soap amount bottom wash bean life"
WALLET_1_ADDR="wasm1e35997edcs7rc28sttwd436u0e83jw6cpfcyq7"
WALLET_2_ADDR="wasm1dxxqgmzkq6qd2q85mkh320k0g6lhueje29v609"
HYDRO_WASM="./../target/wasm32-unknown-unknown/release/hydro.wasm"
HYDRO_WASM_V2="./../target/wasm32-unknown-unknown/release/hydro_v2.wasm"
TRIBUTE_WASM="./../target/wasm32-unknown-unknown/release/tribute.wasm"
HYDRO_CONTRACT_ADDR="wasm14hj2tavq8fpesdwxxcu44rty3hh90vhujrvcmstl4zr3txmfvw9s0phg4d"
TRIBUTE_CONTRACT_ADDR="wasm1nc5tatafv6eyq7llkr2gv50ff9e22mnf70qgjlv737ktmt4eswrqr5j2ht"
CHAIN_ID_FLAG="--chain-id $CHAIN_ID"
KEYRING_TEST_FLAG="--keyring-backend test"
TX_FLAG='--gas auto --gas-adjustment 1.3'

clean_up(){
    pkill "wasmd" &> /dev/null || true
    rm -rf "$HOME/.wasmd"
}

clean_up

wasmd init wasm $CHAIN_ID_FLAG
echo $SEED_PHRASE_1 | wasmd keys add wallet1 $KEYRING_TEST_FLAG --recover
echo $SEED_PHRASE_2 | wasmd keys add wallet2 $KEYRING_TEST_FLAG --recover
wasmd genesis add-genesis-account wallet1 1000000000000stake,1000000000000token $KEYRING_TEST_FLAG
wasmd genesis add-genesis-account wallet2 1000000000000stake,1000000000000token $KEYRING_TEST_FLAG
wasmd genesis gentx wallet1 1000000000stake $KEYRING_TEST_FLAG $CHAIN_ID_FLAG
wasmd genesis collect-gentxs
wasmd start &> "$WASM_HOME"/logs &

sleep 10

wasmd tx wasm store $HYDRO_WASM --from wallet1 $KEYRING_TEST_FLAG $CHAIN_ID_FLAG $TX_FLAG -y
sleep 5

wasmd tx wasm store $TRIBUTE_WASM --from wallet1 $KEYRING_TEST_FLAG $CHAIN_ID_FLAG $TX_FLAG -y
sleep 5

wasmd tx wasm store $HYDRO_WASM_V2 --from wallet1 $KEYRING_TEST_FLAG $CHAIN_ID_FLAG $TX_FLAG -y
sleep 5

wasmd q wasm codes

CURRENT_TIME=$(date +%s%N)
FIRST_ROUND_START_TIME=$((CURRENT_TIME + 10 * 1000000000))

# to check the first round start:
# seconds=$((FIRST_ROUND_START_TIME / 1000000000))
# nanoseconds=$((FIRST_ROUND_START_TIME % 1000000000))
# FORMATTED_TIME=$(date -d @$seconds +"%Y-%m-%d %H:%M:%S")
# echo $FORMATTED_TIME

INIT_HYDRO='{"denom":"stake","round_length":300000000000,"lock_epoch_length":300000000000,"tranches":[{"tranche_id": 1,"metadata": "first tranch"},{"tranche_id": 2, "metadata": "second tranch"}],"first_round_start": "'$FIRST_ROUND_START_TIME'","max_locked_tokens":"100000","whitelist_admins":["wasm1e35997edcs7rc28sttwd436u0e83jw6cpfcyq7"],"initial_whitelist":[{"pool_id":"pool 1","outgoing_channel_id":"channel-1","funding_destination_name":"Osmosis"}]}'
wasmd tx wasm instantiate 1 "$INIT_HYDRO" --from wallet1 --admin wallet1 --label "Hydro Instance 1" $TX_FLAG $KEYRING_TEST_FLAG $CHAIN_ID_FLAG -y
sleep 5

INIT_TRIBUTE='{"hydro_contract":"'$HYDRO_CONTRACT_ADDR'","top_n_props_count":10}'
wasmd tx wasm instantiate 2 "$INIT_TRIBUTE" --from wallet1 --admin wallet1 --label "Tribute Instance 1" $TX_FLAG $KEYRING_TEST_FLAG $CHAIN_ID_FLAG -y
sleep 5

#################################################################################### Hydro: ####################################################################################

# QUERY='{"constants": {}}'
# QUERY='{"current_round": {}}'
# QUERY='{"round_end": {"round_id":0}}'
# QUERY='{"round_total_voting_power": {"round_id": 0}}'
# QUERY='{"tranches": {}}'
# QUERY='{"all_user_lockups":{"address":"'$WALLET_1_ADDR'","start_from":0,"limit":100}}'
# QUERY='{"expired_user_lockups":{"address":"'$WALLET_1_ADDR'","start_from":0,"limit":100}}'
# QUERY='{"user_voting_power":{"address":"'$WALLET_1_ADDR'"}}'
# QUERY='{"user_vote":{"round_id":0,"tranche_id":1,"address":"'$WALLET_1_ADDR'"}}'
# QUERY='{"proposal": {"round_id":0,"tranche_id":1,"proposal_id":0}}'
# QUERY='{"round_proposals": {"round_id":0,"tranche_id":1,"start_from":0,"limit":100}}'
# QUERY='{"top_n_proposals": {"round_id":0,"tranche_id":1,"number_of_proposals":10}}'
# QUERY='{"whitelist": {}}'
# QUERY='{"whitelist_admins": {}}'
# wasmd q wasm contract-state smart $HYDRO_CONTRACT_ADDR "$QUERY"

# EXECUTE='{"lock_tokens":{"lock_duration":3600000000000}}'
# wasmd tx wasm execute $HYDRO_CONTRACT_ADDR "$EXECUTE" --amount 1000stake --from wallet1 $TX_FLAG $KEYRING_TEST_FLAG $CHAIN_ID_FLAG -y

# EXECUTE='{"create_proposal":{"tranche_id":1,"title":"Proposal 1","description":"Proposal 1 description","covenant_params":{"pool_id":"pool 1","outgoing_channel_id":"channel-1","funding_destination_name":"Osmosis"}}}'
# EXECUTE='{"unlock_tokens":{}}'
# EXECUTE='{"add_to_whitelist":{"covenant_params":{"pool_id":"pool 2","outgoing_channel_id":"channel-2","funding_destination_name":"Astroport"}}}'
# wasmd tx wasm execute $HYDRO_CONTRACT_ADDR "$EXECUTE" --from wallet1 $TX_FLAG $KEYRING_TEST_FLAG $CHAIN_ID_FLAG -y

# wasmd tx wasm migrate wasm14hj2tavq8fpesdwxxcu44rty3hh90vhujrvcmstl4zr3txmfvw9s0phg4d 3 '{}' --from wallet1 $TX_FLAG $KEYRING_TEST_FLAG $CHAIN_ID_FLAG -y

################################################################################## Tribute: ####################################################################################

# wasmd q wasm contract-state smart $TRIBUTE_CONTRACT_ADDR '{"config": {}}'
# wasmd q wasm contract-state smart $TRIBUTE_CONTRACT_ADDR '{"proposal_tributes": {"round_id":0,"tranche_id":1,"proposal_id":0}}'

# EXECUTE='{"add_tribute":{"tranche_id":1,"proposal_id":0}}'
# wasmd tx wasm execute $TRIBUTE_CONTRACT_ADDR "$EXECUTE" --amount 1000stake --from wallet1 $TX_FLAG $KEYRING_TEST_FLAG $CHAIN_ID_FLAG -y
