package interchain

import (
	"context"
	"encoding/json"
	"fmt"
	"hydro/test/interchain/chainsuite"
	"path"
	"strconv"
	"strings"
	"time"

	"cosmossdk.io/math"
	wasmtypes "github.com/CosmWasm/wasmd/x/wasm/types"
	abci "github.com/cometbft/cometbft/abci/types"
	"github.com/cosmos/cosmos-sdk/types"
	stakingtypes "github.com/cosmos/cosmos-sdk/x/staking/types"
	transfertypes "github.com/cosmos/ibc-go/v8/modules/apps/transfer/types"
	"github.com/strangelove-ventures/interchaintest/v8/chain/cosmos"
	"github.com/strangelove-ventures/interchaintest/v8/ibc"
)

type HydroSuite struct {
	*chainsuite.Suite
}

func txAmountUatom(txAmount uint64) string {
	return fmt.Sprintf("%d%s", txAmount, chainsuite.Uatom)
}

func (s *HydroSuite) DelegateTokens(node *cosmos.ChainNode, keyMoniker string, valoperAddr string, amount string) {
	_, err := node.ExecTx(
		s.GetContext(),
		keyMoniker,
		"staking", "delegate", valoperAddr, amount,
	)
	s.Require().NoError(err)
}

func (s *HydroSuite) LiquidStakeTokens(node *cosmos.ChainNode, keyMoniker string, valoperAddr string, delegatorAddr string, amount string) string {
	txHash, err := node.ExecTx(
		s.GetContext(),
		keyMoniker,
		"staking", "tokenize-share",
		valoperAddr,
		amount,
		delegatorAddr,
	)
	s.Require().NoError(err)

	response, err := node.TxHashToResponse(s.GetContext(), txHash)
	s.Require().NoError(err)

	recordId, found := getEvtAttribute(response.Events, stakingtypes.EventTypeTokenizeShares, stakingtypes.AttributeKeyShareRecordID)
	s.Require().True(found)

	return recordId
}

func (s *HydroSuite) HubToNeutronShareTokenTransfer(
	validatorIndex int,
	amount math.Int,
	sourceIbcDenom string,
	dstAddress string,
) string {
	keyMoniker := s.HubChain.ValidatorWallets[validatorIndex].Moniker

	hubTransferChannel, err := s.Relayer.GetTransferChannel(s.GetContext(), s.HubChain, s.NeutronChain)
	s.Require().NoError(err)

	dstIbcDenom := transfertypes.ParseDenomTrace(transfertypes.GetPrefixedDenom("transfer", hubTransferChannel.Counterparty.ChannelID, sourceIbcDenom)).IBCDenom()
	_, err = s.HubChain.Validators[validatorIndex].SendIBCTransfer(s.GetContext(), hubTransferChannel.ChannelID, keyMoniker, ibc.WalletAmount{
		Denom:   sourceIbcDenom,
		Amount:  amount,
		Address: dstAddress,
	}, ibc.TransferOptions{})
	s.Require().NoError(err)

	tCtx, tCancel := context.WithTimeout(s.GetContext(), 30*chainsuite.CommitTimeout)
	defer tCancel()

	// check that tokens are sent
	ibcTokensReceived := false
	for tCtx.Err() == nil {
		time.Sleep(chainsuite.CommitTimeout)
		receivedAmt, err := s.NeutronChain.GetBalance(s.GetContext(), dstAddress, dstIbcDenom)
		if err != nil {
			continue
		}

		if receivedAmt.Equal(amount) {
			ibcTokensReceived = true
			break
		}
	}
	s.Require().True(ibcTokensReceived)

	return dstIbcDenom
}

