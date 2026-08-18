#!/bin/bash

if [ "$#" -ne 1 ]; then
    echo "Usage: $0 DAO_DEPLOYMENT_CONFIG_PATH"
    exit 1
fi

CONFIG_FILE="tools/deployment/config_mainnet_gaia.json"

CHAIN_BINARY=$(jq -r '.chain_binary' $CONFIG_FILE)
CHAIN_ID=$(jq -r '.chain_id' $CONFIG_FILE)
CHAIN_NODE=$(jq -r '.chain_rpc_node' $CONFIG_FILE)
TX_SENDER_WALLET=$(jq -r '.tx_sender_wallet' $CONFIG_FILE)
TX_SENDER_ADDRESS=$($CHAIN_BINARY keys show $TX_SENDER_WALLET --keyring-backend test | grep "address:" | sed 's/.*address: //')

CHAIN_ID_FLAG="--chain-id $CHAIN_ID"
KEYRING_TEST_FLAG="--keyring-backend test"
TX_FLAG="--gas auto --gas-adjustment 1.3"
CHAIN_NODE_FLAG="--node $CHAIN_NODE"
CHAIN_TX_FLAGS="$TX_FLAG --gas-prices 0.005uatom $CHAIN_ID_FLAG $CHAIN_NODE_FLAG $KEYRING_TEST_FLAG -y"

DAO_DEPLOYMENT_CONFIG_PATH="$1"
DAO_NAME=$(jq -r '.dao_name' $DAO_DEPLOYMENT_CONFIG_PATH)
DAO_DESCRIPTION=$(jq -r '.dao_description' $DAO_DEPLOYMENT_CONFIG_PATH)
DAO_ADMIN=$(jq -r '.dao_admin' $DAO_DEPLOYMENT_CONFIG_PATH)
DAO_VOTING_ADAPTER_CODE_ID=$(jq -r '.dao_voting_adapter_code_id' $DAO_DEPLOYMENT_CONFIG_PATH)
HYDRO_CONTRACT_ADDRESS=$(jq -r '.hydro_contract_address' $DAO_DEPLOYMENT_CONFIG_PATH)
PROPOSAL_SUBMISSION_APPROVER=$(jq -r '.proposal_submission_approver' $DAO_DEPLOYMENT_CONFIG_PATH)
ALLOWED_PROPOSAL_SUBMITTER=$(jq -r '.allowed_proposal_submitter' $DAO_DEPLOYMENT_CONFIG_PATH)
UUSDC_DEPOSIT_AMOUNT=$(jq -r '.uusdc_deposit_amount' $DAO_DEPLOYMENT_CONFIG_PATH)
MAX_VOTING_PERIOD=$(jq -r '.max_voting_period' $DAO_DEPLOYMENT_CONFIG_PATH)
QUORUM_PERCENT=$(jq -r '.quorum_percent' $DAO_DEPLOYMENT_CONFIG_PATH)
ONLY_MEMBERS_EXECUTE=$(jq -r '.only_members_execute' $DAO_DEPLOYMENT_CONFIG_PATH)
IMAGE_URL=$(jq -r '.image_url' $DAO_DEPLOYMENT_CONFIG_PATH)
BANNER=$(jq -r '.banner' $DAO_DEPLOYMENT_CONFIG_PATH)

