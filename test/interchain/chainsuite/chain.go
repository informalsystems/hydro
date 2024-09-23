package chainsuite

import (
	"context"
	"fmt"
	"io"
	"net/http"
	"strconv"
	"sync"
	"time"

	sdkmath "cosmossdk.io/math"
	govv1 "github.com/cosmos/cosmos-sdk/x/gov/types/v1"
	clienttypes "github.com/cosmos/ibc-go/v8/modules/core/02-client/types"
	ccvclient "github.com/cosmos/interchain-security/v5/x/ccv/provider/client"
	"github.com/strangelove-ventures/interchaintest/v8"
	"github.com/strangelove-ventures/interchaintest/v8/chain/cosmos"
	"github.com/strangelove-ventures/interchaintest/v8/ibc"
	"github.com/tidwall/gjson"
	"golang.org/x/sync/errgroup"
)

// This moniker is hardcoded into interchaintest
const validatorMoniker = "validator"

type Chain struct {
	*cosmos.CosmosChain
	ValidatorWallets []ValidatorWallet
	RelayerWallet    ibc.Wallet
}

type ValidatorWallet struct {
	Moniker        string
	Address        string
	ValoperAddress string
}

type consumerSpecGetter func(ctx context.Context, providerChain *Chain, proposalWaiter *proposalWaiter, spawnTime time.Time) *interchaintest.ChainSpec

func chainFromCosmosChain(cosmos *cosmos.CosmosChain, relayerWallet ibc.Wallet) (*Chain, error) {
	c := &Chain{CosmosChain: cosmos}
	wallets, err := getValidatorWallets(context.Background(), c)
	if err != nil {
		return nil, err
	}
	c.ValidatorWallets = wallets
	c.RelayerWallet = relayerWallet
	return c, nil
}

// CreateChain creates a single new chain with the given version and returns the chain object.
func CreateChain(ctx context.Context, testName interchaintest.TestName, spec *interchaintest.ChainSpec) (*Chain, error) {
	cf := interchaintest.NewBuiltinChainFactory(
		GetLogger(ctx),
		[]*interchaintest.ChainSpec{spec},
	)

	chains, err := cf.Chains(testName.Name())
	if err != nil {
		return nil, err
	}
	cosmosChain := chains[0].(*cosmos.CosmosChain)
	relayerWallet, err := cosmosChain.BuildRelayerWallet(ctx, "relayer-"+cosmosChain.Config().ChainID)
	if err != nil {
		return nil, err
	}

	ic := interchaintest.NewInterchain().AddChain(cosmosChain, ibc.WalletAmount{
		Address: relayerWallet.FormattedAddress(),
		Denom:   cosmosChain.Config().Denom,
		Amount:  sdkmath.NewInt(TotalValidatorFunds),
	})

	dockerClient, dockerNetwork := GetDockerContext(ctx)

	if err := ic.Build(ctx, GetRelayerExecReporter(ctx), interchaintest.InterchainBuildOptions{
		Client:    dockerClient,
		NetworkID: dockerNetwork,
		TestName:  testName.Name(),
	}); err != nil {
		return nil, err
	}

	chain, err := chainFromCosmosChain(cosmosChain, relayerWallet)
	if err != nil {
		return nil, err
	}
	return chain, nil
}

