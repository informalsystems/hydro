#!/bin/bash

if [ "$#" -ne 1 ]; then
    echo "Usage: $0 DAO_DEPLOYMENT_CONFIG_PATH"
    exit 1
fi

NEUTRON_BINARY="neutrond"
CONFIG_FILE="tools/deployment/config_mainnet.json"

NEUTRON_CHAIN_ID=$(jq -r '.chain_id' $CONFIG_FILE)
NEUTRON_NODE=$(jq -r '.neutron_rpc_node' $CONFIG_FILE)
TX_SENDER_WALLET=$(jq -r '.tx_sender_wallet' $CONFIG_FILE)
TX_SENDER_ADDRESS=$($NEUTRON_BINARY keys show $TX_SENDER_WALLET --keyring-backend test | grep "address:" | sed 's/.*address: //')

NEUTRON_CHAIN_ID_FLAG="--chain-id $NEUTRON_CHAIN_ID"
KEYRING_TEST_FLAG="--keyring-backend test"
TX_FLAG="--gas auto --gas-adjustment 1.3"
NEUTRON_NODE_FLAG="--node $NEUTRON_NODE"
NEUTRON_TX_FLAGS="$TX_FLAG --gas-prices 0.0053untrn --chain-id $NEUTRON_CHAIN_ID $NEUTRON_NODE_FLAG $KEYRING_TEST_FLAG -y"

DAO_DEPLOYMENT_CONFIG_PATH="$1"
DAO_NAME=$(jq -r '.dao_name' $DAO_DEPLOYMENT_CONFIG_PATH)
DAO_DESCRIPTION=$(jq -r '.dao_description' $DAO_DEPLOYMENT_CONFIG_PATH)
DAO_VOTING_ADAPTER_CODE_ID=$(jq -r '.dao_voting_adapter_code_id' $DAO_DEPLOYMENT_CONFIG_PATH)
HYDRO_CONTRACT_ADDRESS=$(jq -r '.hydro_contract_address' $DAO_DEPLOYMENT_CONFIG_PATH)
PROPOSAL_SUBMISSION_APPROVER=$(jq -r '.proposal_submission_approver' $DAO_DEPLOYMENT_CONFIG_PATH)
ALLOWED_PROPOSAL_SUBMITTER=$(jq -r '.allowed_proposal_submitter' $DAO_DEPLOYMENT_CONFIG_PATH)
UUSDC_DEPOSIT_AMOUNT=$(jq -r '.uusdc_deposit_amount' $DAO_DEPLOYMENT_CONFIG_PATH)
MAX_VOTING_PERIOD=$(jq -r '.max_voting_period' $DAO_DEPLOYMENT_CONFIG_PATH)
QUORUM_PERCENT=$(jq -r '.quorum_percent' $DAO_DEPLOYMENT_CONFIG_PATH)

# https://github.com/DA0-DA0/dao-dao-ui/blob/development/packages/utils/constants/codeIds.json
DAO_CORE_CODE_ID="2346"
DAO_PROPOSAL_SINGLE_CODE_ID="2353"
DAO_PREPROPOSE_APPROVAL_SINGLE_CODE_ID="2348"

DAO_VOTING_ADAPTER_INIT_MSG='{"hydro_contract":"'$HYDRO_CONTRACT_ADDRESS'"}'
echo 'DAO Voting Adapter init msg:' $DAO_VOTING_ADAPTER_INIT_MSG
echo ""
DAO_VOTING_ADAPTER_INIT_MSG=$(echo -n $DAO_VOTING_ADAPTER_INIT_MSG | base64 | tr -d '[:space:]')

VOTING_MODULE_INSTANTIATE_INFO='{"code_id":'$DAO_VOTING_ADAPTER_CODE_ID',"funds":[],"label":"Hydro DAO Voting Adapter","msg":"'$DAO_VOTING_ADAPTER_INIT_MSG'"}'

if [ -z "$ALLOWED_PROPOSAL_SUBMITTER" ]; then
    SUBMISSION_POLICY='{"anyone": {"denylist": []}}'
else
    SUBMISSION_POLICY='{"specific": {"dao_members": false,"allowlist": ["'$ALLOWED_PROPOSAL_SUBMITTER'"],"denylist": []}}'