EXECUTIVE_DAO_NAME=$(jq -r '.executive_dao_name' $DAO_DEPLOYMENT_CONFIG_PATH)
EXECUTIVE_DAO_DESCRIPTION=$(jq -r '.executive_dao_description' $DAO_DEPLOYMENT_CONFIG_PATH)
EXECUTIVE_IMAGE_URL=$(jq -r '.executive_image_url' $DAO_DEPLOYMENT_CONFIG_PATH)
EXECUTIVE_BANNER=$(jq -r '.executive_banner' $DAO_DEPLOYMENT_CONFIG_PATH)
EXECUTIVE_MAX_VOTING_PERIOD=$(jq -r '.executive_max_voting_period' $DAO_DEPLOYMENT_CONFIG_PATH)
EXECUTIVE_SINGLE_CHOICE_THRESHOLD_PERCENT=$(jq -r '.executive_single_choice_threshold_percent' $DAO_DEPLOYMENT_CONFIG_PATH)
EXECUTIVE_SINGLE_CHOICE_QUORUM_PERCENT=$(jq -r '.executive_single_choice_quorum_percent' $DAO_DEPLOYMENT_CONFIG_PATH)
EXECUTIVE_MULTIPLE_CHOICE_QUORUM_PERCENT=$(jq -r '.executive_multiple_choice_quorum_percent' $DAO_DEPLOYMENT_CONFIG_PATH)
EXECUTIVE_ONLY_MEMBERS_EXECUTE=$(jq -r '.executive_only_members_execute' $DAO_DEPLOYMENT_CONFIG_PATH)

# https://github.com/DA0-DA0/dao-dao-ui/blob/development/packages/utils/constants/codeIds.json
DAO_CORE_CODE_ID="179"
DAO_PROPOSAL_SINGLE_CODE_ID="186"
DAO_PREPROPOSE_APPROVAL_SINGLE_CODE_ID="181"
DAO_VOTING_CW4_CODE_ID="189"
CW4_GROUP_CODE_ID="3"
DAO_PROPOSAL_MULTIPLE_CODE_ID="185"
DAO_PRE_PROPOSE_SINGLE_CODE_ID="184"
DAO_PRE_PROPOSE_MULTIPLE_CODE_ID="183"

############################################ Hydro Executive DAO Setup ################################################

EXECUTIVE_CW4_PLACEHOLDER_WEIGHT=1000
EXECUTIVE_INITIAL_MEMBERS='[{"addr":"'$TX_SENDER_ADDRESS'","weight":'$EXECUTIVE_CW4_PLACEHOLDER_WEIGHT'}]'

EXECUTIVE_VOTING_MODULE_INIT_MSG='{"group_contract":{"new":{"cw4_group_code_id":'$CW4_GROUP_CODE_ID',"initial_members":'$EXECUTIVE_INITIAL_MEMBERS'}}}'
echo 'Hydro Executive DAO Voting (cw4) init msg:' $EXECUTIVE_VOTING_MODULE_INIT_MSG
echo ""
EXECUTIVE_VOTING_MODULE_INIT_MSG=$(echo -n $EXECUTIVE_VOTING_MODULE_INIT_MSG | base64 | tr -d '[:space:]')

EXECUTIVE_VOTING_MODULE_INSTANTIATE_INFO='{"code_id":'$DAO_VOTING_CW4_CODE_ID',"funds":[],"label":"Hydro Executive DAO Voting (cw4)","msg":"'$EXECUTIVE_VOTING_MODULE_INIT_MSG'"}'

# Both Executive proposal modules only allow DAO members to submit proposals, with no deposit.
EXECUTIVE_SUBMISSION_POLICY='{"specific": {"dao_members": true,"allowlist": [],"denylist": []}}'

# -- Proposal module A: dao-proposal-single --
EXECUTIVE_PRE_PROPOSE_SINGLE_INIT_MSG='{"deposit_info": null,"extension": {},"submission_policy": '$EXECUTIVE_SUBMISSION_POLICY'}'
echo "Hydro Executive pre-propose-single init msg:" $EXECUTIVE_PRE_PROPOSE_SINGLE_INIT_MSG
echo ""
EXECUTIVE_PRE_PROPOSE_SINGLE_INIT_MSG=$(echo -n $EXECUTIVE_PRE_PROPOSE_SINGLE_INIT_MSG | base64 | tr -d '[:space:]')