func (p *Chain) AddConsumerChain(ctx context.Context, relayer *Relayer, chainId string, chainSpecGetter consumerSpecGetter) (*Chain, error) {
	dockerClient, dockerNetwork := GetDockerContext(ctx)

	spawnTime := time.Now().Add(ChainSpawnWait)
	proposalWaiter, errCh, err := p.consumerAdditionProposal(ctx, chainId, spawnTime)
	if err != nil {
		return nil, err
	}

	chainSpec := chainSpecGetter(ctx, p, proposalWaiter, spawnTime)

	cf := interchaintest.NewBuiltinChainFactory(
		GetLogger(ctx),
		[]*interchaintest.ChainSpec{chainSpec},
	)
	chains, err := cf.Chains(p.GetNode().TestName)
	if err != nil {
		return nil, err
	}
	cosmosConsumer := chains[0].(*cosmos.CosmosChain)

	// We can't use AddProviderConsumerLink here because the provider chain is already built; we'll have to do everything by hand.
	p.Consumers = append(p.Consumers, cosmosConsumer)
	cosmosConsumer.Provider = p.CosmosChain

	relayerWallet, err := cosmosConsumer.BuildRelayerWallet(ctx, "relayer-"+cosmosConsumer.Config().ChainID)
	if err != nil {
		return nil, err
	}
	wallets := make([]ibc.Wallet, len(p.Validators)+1)
	wallets[0] = relayerWallet
	// This is a hack, but we need to create wallets for the validators that have the right moniker.
	for i := 1; i <= len(p.Validators); i++ {
		wallets[i], err = cosmosConsumer.BuildRelayerWallet(ctx, validatorMoniker)
		if err != nil {
			return nil, err
		}
	}
	walletAmounts := make([]ibc.WalletAmount, len(wallets)+1)
	for i, wallet := range wallets {
		walletAmounts[i] = ibc.WalletAmount{
			Address: wallet.FormattedAddress(),
			Denom:   cosmosConsumer.Config().Denom,
			Amount:  sdkmath.NewInt(TotalValidatorFunds),
		}
	}

	// fund icq relayer
	walletAmounts[len(wallets)] = ibc.WalletAmount{
		Address: IcqRelayerAddress,
		Denom:   cosmosConsumer.Config().Denom,
		Amount:  sdkmath.NewInt(TotalValidatorFunds),
	}

	ic := interchaintest.NewInterchain().
		AddChain(cosmosConsumer, walletAmounts...).
		AddRelayer(relayer, "relayer")

	if err := ic.Build(ctx, GetRelayerExecReporter(ctx), interchaintest.InterchainBuildOptions{
		Client:    dockerClient,
		NetworkID: dockerNetwork,
		TestName:  p.GetNode().TestName,
	}); err != nil {
		return nil, err
	}

	// The chain should be built now, so we gotta check for errors in passing the proposal.
	if err := <-errCh; err != nil {
		return nil, err
	}

	for i, val := range cosmosConsumer.Validators {
		if err := val.RecoverKey(ctx, validatorMoniker, wallets[i+1].Mnemonic()); err != nil {
			return nil, err
		}
	}
	consumer, err := chainFromCosmosChain(cosmosConsumer, relayerWallet)
	if err != nil {
		return nil, err
	}

	err = relayer.SetupChainKeys(ctx, consumer)
	if err != nil {
		return nil, err
	}
	rep := GetRelayerExecReporter(ctx)
	if err := relayer.StopRelayer(ctx, rep); err != nil {
		return nil, err
	}
	if err := relayer.StartRelayer(ctx, rep); err != nil {
		return nil, err
	}
	err = connectProviderConsumer(ctx, p, consumer, relayer)
	if err != nil {
		return nil, err
	}

	return consumer, nil
}

func (c *Chain) WaitForProposalStatus(ctx context.Context, proposalID string, status govv1.ProposalStatus) error {
	propID, err := strconv.ParseInt(proposalID, 10, 64)
	if err != nil {
		return err
	}
	chainHeight, err := c.Height(ctx)
	if err != nil {
		return err
	}
	maxHeight := chainHeight + UpgradeDelta
	_, err = cosmos.PollForProposalStatusV1(ctx, c.CosmosChain, chainHeight, maxHeight, uint64(propID), status)
	return err
}

func (c *Chain) PassProposal(ctx context.Context, proposalID string) error {
	propID, err := strconv.ParseInt(proposalID, 10, 64)
	if err != nil {
		return err
	}
	err = c.VoteOnProposalAllValidators(ctx, uint64(propID), cosmos.ProposalVoteYes)
	if err != nil {
		return err
	}
	return c.WaitForProposalStatus(ctx, proposalID, govv1.StatusPassed)
}