func (s *HydroSuite) StoreCode(node *cosmos.ChainNode, keyMoniker string, contractPath string) string {
	txHash, err := node.ExecTx(s.GetContext(), keyMoniker, "wasm", "store", contractPath, "--gas", "auto")
	s.Require().NoError(err)

	response, err := node.TxHashToResponse(s.GetContext(), txHash)
	s.Require().NoError(err)

	codeId, found := getEvtAttribute(response.Events, wasmtypes.EventTypeStoreCode, wasmtypes.AttributeKeyCodeID)
	s.Require().True(found)

	return codeId
}

func (s *HydroSuite) InstantiateHydroContract(
	keyMoniker string,
	codeId string,
	adminAddr string,
	maxValParticipating int,
	roundLength int,
) string {
	firstRoundStartBuffer := int64(10000000000)
	firstRoundStartTime := time.Now().UnixNano() + firstRoundStartBuffer // 10sec from now
	neutronTransferChannel, err := s.Relayer.GetTransferChannel(s.GetContext(), s.NeutronChain, s.HubChain)
	s.Require().NoError(err)

	initHydro := map[string]interface{}{
		"round_length":      roundLength,
		"lock_epoch_length": roundLength,
		"tranches": []map[string]string{
			{
				"name":     "General tranche",
				"metadata": "General tranche metadata",
			},
			{
				"name":     "Consumer chains tranche",
				"metadata": "Consumer chains tranche metadata",
			},
		},
		"first_round_start":                  strconv.FormatInt(firstRoundStartTime, 10),
		"max_locked_tokens":                  "1000000000",
		"whitelist_admins":                   []string{adminAddr},
		"initial_whitelist":                  []string{adminAddr},
		"max_validator_shares_participating": maxValParticipating,
		"hub_connection_id":                  neutronTransferChannel.ConnectionHops[0],
		"hub_transfer_channel_id":            neutronTransferChannel.ChannelID,
		"icq_update_period":                  10,
	}
	initHydroJson, err := json.Marshal(initHydro)
	s.Require().NoError(err)

	txHash, err := s.NeutronChain.Validators[0].ExecTx(
		s.GetContext(),
		s.NeutronChain.ValidatorWallets[0].Moniker,
		"wasm", "instantiate", codeId, string(initHydroJson), "--admin", adminAddr, "--label", "Hydro Smart Contract", "--gas", "auto",
	)
	s.Require().NoError(err)
	response, err := s.NeutronChain.Validators[0].TxHashToResponse(s.GetContext(), txHash)
	s.Require().NoError(err)
	contractAddr, found := getEvtAttribute(response.Events, wasmtypes.EventTypeInstantiate, wasmtypes.AttributeKeyContractAddr)
	s.Require().True(found)
	time.Sleep(time.Nanosecond * time.Duration(firstRoundStartBuffer)) // wait for the first round to start

	return contractAddr
}

func (s *HydroSuite) InstantiateTributeContract(codeId, hydroContractAddress, adminAddr string) string {
	transferChannel, err := s.Relayer.GetTransferChannel(s.GetContext(), s.NeutronChain, s.HubChain)
	s.Require().NoError(err)

	initTribute := map[string]interface{}{
		"hydro_contract":    hydroContractAddress,
		"top_n_props_count": 2,
		"community_pool_config": map[string]interface{}{
			"bucket_address": s.HubChain.ValidatorWallets[0].Address,
			"channel_id":     transferChannel.ChannelID,
			"tax_percent":    "0.1", //10% to be sent to bucket address on the hub
		},
	}
	initTributeJson, err := json.Marshal(initTribute)
	s.Require().NoError(err)

	txHash, err := s.NeutronChain.Validators[0].ExecTx(
		s.GetContext(),
		s.NeutronChain.ValidatorWallets[0].Moniker,
		"wasm", "instantiate", codeId, string(initTributeJson), "--admin", adminAddr, "--label", "Tribute Smart Contract", "--gas", "auto",
	)
	s.Require().NoError(err)
	response, err := s.NeutronChain.Validators[0].TxHashToResponse(s.GetContext(), txHash)
	s.Require().NoError(err)
	contractAddr, found := getEvtAttribute(response.Events, wasmtypes.EventTypeInstantiate, wasmtypes.AttributeKeyContractAddr)
	s.Require().True(found)

	return contractAddr
}

