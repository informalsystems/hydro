package interchain

import (
	"context"
	"encoding/json"
	"fmt"
	"hydro/test/interchain/chainsuite"
	"os"
	"path"
	"strconv"
	"strings"
	"testing"
	"time"

	"cosmossdk.io/math"
	wasmtypes "github.com/CosmWasm/wasmd/x/wasm/types"
	abci "github.com/cometbft/cometbft/abci/types"
	stakingtypes "github.com/cosmos/cosmos-sdk/x/staking/types"
	transfertypes "github.com/cosmos/ibc-go/v8/modules/apps/transfer/types"
	"github.com/strangelove-ventures/interchaintest/v8/chain/cosmos"
	"github.com/strangelove-ventures/interchaintest/v8/ibc"
	"github.com/stretchr/testify/suite"
)

type HydroSuite struct {
	*chainsuite.Suite
}

func TestHydroSuite(t *testing.T) {
	s := &HydroSuite{&chainsuite.Suite{}}
	suite.Run(t, s)
}

func txAmountUatom(txAmount uint64) string {
	return fmt.Sprintf("%d%s", txAmount, chainsuite.Uatom)
}

func (s *HydroSuite) TestHappyPath() {
	hubNode := s.HubChain.Validators[0]
	neutronNode := s.NeutronChain.Validators[0]

	// delegate tokens
	s.DelegateTokens(hubNode, s.HubChain.ValidatorWallets[0].Moniker, s.HubChain.ValidatorWallets[0].ValoperAddress, txAmountUatom(1000))
	s.DelegateTokens(hubNode, s.HubChain.ValidatorWallets[0].Moniker, s.HubChain.ValidatorWallets[1].ValoperAddress, txAmountUatom(1000))

	// liquid stake tokens
	recordId1 := s.LiquidStakeTokens(hubNode, s.HubChain.ValidatorWallets[0].Moniker, s.HubChain.ValidatorWallets[0].ValoperAddress,
		s.HubChain.ValidatorWallets[0].Address, txAmountUatom(500))
	recordId2 := s.LiquidStakeTokens(hubNode, s.HubChain.ValidatorWallets[0].Moniker, s.HubChain.ValidatorWallets[1].ValoperAddress,
		s.HubChain.ValidatorWallets[0].Address, txAmountUatom(500))

	// transfer share tokens to neutron chain
	sourceIbcDenom1 := fmt.Sprintf("%s/%s", strings.ToLower(s.HubChain.ValidatorWallets[0].ValoperAddress), recordId1)
	dstIbcDenom1 := s.HubToNeutronShareTokenTransfer(s.HubChain.ValidatorWallets[0].Moniker, math.NewInt(400), sourceIbcDenom1, s.NeutronChain.ValidatorWallets[0].Address)
	sourceIbcDenom2 := fmt.Sprintf("%s/%s", strings.ToLower(s.HubChain.ValidatorWallets[1].ValoperAddress), recordId2)
	dstIbcDenom2 := s.HubToNeutronShareTokenTransfer(s.HubChain.ValidatorWallets[0].Moniker, math.NewInt(400), sourceIbcDenom2, s.NeutronChain.ValidatorWallets[0].Address)

	// deploy hydro contract
	// store code
	hydroContract, err := os.ReadFile("testdata/hydro.wasm")
	s.Require().NoError(err)

	s.Require().NoError(s.NeutronChain.GetNode().WriteFile(s.GetContext(), hydroContract, "hydro.wasm"))
	contractPath := path.Join(s.NeutronChain.GetNode().HomeDir(), "hydro.wasm")

	codeId := s.StoreCode(neutronNode, s.HubChain.ValidatorWallets[0].Moniker, contractPath)

	// instantiate code
	contractAddr := s.InstantiateHydroContract(s.NeutronChain.ValidatorWallets[0].Moniker, codeId, s.NeutronChain.ValidatorWallets[0].Address, 2)

	// register interchain query
	s.RegisterInterchainQueries([]string{s.HubChain.ValidatorWallets[0].ValoperAddress, s.HubChain.ValidatorWallets[1].ValoperAddress},
		contractAddr, s.NeutronChain.ValidatorWallets[0].Moniker)

	//lockTxData tokens
	s.LockTokens(s.NeutronChain.ValidatorWallets[0].Moniker, s.NeutronChain.ValidatorWallets[0].Address, 86400000000000, "10", dstIbcDenom1, contractAddr)
	s.LockTokens(s.NeutronChain.ValidatorWallets[0].Moniker, s.NeutronChain.ValidatorWallets[0].Address, 86400000000000, "10", dstIbcDenom2, contractAddr)
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
		"icq_update_period":                  50,
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

func (s *HydroSuite) LockTokens(keyMoniker string, address string, lockDuration int64, lockAmount string, lockDenom string, contractAddr string) {
	lockTxData := map[string]interface{}{
		"lock_tokens": map[string]interface{}{
			"lock_duration": lockDuration,
		},
	}
	lockTxJson, err := json.Marshal(lockTxData)
	s.Require().NoError(err)

	_, err = s.NeutronChain.Validators[0].ExecTx(
		s.GetContext(),
		keyMoniker,
		"wasm", "execute", contractAddr, string(lockTxJson), "--amount", lockAmount+lockDenom, "--gas", "auto",
	)
	s.Require().NoError(err)

	lockQueryData := map[string]interface{}{
		"all_user_lockups": map[string]interface{}{
			"address":    address,
			"start_from": 0,
			"limit":      100,
		},
	}
	lockQueryJson, err := json.Marshal(lockQueryData)
	s.Require().NoError(err)

	lockQueryResp, _, err := s.NeutronChain.Validators[0].ExecQuery(
		s.GetContext(),
		"wasm", "contract-state", "smart", contractAddr, string(lockQueryJson),
	)
	s.Require().NoError(err)

	var lockResponse chainsuite.LockResponse
	err = json.Unmarshal([]byte(lockQueryResp), &lockResponse)
	s.Require().NoError(err)
	s.Require().True(len(lockResponse.Data.Lockups) > 0)

	lockFound := false
	for _, lock := range lockResponse.Data.Lockups {
		if lock.LockEntry.Funds.Denom == lockDenom {
			s.Require().Equal(lockAmount, lock.LockEntry.Funds.Amount)
			lockFound = true
		}
	}
	s.Require().True(lockFound)
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