EXECUTIVE_PRE_PROPOSE_SINGLE_INSTANTIATE_INFO='{"module_may_propose": {"info": {"admin": {"core_module": {}}, "code_id": '$DAO_PRE_PROPOSE_SINGLE_CODE_ID', "label": "Hydro Executive pre-propose-single", "msg": "'$EXECUTIVE_PRE_PROPOSE_SINGLE_INIT_MSG'", "funds": []}}}'
EXECUTIVE_PROPOSAL_SINGLE_INIT_MSG='{"threshold": {"threshold_quorum": {"quorum": {"percent": "'$EXECUTIVE_SINGLE_CHOICE_QUORUM_PERCENT'"}, "threshold": {"percent": "'$EXECUTIVE_SINGLE_CHOICE_THRESHOLD_PERCENT'"}}}, "max_voting_period":{"time":'$EXECUTIVE_MAX_VOTING_PERIOD'}, "min_voting_period": null, "only_members_execute": '$EXECUTIVE_ONLY_MEMBERS_EXECUTE', "allow_revoting":false, "close_proposal_on_execution_failure":true, "veto": null, "pre_propose_info":'$EXECUTIVE_PRE_PROPOSE_SINGLE_INSTANTIATE_INFO'}'
echo 'Hydro Executive DAO Proposal Single init msg:' $EXECUTIVE_PROPOSAL_SINGLE_INIT_MSG
echo ""
EXECUTIVE_PROPOSAL_SINGLE_INIT_MSG=$(echo -n $EXECUTIVE_PROPOSAL_SINGLE_INIT_MSG | base64 | tr -d '[:space:]')

EXECUTIVE_PROPOSAL_MODULE_A_INSTANTIATE_INFO='{"code_id":'$DAO_PROPOSAL_SINGLE_CODE_ID', "funds":[], "label":"Hydro Executive DAO proposal-single", "msg":"'$EXECUTIVE_PROPOSAL_SINGLE_INIT_MSG'"}'

# -- Proposal module B: dao-proposal-multiple --
EXECUTIVE_PRE_PROPOSE_MULTIPLE_INIT_MSG='{"deposit_info": null,"extension": {},"submission_policy": '$EXECUTIVE_SUBMISSION_POLICY'}'
echo "Hydro Executive pre-propose-multiple init msg:" $EXECUTIVE_PRE_PROPOSE_MULTIPLE_INIT_MSG
echo ""
EXECUTIVE_PRE_PROPOSE_MULTIPLE_INIT_MSG=$(echo -n $EXECUTIVE_PRE_PROPOSE_MULTIPLE_INIT_MSG | base64 | tr -d '[:space:]')

EXECUTIVE_PRE_PROPOSE_MULTIPLE_INSTANTIATE_INFO='{"module_may_propose": {"info": {"admin": {"core_module": {}}, "code_id": '$DAO_PRE_PROPOSE_MULTIPLE_CODE_ID', "label": "Hydro Executive pre-propose-multiple", "msg": "'$EXECUTIVE_PRE_PROPOSE_MULTIPLE_INIT_MSG'", "funds": []}}}'
EXECUTIVE_PROPOSAL_MULTIPLE_INIT_MSG='{"voting_strategy": {"single_choice": {"quorum": {"percent": "'$EXECUTIVE_MULTIPLE_CHOICE_QUORUM_PERCENT'"}}}, "min_voting_period": null, "max_voting_period":{"time":'$EXECUTIVE_MAX_VOTING_PERIOD'}, "only_members_execute": '$EXECUTIVE_ONLY_MEMBERS_EXECUTE', "allow_revoting":false, "close_proposal_on_execution_failure":true, "veto": null, "pre_propose_info":'$EXECUTIVE_PRE_PROPOSE_MULTIPLE_INSTANTIATE_INFO'}'
echo 'Hydro Executive DAO Proposal Multiple init msg:' $EXECUTIVE_PROPOSAL_MULTIPLE_INIT_MSG
echo ""
EXECUTIVE_PROPOSAL_MULTIPLE_INIT_MSG=$(echo -n $EXECUTIVE_PROPOSAL_MULTIPLE_INIT_MSG | base64 | tr -d '[:space:]')

EXECUTIVE_PROPOSAL_MODULE_B_INSTANTIATE_INFO='{"code_id":'$DAO_PROPOSAL_MULTIPLE_CODE_ID', "funds":[], "label":"Hydro Executive DAO proposal-multiple", "msg":"'$EXECUTIVE_PROPOSAL_MULTIPLE_INIT_MSG'"}'