func (s *HydroSuite) RegisterInterchainQueries(
	validators []string,
	contractAddr string,
	keyMoniker string,
) {
	icqs := map[string]interface{}{
		"create_icqs_for_validators": map[string]interface{}{
			"validators": validators,
		},
	}

	queryDeposit := len(validators) * chainsuite.NeutronMinQueryDeposit
	_, err := s.WasmExecuteTx(0, icqs, contractAddr, []string{"--amount", strconv.Itoa(queryDeposit) + "untrn"})
	s.Require().NoError(err)
	// Wait for the relayer to retrieve the initial query data before proceeding with locking
	tCtx, cancelFn := context.WithTimeout(s.GetContext(), 30*chainsuite.CommitTimeout)
	defer cancelFn()

	dataSubmitted := false
	for tCtx.Err() == nil {
		time.Sleep(chainsuite.CommitTimeout)
		queryRes, _, err := s.NeutronChain.Validators[0].ExecQuery(
			s.GetContext(),
			"interchainqueries", "registered-queries", "--owners", contractAddr,
		)
		if err != nil {
			continue
		}

		var queryResponse chainsuite.QueryResponse
		err = json.Unmarshal([]byte(queryRes), &queryResponse)
		s.Require().NoError(err)
		s.Require().NotNil(queryResponse)

		dataSubmitted = true
		for _, query := range queryResponse.RegisteredQueries {
			if query.LastSubmittedResultLocalHeight == "0" {
				dataSubmitted = false
				break
			}
		}

		if dataSubmitted {
			break
		}

	}
	s.Require().True(dataSubmitted)
}

func (s HydroSuite) WaitForQueryUpdate(contractAddr string, remoteHeight int64) {
	tCtx, cancelFn := context.WithTimeout(s.GetContext(), 30*chainsuite.CommitTimeout)
	defer cancelFn()

	dataSubmitted := false
	for tCtx.Err() == nil {
		time.Sleep(chainsuite.CommitTimeout)
		queryRes, _, err := s.NeutronChain.Validators[0].ExecQuery(
			s.GetContext(),
			"interchainqueries", "registered-queries", "--owners", contractAddr,
		)
		if err != nil {
			continue
		}

		var queryResponse chainsuite.QueryResponse
		err = json.Unmarshal([]byte(queryRes), &queryResponse)
		s.Require().NoError(err)
		s.Require().NotNil(queryResponse)

		dataSubmitted = true
		for _, query := range queryResponse.RegisteredQueries {
			submittedRemoteHeight, err := strconv.ParseInt(query.LastSubmittedResultRemoteHeight.RevisionHeight, 10, 64)
			s.Require().NoError(err)

			if submittedRemoteHeight < remoteHeight {
				dataSubmitted = false
				break
			}
		}

		if dataSubmitted {
			break
		}

	}
	s.Require().True(dataSubmitted)
}

func (s *HydroSuite) LockTokens(validatorIndex int, lockDuration int, lockAmount string, lockDenom string, contractAddr string) error {
	address := s.NeutronChain.ValidatorWallets[validatorIndex].Address

	lockTxData := map[string]interface{}{
		"lock_tokens": map[string]interface{}{
			"lock_duration": lockDuration,
		},
	}

	_, err := s.WasmExecuteTx(validatorIndex, lockTxData, contractAddr, []string{"--amount", lockAmount + lockDenom})
	if err != nil {
		return err
	}

	lockQueryData := map[string]interface{}{
		"all_user_lockups": map[string]interface{}{
			"address":    address,
			"start_from": 0,
			"limit":      100,
		},
	}

	lockQueryResp := s.QueryContractState(lockQueryData, contractAddr)

	var lockResponse chainsuite.LockResponse
	err = json.Unmarshal([]byte(lockQueryResp), &lockResponse)
	if err != nil {
		return err
	}

	lockFound := false
	for _, lock := range lockResponse.Data.Lockups {
		if lock.LockEntry.Funds.Denom == lockDenom {
			s.Require().Equal(lockAmount, lock.LockEntry.Funds.Amount)
			lockFound = true
		}
	}
	if !lockFound {
		return fmt.Errorf("error locking tokens")
	}

	return nil
}