fi

if [ -z "$UUSDC_DEPOSIT_AMOUNT" ]; then
    DEPOSIT_INFO='null'
else
    DEPOSIT_INFO='{"denom": {"token": {"denom": {"native": "ibc/B559A80D62249C8AA07A380E2A2BEA6E5CA9A6F079C912C3A9E9B494105E4F81"}}}, "amount": "'$UUSDC_DEPOSIT_AMOUNT'", "refund_policy": "only_passed"}}'
fi

PRE_PROPOSE_APPROVAL_INIT_MSG='{"deposit_info": '$DEPOSIT_INFO',"submission_policy": '$SUBMISSION_POLICY', "extension": { "approver": "'$PROPOSAL_SUBMISSION_APPROVER'"}}'
echo "DAO Pre-propose approval init msg:" $PRE_PROPOSE_APPROVAL_INIT_MSG
echo ""
PRE_PROPOSE_APPROVAL_INIT_MSG=$(echo -n $PRE_PROPOSE_APPROVAL_INIT_MSG | base64 | tr -d '[:space:]')

PRE_PROPOSE_MODULE_INSTANTIATE_INFO='{"module_may_propose": {"info": {"admin": {"core_module": {}}, "code_id": '$DAO_PREPROPOSE_APPROVAL_SINGLE_CODE_ID', "label": "Hydro pre-propose-single with Approver", "msg": "'$PRE_PROPOSE_APPROVAL_INIT_MSG'", "funds": []}}}'
DAO_PROPOSAL_SINGLE_INIT_MSG='{"threshold": {"threshold_quorum": {"quorum": {"percent": "'$QUORUM_PERCENT'"}, "threshold": {"majority": {}}}}, "max_voting_period":{"time":'$MAX_VOTING_PERIOD'}, "only_members_execute":true, "allow_revoting":false, "close_proposal_on_execution_failure":true, "pre_propose_info":'$PRE_PROPOSE_MODULE_INSTANTIATE_INFO'}'
echo 'DAO Proposal Single init msg:' $DAO_PROPOSAL_SINGLE_INIT_MSG
echo ""
DAO_PROPOSAL_SINGLE_INIT_MSG=$(echo -n $DAO_PROPOSAL_SINGLE_INIT_MSG | base64 | tr -d '[:space:]')

PROPOSAL_MODULE_INSTANTIATE_INFO='{"code_id":'$DAO_PROPOSAL_SINGLE_CODE_ID', "funds":[], "label":"Hydro DAO proposal-single", "msg":"'$DAO_PROPOSAL_SINGLE_INIT_MSG'"}'

INIT_DAODAO='{"name":"'$DAO_NAME'", "description":"'$DAO_DESCRIPTION'", "automatically_add_cw20s":false, "automatically_add_cw721s":false, "voting_module_instantiate_info":'$VOTING_MODULE_INSTANTIATE_INFO',"proposal_modules_instantiate_info":['$PROPOSAL_MODULE_INSTANTIATE_INFO']}'
echo 'DAO Core init msg:' $INIT_DAODAO
echo ""

echo 'Instantiating Hydro DAO...'
$NEUTRON_BINARY tx wasm instantiate $DAO_CORE_CODE_ID "$INIT_DAODAO" --admin $TX_SENDER_ADDRESS --label "'$DAO_NAME'" --from $TX_SENDER_WALLET $NEUTRON_TX_FLAGS --output json &> ./instantiate_hydro_dao_res.json
sleep 10

INSTANTIATE_HYDRO_DAO_TX_HASH=$(grep -o '{.*}' ./instantiate_hydro_dao_res.json | jq -r '.txhash')
$NEUTRON_BINARY q tx $INSTANTIATE_HYDRO_DAO_TX_HASH $NEUTRON_NODE_FLAG --output json &> ./instantiate_hydro_dao_tx.json
HYDRO_DAO_CONTRACT_ADDRESS=$(jq -r '[.events[] | select(.type == "instantiate") | .attributes[] | select(.key == "_contract_address") | .value] | .[0]' ./instantiate_hydro_dao_tx.json)

echo 'Hydro DAO successfully instantiated: https://daodao.zone/dao/'$HYDRO_DAO_CONTRACT_ADDRESS'/home'