EXECUTIVE_INITIAL_ITEMS='[{"key": "banner", "value": "'$EXECUTIVE_BANNER'"}]'

INIT_EXECUTIVE_DAODAO='{"name":"'$EXECUTIVE_DAO_NAME'", "description":"'$EXECUTIVE_DAO_DESCRIPTION'", "image_url": "'$EXECUTIVE_IMAGE_URL'", "initial_items": '$EXECUTIVE_INITIAL_ITEMS', "automatically_add_cw20s":true, "automatically_add_cw721s":true, "voting_module_instantiate_info":'$EXECUTIVE_VOTING_MODULE_INSTANTIATE_INFO',"proposal_modules_instantiate_info":['$EXECUTIVE_PROPOSAL_MODULE_A_INSTANTIATE_INFO','$EXECUTIVE_PROPOSAL_MODULE_B_INSTANTIATE_INFO']}'
echo 'Hydro Executive DAO Core init msg:' $INIT_EXECUTIVE_DAODAO
echo ""

echo 'Instantiating Hydro Executive DAO...'
$CHAIN_BINARY tx wasm instantiate $DAO_CORE_CODE_ID "$INIT_EXECUTIVE_DAODAO" --admin $DAO_ADMIN --label "$EXECUTIVE_DAO_NAME" --from $TX_SENDER_WALLET $CHAIN_TX_FLAGS --output json &> ./instantiate_hydro_executive_dao_res.json
sleep 10

INSTANTIATE_HYDRO_EXECUTIVE_DAO_TX_HASH=$(grep -o '{.*}' ./instantiate_hydro_executive_dao_res.json | jq -r '.txhash')
$CHAIN_BINARY q tx $INSTANTIATE_HYDRO_EXECUTIVE_DAO_TX_HASH $CHAIN_NODE_FLAG --output json &> ./instantiate_hydro_executive_dao_tx.json
HYDRO_EXECUTIVE_DAO_CONTRACT_ADDRESS=$(jq -r '[.events[] | select(.type == "instantiate") | .attributes[] | select(.key == "_contract_address") | .value] | .[0]' ./instantiate_hydro_executive_dao_tx.json)

echo 'Hydro Executive DAO successfully instantiated: https://daodao.zone/dao/'$HYDRO_EXECUTIVE_DAO_CONTRACT_ADDRESS'/home'
echo ""

############################################ Hydro Governance DAO Setup ###############################################

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
    DEPOSIT_INFO='{"denom": {"token": {"denom": {"native": "ibc/F663521BF1836B00F5F177680F74BFB9A8B5654A694D0D2BC249E03CF2509013"}}}, "amount": "'$UUSDC_DEPOSIT_AMOUNT'", "refund_policy": "only_passed"}'
fi

if [ -z "$IMAGE_URL" ]; then
    IMAGE_URL='null'
else
    IMAGE_URL='"'$IMAGE_URL'"'
fi

if [ -z "$BANNER" ]; then
    INITIAL_ITEMS='null'
else
    INITIAL_ITEMS='[{"key": "banner", "value": "'$BANNER'"}]'
fi

PRE_PROPOSE_APPROVAL_INIT_MSG='{"deposit_info": '$DEPOSIT_INFO',"submission_policy": '$SUBMISSION_POLICY', "extension": { "approver": "'$PROPOSAL_SUBMISSION_APPROVER'"}}'
echo "DAO Pre-propose approval init msg:" $PRE_PROPOSE_APPROVAL_INIT_MSG
echo ""
PRE_PROPOSE_APPROVAL_INIT_MSG=$(echo -n $PRE_PROPOSE_APPROVAL_INIT_MSG | base64 | tr -d '[:space:]')