func (s *HydroSuite) SubmitHydroProposal(validatorIndex int, contractAddr, proposalTitle string, trancheId int64) error {
	proposalTxData := map[string]interface{}{
		"create_proposal": map[string]interface{}{
			"tranche_id":  trancheId,
			"title":       proposalTitle,
			"description": "Proposal Description",
		},
	}

	_, err := s.WasmExecuteTx(validatorIndex, proposalTxData, contractAddr, []string{})
	if err != nil {
		return err
	}

	return nil
}

func (s *HydroSuite) VoteForHydroProposal(validatorIndex int, contractAddr string, proposalId int, trancheId int64) error {
	voteTxData := map[string]interface{}{
		"vote": map[string]interface{}{
			"tranche_id":  trancheId,
			"proposal_id": proposalId,
		},
	}

	_, err := s.WasmExecuteTx(validatorIndex, voteTxData, contractAddr, []string{})
	if err != nil {
		return err
	}

	return nil
}

func (s *HydroSuite) GetProposalByTitle(contractAddr string, proposalTitle string, trancheId int64) (chainsuite.Proposal, error) {
	roundId := s.GetCurrentRound(contractAddr)
	// query proposal to get id
	queryData := map[string]interface{}{
		"round_proposals": map[string]interface{}{
			"round_id":   roundId,
			"tranche_id": trancheId,
			"start_from": 0,
			"limit":      100,
		},
	}

	response := s.QueryContractState(queryData, contractAddr)

	var proposals chainsuite.ProposalData
	err := json.Unmarshal([]byte(response), &proposals)
	s.Require().NoError(err)

	for _, proposal := range proposals.Data.Proposals {
		if proposal.Title == proposalTitle {
			return proposal, nil
		}
	}

	return chainsuite.Proposal{}, fmt.Errorf("proposal is not found")
}

func (s *HydroSuite) GetCurrentRound(contractAddr string) int {
	queryData := map[string]interface{}{
		"current_round": map[string]interface{}{},
	}

	response := s.QueryContractState(queryData, contractAddr)

	var roundData chainsuite.RoundData
	err := json.Unmarshal([]byte(response), &roundData)
	s.Require().NoError(err)

	return roundData.Data.RoundID
}

func (s *HydroSuite) WaitForRound(contractAddress string, roundId int) {
	tCtx, cancelFn := context.WithTimeout(s.GetContext(), 1000*chainsuite.CommitTimeout)
	defer cancelFn()

	roundReached := false
	for tCtx.Err() == nil {
		time.Sleep(chainsuite.CommitTimeout)

		if s.GetCurrentRound(contractAddress) >= roundId {
			roundReached = true
			break
		}
	}

	s.Require().True(roundReached)
}

func (s *HydroSuite) GetUserVotingPower(contractAddr string, address string) int64 {
	queryData := map[string]interface{}{
		"user_voting_power": map[string]interface{}{
			"address": address,
		},
	}

	response := s.QueryContractState(queryData, contractAddr)

	var userData chainsuite.UserVotingPower
	err := json.Unmarshal([]byte(response), &userData)
	s.Require().NoError(err)

	return userData.Data.VotingPower
}

func (s *HydroSuite) GetRoundVotingPower(contractAddr string, roundId int64) int64 {
	queryData := map[string]interface{}{
		"round_total_voting_power": map[string]interface{}{
			"round_id": roundId,
		},
	}

	response := s.QueryContractState(queryData, contractAddr)
	var roundData chainsuite.RoundVotingPower
	err := json.Unmarshal([]byte(response), &roundData)
	s.Require().NoError(err)

	roundPower, err := strconv.ParseInt(roundData.Data.VotingPower, 10, 64)
	s.Require().NoError(err)

	return roundPower
}