func (p *Chain) consumerAdditionProposal(ctx context.Context, chainID string, spawnTime time.Time) (*proposalWaiter, chan error, error) {
	propWaiter := newProposalWaiter()
	prop := ccvclient.ConsumerAdditionProposalJSON{
		Title:         fmt.Sprintf("Addition of %s consumer chain", chainID),
		Summary:       "Proposal to add new consumer chain",
		ChainId:       chainID,
		InitialHeight: clienttypes.Height{RevisionNumber: clienttypes.ParseChainID(chainID), RevisionHeight: 1},
		GenesisHash:   []byte("gen_hash"),
		BinaryHash:    []byte("bin_hash"),
		SpawnTime:     spawnTime,

		BlocksPerDistributionTransmission: 1000,
		CcvTimeoutPeriod:                  2419200000000000,
		TransferTimeoutPeriod:             3600000000000,
		ConsumerRedistributionFraction:    "0.75",
		HistoricalEntries:                 10000,
		UnbondingPeriod:                   1728000000000000,
		Deposit:                           strconv.Itoa(GovMinDepositAmount/2) + p.Config().Denom,
		TopN:                              95,
	}

	propTx, err := p.ConsumerAdditionProposal(ctx, interchaintest.FaucetAccountKeyName, prop)
	if err != nil {
		return nil, nil, err
	}
	errCh := make(chan error)
	go func() {
		defer close(errCh)
		if err := p.WaitForProposalStatus(ctx, propTx.ProposalID, govv1.StatusDepositPeriod); err != nil {
			errCh <- err
			return
		}
		propWaiter.waitForDepositAllowed()

		if _, err := p.GetNode().ExecTx(ctx, interchaintest.FaucetAccountKeyName, "gov", "deposit", propTx.ProposalID, prop.Deposit); err != nil {
			errCh <- err
			return
		}

		if err := p.WaitForProposalStatus(ctx, propTx.ProposalID, govv1.StatusVotingPeriod); err != nil {
			errCh <- err
			return
		}
		propWaiter.startVotingPeriod()
		propWaiter.waitForVoteAllowed()

		if err := p.PassProposal(ctx, propTx.ProposalID); err != nil {
			errCh <- err
			return
		}
		propWaiter.pass()
	}()
	return propWaiter, errCh, nil
}

// UpdateAndVerifyStakeChange updates the staking amount on the provider chain and verifies that the change is reflected on the consumer side
func (p *Chain) UpdateAndVerifyStakeChange(ctx context.Context, consumer *Chain, relayer *Relayer, amount, valIdx int) error {
	providerAddress := p.ValidatorWallets[valIdx]

	providerHex, err := p.GetValidatorHexAddress(ctx, valIdx)
	if err != nil {
		return err
	}
	consumerHex, err := consumer.GetValidatorHexAddress(ctx, valIdx)
	if err != nil {
		return err
	}

	providerPowerBefore, err := p.GetValidatorPower(ctx, providerHex)
	if err != nil {
		return err
	}

	// increase the stake for the given validator
	_, err = p.Validators[valIdx].ExecTx(ctx, providerAddress.Moniker,
		"staking", "delegate",
		providerAddress.ValoperAddress, fmt.Sprintf("%d%s", amount, p.Config().Denom),
	)
	if err != nil {
		return err
	}

	// check that the validator power is updated on both, provider and consumer chains
	tCtx, tCancel := context.WithTimeout(ctx, 15*time.Minute)
	defer tCancel()
	var retErr error
	for tCtx.Err() == nil {
		retErr = nil
		providerPower, err := p.GetValidatorPower(ctx, providerHex)
		if err != nil {
			return err
		}
		consumerPower, err := consumer.GetValidatorPower(ctx, consumerHex)
		if err != nil {
			return err
		}
		if providerPowerBefore >= providerPower {
			retErr = fmt.Errorf("provider power did not increase after delegation")
		} else if providerPower != consumerPower {
			retErr = fmt.Errorf("consumer power did not update after provider delegation")
		}
		if retErr == nil {
			break
		}
		time.Sleep(CommitTimeout)
	}
	return retErr
}

func (p *Chain) GetValidatorHexAddress(ctx context.Context, valIdx int) (string, error) {
	json, err := p.Validators[valIdx].ReadFile(ctx, "config/priv_validator_key.json")
	if err != nil {
		return "", err
	}
	return gjson.GetBytes(json, "address").String(), nil
}

func (c *Chain) GetValidatorPower(ctx context.Context, hexaddr string) (int64, error) {
	var power int64
	err := checkEndpoint(c.GetHostRPCAddress()+"/validators", func(b []byte) error {
		power = gjson.GetBytes(b, fmt.Sprintf("result.validators.#(address==\"%s\").voting_power", hexaddr)).Int()
		if power == 0 {
			return fmt.Errorf("validator %s power not found; validators are: %s", hexaddr, string(b))
		}
		return nil
	})
	if err != nil {
		return 0, err
	}
	return power, nil
}

