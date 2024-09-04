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

func (s *HydroSuite) TestLockTokens() {
	// delegate tokens
	_, err := s.HubChain.Validators[0].ExecTx(
		s.GetContext(),
		s.HubChain.ValidatorWallets[0].Moniker,
		"staking", "delegate", s.HubChain.ValidatorWallets[0].ValoperAddress, txAmountUatom(1000),
	)
	s.Require().NoError(err)

	// liquid stake tokens
	txHash, err := s.HubChain.Validators[0].ExecTx(
		s.GetContext(),
		s.HubChain.ValidatorWallets[0].Moniker,
		"staking", "tokenize-share",
		s.HubChain.ValidatorWallets[0].ValoperAddress, txAmountUatom(500), s.HubChain.ValidatorWallets[0].Address,
	)
	s.Require().NoError(err)
	response, err := s.HubChain.Validators[0].TxHashToResponse(s.GetContext(), txHash)
	s.Require().NoError(err)
	recordId1, found := getEvtAttribute(response.Events, stakingtypes.EventTypeTokenizeShares, stakingtypes.AttributeKeyShareRecordID)
	s.Require().True(found)

	// transfer share tokens to neutron chain
	hubTransferChannel, err := s.Relayer.GetTransferChannel(s.GetContext(), s.HubChain, s.NeutronChain)
	s.Require().NoError(err)
	amountToSend := math.NewInt(400)

	sourceIbcDenom1 := fmt.Sprintf("%s/%s", strings.ToLower(s.HubChain.ValidatorWallets[0].ValoperAddress), recordId1)
	srcDenomTrace1 := transfertypes.ParseDenomTrace(transfertypes.GetPrefixedDenom("transfer", hubTransferChannel.Counterparty.ChannelID, sourceIbcDenom1))
	dstIbcDenom1 := srcDenomTrace1.IBCDenom()
	_, err = s.HubChain.Validators[0].SendIBCTransfer(s.GetContext(), hubTransferChannel.ChannelID, s.HubChain.ValidatorWallets[0].Moniker, ibc.WalletAmount{
		Denom:   sourceIbcDenom1,
		Amount:  amountToSend,
		Address: s.NeutronChain.ValidatorWallets[0].Address,
	}, ibc.TransferOptions{})
	s.Require().NoError(err)

	tCtx, tCancel := context.WithTimeout(s.GetContext(), 30*chainsuite.CommitTimeout)
	defer tCancel()

	// check that tokens are sent
	ibcTokensReceived := false
	for tCtx.Err() == nil {
		time.Sleep(chainsuite.CommitTimeout)
		receivedAmt1, err := s.NeutronChain.GetBalance(s.GetContext(), s.NeutronChain.ValidatorWallets[0].Address, dstIbcDenom1)
		if err != nil {
			continue
		}

		if receivedAmt1.Equal(amountToSend) {
			ibcTokensReceived = true
			break
		}
	}
	s.Require().True(ibcTokensReceived)

	// deploy hydro contract
	// store code
	hydroContract, err := os.ReadFile("testdata/hydro.wasm")
	s.Require().NoError(err)

	s.Require().NoError(s.NeutronChain.GetNode().WriteFile(s.GetContext(), hydroContract, "hydro.wasm"))
	contractPath := path.Join(s.NeutronChain.GetNode().HomeDir(), "hydro.wasm")

	txHash, err = s.NeutronChain.Validators[0].ExecTx(
		s.GetContext(),
		s.HubChain.ValidatorWallets[0].Moniker,
		"wasm", "store", contractPath, "--gas", "auto",
	)
	s.Require().NoError(err)
	response, err = s.NeutronChain.Validators[0].TxHashToResponse(s.GetContext(), txHash)
	s.Require().NoError(err)
	codeId, found := getEvtAttribute(response.Events, wasmtypes.EventTypeStoreCode, wasmtypes.AttributeKeyCodeID)
	s.Require().True(found)

	// instantiate code
	firstRoundStartTime := time.Now().UnixNano() + 10000000000 // 10sec from now
	neutronWallet1Address := s.NeutronChain.ValidatorWallets[0].Address
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
		"whitelist_admins":                   []string{neutronWallet1Address},
		"initial_whitelist":                  []string{neutronWallet1Address},
		"max_validator_shares_participating": 180,
		"hub_connection_id":                  neutronTransferChannel.ConnectionHops[0],
		"hub_transfer_channel_id":            neutronTransferChannel.ChannelID,
		"icq_update_period":                  50,
	}
	initHydroJson, err := json.Marshal(initHydro)
	s.Require().NoError(err)

	txHash, err = s.NeutronChain.Validators[0].ExecTx(
		s.GetContext(),
		s.NeutronChain.ValidatorWallets[0].Moniker,
		"wasm", "instantiate", codeId, string(initHydroJson), "--admin", neutronWallet1Address, "--label", "Hydro Smart Contract", "--gas", "auto",
	)
	s.Require().NoError(err)
	response, err = s.NeutronChain.Validators[0].TxHashToResponse(s.GetContext(), txHash)
	s.Require().NoError(err)
	contractAddr, found := getEvtAttribute(response.Events, wasmtypes.EventTypeInstantiate, wasmtypes.AttributeKeyContractAddr)
	s.Require().True(found)
	time.Sleep(time.Second * 10) // wait for the first round to start

	// register interchain query
	icqs := map[string]interface{}{
		"create_icqs_for_validators": map[string]interface{}{
			"validators": []string{s.HubChain.ValidatorWallets[0].ValoperAddress, s.HubChain.ValidatorWallets[1].ValoperAddress},
		},
	}
	icqsJson, err := json.Marshal(icqs)
	s.Require().NoError(err)

	_, err = s.NeutronChain.Validators[0].ExecTx(
		s.GetContext(),
		s.NeutronChain.ValidatorWallets[0].Moniker,
		"wasm", "execute", contractAddr, string(icqsJson), "--amount", strconv.Itoa(2*chainsuite.NeutronMinQueryDeposit)+"untrn", "--gas", "auto",
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
			"interchainqueries", "registered-queries",
		)
		if err != nil {
			continue
		}

		var queryResponse chainsuite.QueryResponse
		err = json.Unmarshal([]byte(queryRes), &queryResponse)
		s.Require().NoError(err)
		s.Require().NotNil(queryResponse)
		if len(queryResponse.RegisteredQueries) > 0 && queryResponse.RegisteredQueries[0].LastSubmittedResultLocalHeight != "0" {
			dataSubmitted = true
			break
		}

	}
	s.Require().True(dataSubmitted)

	//lockTxData tokens
	lockTxData := map[string]interface{}{
		"lock_tokens": map[string]interface{}{
			"lock_duration": 86400000000000,
		},
	}
	lockTxJson, err := json.Marshal(lockTxData)
	s.Require().NoError(err)

	lockAmt := "10"
	_, err = s.NeutronChain.Validators[0].ExecTx(
		s.GetContext(),
		s.NeutronChain.ValidatorWallets[0].Moniker,
		"wasm", "execute", contractAddr, string(lockTxJson), "--amount", lockAmt+dstIbcDenom1, "--gas", "auto",
	)
	s.Require().NoError(err)

	lockQueryData := map[string]interface{}{
		"all_user_lockups": map[string]interface{}{
			"address":    s.NeutronChain.ValidatorWallets[0].Address,
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
	amount := lockResponse.Data.Lockups[0].LockEntry.Funds.Amount
	s.Require().Equal(lockAmt, amount)
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