func (s *HydroSuite) PauseTheHydroContract(keyMoniker string, contractAddr string) {
	pauseTxData := map[string]interface{}{
		"pause": map[string]interface{}{},
	}

	_, err := s.WasmExecuteTx(0, pauseTxData, contractAddr, []string{})
	s.Require().NoError(err)
}

func (s *HydroSuite) UnlockTokens(keyMoniker string, contractAddr string) error {
	unlockTxData := map[string]interface{}{
		"unlock_tokens": map[string]interface{}{},
	}

	_, err := s.WasmExecuteTx(0, unlockTxData, contractAddr, []string{})
	if err != nil {
		return err
	}

	return nil
}

func (s *HydroSuite) WhitelistAccount(contractAddr string, address string) error {
	whiteListTxData := map[string]interface{}{
		"add_account_to_whitelist": map[string]interface{}{
			"address": address,
		},
	}

	_, err := s.WasmExecuteTx(0, whiteListTxData, contractAddr, []string{})
	if err != nil {
		return err
	}

	return nil
}

func (s *HydroSuite) RemoveFromWhitelist(contractAddr string, address string) error {
	whiteListTxData := map[string]interface{}{
		"remove_account_from_whitelist": map[string]interface{}{
			"address": address,
		},
	}

	_, err := s.WasmExecuteTx(0, whiteListTxData, contractAddr, []string{})
	if err != nil {
		return err
	}

	return nil
}

func (s *HydroSuite) AddTranche(contractAddr, name, metadata string) error {
	trancheTxData := map[string]interface{}{
		"add_tranche": map[string]interface{}{
			"tranche": map[string]interface{}{
				"name":     name,
				"metadata": metadata,
			},
		},
	}

	_, err := s.WasmExecuteTx(0, trancheTxData, contractAddr, []string{})
	if err != nil {
		return err
	}

	return nil
}

func (s *HydroSuite) EditTranche(contractAddr, name, metadata string, trancheId int) error {
	trancheTxData := map[string]interface{}{
		"edit_tranche": map[string]interface{}{
			"tranche_id": trancheId,
			"name":       name,
			"metadata":   metadata,
		},
	}

	_, err := s.WasmExecuteTx(0, trancheTxData, contractAddr, []string{})
	if err != nil {
		return err
	}

	return nil
}

func (s *HydroSuite) SubmitTribute(validatorIndex, amount, trancheId, proposalId int, contractAddr string) (int, error) {
	txData := map[string]interface{}{
		"add_tribute": map[string]interface{}{
			"tranche_id":  trancheId,
			"proposal_id": proposalId,
		},
	}

	_, err := s.WasmExecuteTx(validatorIndex, txData, contractAddr, []string{"--amount", strconv.Itoa(amount) + "untrn"})
	if err != nil {
		return 0, err
	}

	return proposalId, nil
	//todo uncoment when event is added
	// response, err := s.NeutronChain.Validators[0].TxHashToResponse(s.GetContext(), txHash)
	// if err != nil {
	// 	return 0, err
	// }
	// tributeIdStr, found := getEvtAttribute(response.Events, "", "")
	// s.Require().True(found)

	// tributeId, err := strconv.ParseInt(tributeIdStr, 10, 64)
	// s.Require().NoError(err)

	// return int(tributeId), nil
}

func (s *HydroSuite) ClaimTribute(validatorIndex int, contractAddr, voterAddress string, roundId, trancheId, tributeId int) error {
	txData := map[string]interface{}{
		"claim_tribute": map[string]interface{}{
			"round_id":      roundId,
			"tranche_id":    trancheId,
			"tribute_id":    tributeId,
			"voter_address": voterAddress,
		},
	}

	_, err := s.WasmExecuteTx(validatorIndex, txData, contractAddr, []string{})
	if err != nil {
		return err
	}

	return nil
}