func getValidatorWallets(ctx context.Context, chain *Chain) ([]ValidatorWallet, error) {
	wallets := make([]ValidatorWallet, ValidatorCount)
	lock := new(sync.Mutex)
	eg := new(errgroup.Group)
	for i := 0; i < ValidatorCount; i++ {
		i := i
		eg.Go(func() error {
			// This moniker is hardcoded into the chain's genesis process.
			moniker := validatorMoniker
			address, err := chain.Validators[i].KeyBech32(ctx, moniker, "acc")
			if err != nil {
				return err
			}
			valoperAddress, err := chain.Validators[i].KeyBech32(ctx, moniker, "val")
			if err != nil {
				return err
			}
			lock.Lock()
			defer lock.Unlock()
			wallets[i] = ValidatorWallet{
				Moniker:        moniker,
				Address:        address,
				ValoperAddress: valoperAddress,
			}
			return nil
		})
	}
	if err := eg.Wait(); err != nil {
		return nil, err
	}
	return wallets, nil
}

func connectProviderConsumer(ctx context.Context, provider *Chain, consumer *Chain, relayer *Relayer) error {
	icsPath := relayerICSPathFor(provider, consumer)
	rep := GetRelayerExecReporter(ctx)
	if err := relayer.GeneratePath(ctx, rep, consumer.Config().ChainID, provider.Config().ChainID, icsPath); err != nil {
		return err
	}

	consumerClients, err := relayer.GetClients(ctx, rep, consumer.Config().ChainID)
	if err != nil {
		return err
	}

	var consumerClient *ibc.ClientOutput
	for _, client := range consumerClients {
		if client.ClientState.ChainID == provider.Config().ChainID {
			consumerClient = client
			break
		}
	}
	if consumerClient == nil {
		return fmt.Errorf("consumer chain %s does not have a client tracking the provider chain %s", consumer.Config().ChainID, provider.Config().ChainID)
	}
	consumerClientID := consumerClient.ClientID

	providerClients, err := relayer.GetClients(ctx, rep, provider.Config().ChainID)
	if err != nil {
		return err
	}

	var providerClient *ibc.ClientOutput
	for _, client := range providerClients {
		if client.ClientState.ChainID == consumer.Config().ChainID {
			providerClient = client
			break
		}
	}
	if providerClient == nil {
		return fmt.Errorf("provider chain %s does not have a client tracking the consumer chain %s for path %s on relayer %s",
			provider.Config().ChainID, consumer.Config().ChainID, icsPath, relayer)
	}
	providerClientID := providerClient.ClientID

	if err := relayer.UpdatePath(ctx, rep, icsPath, ibc.PathUpdateOptions{
		SrcClientID: &consumerClientID,
		DstClientID: &providerClientID,
	}); err != nil {
		return err
	}

	if err := relayer.CreateConnections(ctx, rep, icsPath); err != nil {
		return err
	}

	if err := relayer.CreateChannel(ctx, rep, icsPath, ibc.CreateChannelOptions{
		SourcePortName: "consumer",
		DestPortName:   "provider",
		Order:          ibc.Ordered,
		Version:        "1",
	}); err != nil {
		return err
	}

	tCtx, tCancel := context.WithTimeout(ctx, 30*CommitTimeout)
	defer tCancel()
	for tCtx.Err() == nil {
		var ch *ibc.ChannelOutput
		ch, err = relayer.GetTransferChannel(ctx, provider, consumer)
		if err == nil && ch != nil {
			break
		} else if err == nil {
			err = fmt.Errorf("channel not found")
		}
		time.Sleep(CommitTimeout)
	}
	return err
}

func relayerICSPathFor(chainA, chainB *Chain) string {
	return fmt.Sprintf("ics-%s-%s", chainA.Config().ChainID, chainB.Config().ChainID)
}

func checkEndpoint(url string, f func([]byte) error) error {
	resp, err := http.Get(url) //nolint:gosec
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	bts, err := io.ReadAll(resp.Body)
	if err != nil {
		return err
	}
	return f(bts)
}