PRE_PROPOSE_MODULE_INSTANTIATE_INFO='{"module_may_propose": {"info": {"admin": {"core_module": {}}, "code_id": '$DAO_PREPROPOSE_APPROVAL_SINGLE_CODE_ID', "label": "Hydro pre-propose-single with Approver", "msg": "'$PRE_PROPOSE_APPROVAL_INIT_MSG'", "funds": []}}}'
DAO_PROPOSAL_SINGLE_INIT_MSG='{"threshold": {"threshold_quorum": {"quorum": {"percent": "'$QUORUM_PERCENT'"}, "threshold": {"majority": {}}}}, "max_voting_period":{"time":'$MAX_VOTING_PERIOD'}, "only_members_execute": '$ONLY_MEMBERS_EXECUTE', "allow_revoting":false, "close_proposal_on_execution_failure":true, "pre_propose_info":'$PRE_PROPOSE_MODULE_INSTANTIATE_INFO'}'
echo 'DAO Proposal Single init msg:' $DAO_PROPOSAL_SINGLE_INIT_MSG
echo ""
DAO_PROPOSAL_SINGLE_INIT_MSG=$(echo -n $DAO_PROPOSAL_SINGLE_INIT_MSG | base64 | tr -d '[:space:]')

PROPOSAL_MODULE_INSTANTIATE_INFO='{"code_id":'$DAO_PROPOSAL_SINGLE_CODE_ID', "funds":[], "label":"Hydro DAO proposal-single", "msg":"'$DAO_PROPOSAL_SINGLE_INIT_MSG'"}'

INIT_DAODAO='{"admin":"'$HYDRO_EXECUTIVE_DAO_CONTRACT_ADDRESS'", "name":"'$DAO_NAME'", "description":"'$DAO_DESCRIPTION'", "image_url": '$IMAGE_URL', "initial_items": '$INITIAL_ITEMS', "automatically_add_cw20s":false, "automatically_add_cw721s":false, "voting_module_instantiate_info":'$VOTING_MODULE_INSTANTIATE_INFO',"proposal_modules_instantiate_info":['$PROPOSAL_MODULE_INSTANTIATE_INFO']}'
echo 'DAO Core init msg:' $INIT_DAODAO
echo ""

echo 'Instantiating Hydro Governance DAO...'
$CHAIN_BINARY tx wasm instantiate $DAO_CORE_CODE_ID "$INIT_DAODAO" --admin $DAO_ADMIN --label "$DAO_NAME" --from $TX_SENDER_WALLET $CHAIN_TX_FLAGS --output json &> ./instantiate_hydro_dao_res.json
sleep 10

INSTANTIATE_HYDRO_DAO_TX_HASH=$(grep -o '{.*}' ./instantiate_hydro_dao_res.json | jq -r '.txhash')
$CHAIN_BINARY q tx $INSTANTIATE_HYDRO_DAO_TX_HASH $CHAIN_NODE_FLAG --output json &> ./instantiate_hydro_dao_tx.json
HYDRO_DAO_CONTRACT_ADDRESS=$(jq -r '[.events[] | select(.type == "instantiate") | .attributes[] | select(.key == "_contract_address") | .value] | .[0]' ./instantiate_hydro_dao_tx.json)

echo 'Hydro Governance DAO successfully instantiated: https://daodao.zone/dao/'$HYDRO_DAO_CONTRACT_ADDRESS'/home'
echo ""
echo 'Hydro Executive DAO: https://daodao.zone/dao/'$HYDRO_EXECUTIVE_DAO_CONTRACT_ADDRESS'/home'
echo ""
echo 'REMINDER -- follow-up steps not performed by this script:'
echo '  1. Hydro Executive'"'"'s cw4 group has a single placeholder member (the tx sender). Replace it with the'
echo '     real 10-member set via a cw4-group UpdateMembers proposal once addresses are finalized.'
echo '  2. Hydro Executive'"'"'s two proposal modules have no veto configured yet. Once real membership is in'
echo '     place, submit an Executive proposal calling UpdateConfig on each module to set'
echo '     veto.vetoer = '$HYDRO_DAO_CONTRACT_ADDRESS' (Hydro Governance), completing the reverse relation.'