func (s *HydroSuite) ClaimCommunityPoolReward(validatorIndex int, contractAddr string, roundId, trancheId int) error {
	txData := map[string]interface{}{
		"claim_community_pool_tribute": map[string]interface{}{
			"round_id":   roundId,
			"tranche_id": trancheId,
		},
	}

	_, err := s.WasmExecuteTx(validatorIndex, txData, contractAddr, []string{})
	if err != nil {
		return err
	}

	return nil
}

func (s *HydroSuite) RefundTribute(validatorIndex int, contractAddr string, roundId, trancheId, tributeId, proposalId int) error {
	txData := map[string]interface{}{
		"refund_tribute": map[string]interface{}{
			"round_id":    roundId,
			"tranche_id":  trancheId,
			"tribute_id":  tributeId,
			"proposal_id": proposalId,
		},
	}

	_, err := s.WasmExecuteTx(validatorIndex, txData, contractAddr, []string{})
	if err != nil {
		return err
	}

	return nil
}

func (s *HydroSuite) WaitForCommunityRewards(address string) math.Int {
	balanceAmount := math.NewInt(0)
	tCtx, cancelFn := context.WithTimeout(s.GetContext(), 30*chainsuite.CommitTimeout)
	defer cancelFn()

	for tCtx.Err() == nil {
		time.Sleep(chainsuite.CommitTimeout)

		balances, err := s.HubChain.BankQueryAllBalances(s.GetContext(), address)
		s.Require().NoError(err)

		for _, balance := range balances {
			if strings.Contains(balance.Denom, "ibc") {
				balanceAmount = balance.Amount
				break
			}
		}
	}

	return balanceAmount
}

func (s *HydroSuite) QueryContractState(queryData map[string]interface{}, contractAddr string) []byte {
	queryJson, err := json.Marshal(queryData)
	s.Require().NoError(err)

	response, _, err := s.NeutronChain.Validators[0].ExecQuery(
		s.GetContext(),
		"wasm", "contract-state", "smart", contractAddr, string(queryJson),
	)
	s.Require().NoError(err)

	return response
}

func (s *HydroSuite) WasmExecuteTx(validatorIndex int, txData map[string]interface{}, contractAddr string, flags []string) (*types.TxResponse, error) {
	keyMoniker := s.NeutronChain.ValidatorWallets[validatorIndex].Moniker
	txJson, err := json.Marshal(txData)
	if err != nil {
		return nil, err
	}

	cmdFlags := []string{"wasm", "execute", contractAddr, string(txJson), "--gas", "auto"}
	cmdFlags = append(cmdFlags, flags...)

	txHash, err := s.NeutronChain.Validators[validatorIndex].ExecTx(
		s.GetContext(),
		keyMoniker,
		cmdFlags...,
	)
	if err != nil {
		return nil, err
	}

	response, err := s.NeutronChain.Validators[validatorIndex].TxHashToResponse(s.GetContext(), txHash)
	if err != nil {
		return nil, err
	}
	if response.Code != 0 {
		return nil, fmt.Errorf("ExecuteNeutronTx failed. Error code:%d", response.Code)
	}

	return response, nil
}

func (s *HydroSuite) GetHydroContractPath() string {
	return path.Join(s.NeutronChain.GetNode().HomeDir(), "hydro.wasm")
}

func (s *HydroSuite) GetTributeContractPath() string {
	return path.Join(s.NeutronChain.GetNode().HomeDir(), "tribute.wasm")
}

func getEvtAttribute(events []abci.Event, evtType string, key string) (string, bool) {
	for _, evt := range events {
		if evt.GetType() == evtType {
			for _, attr := range evt.Attributes {
				if attr.Key == key {
					return attr.Value, true
				}
			}
		}
	}

	return "", false
}
