package interchain

import (
	"context"
	"encoding/json"
	"fmt"
	"hydro/test/interchain/chainsuite"
	"strconv"
	"time"

	"cosmossdk.io/math"
	wasmtypes "github.com/CosmWasm/wasmd/x/wasm/types"
	abci "github.com/cometbft/cometbft/abci/types"
	stakingtypes "github.com/cosmos/cosmos-sdk/x/staking/types"
	transfertypes "github.com/cosmos/ibc-go/v8/modules/apps/transfer/types"
	"github.com/strangelove-ventures/interchaintest/v8/chain/cosmos"
	"github.com/strangelove-ventures/interchaintest/v8/ibc"
)

//todo: move here suite and its helper methods, leaving only tests in hydro_test.go

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
	keyMoniker string,
	amount math.Int,
	sourceIbcDenom string,
	dstAddress string,
) string {
	hubTransferChannel, err := s.Relayer.GetTransferChannel(s.GetContext(), s.HubChain, s.NeutronChain)
	s.Require().NoError(err)

	dstIbcDenom := transfertypes.ParseDenomTrace(transfertypes.GetPrefixedDenom("transfer", hubTransferChannel.Counterparty.ChannelID, sourceIbcDenom)).IBCDenom()
	_, err = s.HubChain.Validators[0].SendIBCTransfer(s.GetContext(), hubTransferChannel.ChannelID, keyMoniker, ibc.WalletAmount{
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
) string {
	firstRoundStartBuffer := int64(10000000000)
	firstRoundStartTime := time.Now().UnixNano() + firstRoundStartBuffer // 10sec from now
	neutronTransferChannel, err := s.Relayer.GetTransferChannel(s.GetContext(), s.NeutronChain, s.HubChain)
	s.Require().NoError(err)

	initHydro := map[string]interface{}{
		"round_length":      86400000000000,
		"lock_epoch_length": 86400000000000,
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
	icqsJson, err := json.Marshal(icqs)
	s.Require().NoError(err)

	queryDeposit := len(validators) * chainsuite.NeutronMinQueryDeposit
	_, err = s.NeutronChain.Validators[0].ExecTx(
		s.GetContext(),
		keyMoniker,
		"wasm", "execute", contractAddr, string(icqsJson), "--amount", strconv.Itoa(queryDeposit)+"untrn", "--gas", "auto",
	)
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

		if dataSubmitted == true {
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

		if dataSubmitted == true {
			break
		}

	}
	s.Require().True(dataSubmitted)
}

func (s *HydroSuite) LockTokens(keyMoniker string, address string, lockDuration int64, lockAmount string, lockDenom string, contractAddr string) error {
	lockTxData := map[string]interface{}{
		"lock_tokens": map[string]interface{}{
			"lock_duration": lockDuration,
		},
	}
	lockTxJson, err := json.Marshal(lockTxData)
	if err != nil {
		return err
	}

	_, err = s.NeutronChain.Validators[0].ExecTx(
		s.GetContext(),
		keyMoniker,
		"wasm", "execute", contractAddr, string(lockTxJson), "--amount", lockAmount+lockDenom, "--gas", "auto",
	)
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

func (s *HydroSuite) SubmitHydroProposal(keyMoniker, contractAddr, proposalTitle string, trancheId int64) error {
	proposalTxData := map[string]interface{}{
		"create_proposal": map[string]interface{}{
			"tranche_id":  trancheId,
			"title":       proposalTitle,
			"description": "Proposal Description",
		},
	}
	proposalTxJson, err := json.Marshal(proposalTxData)
	if err != nil {
		return err
	}

	_, err = s.NeutronChain.Validators[0].ExecTx(
		s.GetContext(),
		keyMoniker,
		"wasm", "execute", contractAddr, string(proposalTxJson), "--gas", "auto",
	)
	if err != nil {
		return err
	}

	return nil
}

func (s *HydroSuite) VoteForHydroProposal(keyMoniker string, contractAddr string, proposalId int, trancheId int64) error {
	voteTxData := map[string]interface{}{
		"vote": map[string]interface{}{
			"tranche_id":  trancheId,
			"proposal_id": proposalId,
		},
	}
	voteTxJson, err := json.Marshal(voteTxData)
	if err != nil {
		return err
	}

	_, err = s.NeutronChain.Validators[0].ExecTx(
		s.GetContext(),
		keyMoniker,
		"wasm", "execute", contractAddr, string(voteTxJson), "--gas", "auto",
	)
	if err != nil {
		return err
	}

	return nil
}

func (s *HydroSuite) GetProposalByTitle(keyMoniker string, contractAddr string, proposalTitle string, trancheId int64) (chainsuite.Proposal, error) {
	roundId := s.GetCurrentRound(keyMoniker, contractAddr)
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

func (s *HydroSuite) GetCurrentRound(keyMoniker string, contractAddr string) int64 {
	queryData := map[string]interface{}{
		"current_round": map[string]interface{}{},
	}

	response := s.QueryContractState(queryData, contractAddr)

	var roundData chainsuite.RoundData
	err := json.Unmarshal([]byte(response), &roundData)
	s.Require().NoError(err)

	return int64(roundData.Data.RoundID)
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
	pauseTxJson, err := json.Marshal(pauseTxData)
	s.Require().NoError(err)

	_, err = s.NeutronChain.Validators[0].ExecTx(
		s.GetContext(),
		keyMoniker,
		"wasm", "execute", contractAddr, string(pauseTxJson), "--gas", "auto",
	)
	s.Require().NoError(err)
}

func (s *HydroSuite) UnlockTokens(keyMoniker string, contractAddr string) error {
	unlockTxData := map[string]interface{}{
		"unlock_tokens": map[string]interface{}{},
	}
	unlockTxJson, err := json.Marshal(unlockTxData)
	if err != nil {
		return err
	}

	_, err = s.NeutronChain.Validators[0].ExecTx(
		s.GetContext(),
		keyMoniker,
		"wasm", "execute", contractAddr, string(unlockTxJson), "--gas", "auto",
	)
	if err != nil {
		return err
	}

	return nil
}

func (s *HydroSuite) WhitelistAccount(keyMoniker string, contractAddr string, address string) error {
	whiteListTxData := map[string]interface{}{
		"add_account_to_whitelist": map[string]interface{}{
			"address": address,
		},
	}
	whiteListTxJson, err := json.Marshal(whiteListTxData)
	if err != nil {
		return err
	}

	_, err = s.NeutronChain.Validators[0].ExecTx(
		s.GetContext(),
		keyMoniker,
		"wasm", "execute", contractAddr, string(whiteListTxJson), "--gas", "auto",
	)
	if err != nil {
		return err
	}

	return nil
}

func (s *HydroSuite) RemoveFromWhitelist(keyMoniker string, contractAddr string, address string) error {
	whiteListTxData := map[string]interface{}{
		"remove_account_from_whitelist": map[string]interface{}{
			"address": address,
		},
	}
	whiteListTxJson, err := json.Marshal(whiteListTxData)
	if err != nil {
		return err
	}

	_, err = s.NeutronChain.Validators[0].ExecTx(
		s.GetContext(),
		keyMoniker,
		"wasm", "execute", contractAddr, string(whiteListTxJson), "--gas", "auto",
	)
	if err != nil {
		return err
	}

	return nil
}

func (s *HydroSuite) AddTranche(keyMoniker, contractAddr, name, metadata string) error {
	trancheTxData := map[string]interface{}{
		"add_tranche": map[string]interface{}{
			"tranche": map[string]interface{}{
				"name":     name,
				"metadata": metadata,
			},
		},
	}
	trancheTxJson, err := json.Marshal(trancheTxData)
	if err != nil {
		return err
	}

	_, err = s.NeutronChain.Validators[0].ExecTx(
		s.GetContext(),
		keyMoniker,
		"wasm", "execute", contractAddr, string(trancheTxJson), "--gas", "auto",
	)
	if err != nil {
		return err
	}

	return nil
}

func (s *HydroSuite) EditTranche(keyMoniker, contractAddr, name, metadata string, trancheId int) error {
	trancheTxData := map[string]interface{}{
		"edit_tranche": map[string]interface{}{
			"tranche_id": trancheId,
			"name":       name,
			"metadata":   metadata,
		},
	}
	trancheTxJson, err := json.Marshal(trancheTxData)
	if err != nil {
		return err
	}

	_, err = s.NeutronChain.Validators[0].ExecTx(
		s.GetContext(),
		keyMoniker,
		"wasm", "execute", contractAddr, string(trancheTxJson), "--gas", "auto",
	)
	if err != nil {
		return err
	}

	return nil
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
