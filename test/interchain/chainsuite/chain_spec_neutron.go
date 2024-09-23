package chainsuite

import (
	"context"
	"strconv"
	"time"

	"github.com/strangelove-ventures/interchaintest/v8"
	"github.com/strangelove-ventures/interchaintest/v8/chain/cosmos"
	"github.com/strangelove-ventures/interchaintest/v8/ibc"
	"github.com/strangelove-ventures/interchaintest/v8/testutil"
)

func GetNeutronSpec(ctx context.Context, hubChain *Chain, proposalWaiter *proposalWaiter, spawnTime time.Time) *interchaintest.ChainSpec {
	fullNodes := FullNodeCount
	validators := ValidatorCount

	return &interchaintest.ChainSpec{
		ChainName:     NeutronChainID,
		NumFullNodes:  &fullNodes,
		NumValidators: &validators,
		ChainConfig: ibc.ChainConfig{
			ChainID:        NeutronChainID,
			Bin:            NeutronBin,
			Denom:          Untrn,
			Type:           CosmosChainType,
			GasPrices:      GasPrices + Untrn,
			GasAdjustment:  2.0,
			TrustingPeriod: "336h",
			CoinType:       "118",
			Images: []ibc.DockerImage{
				{
					Repository: NeutronImageName,
					Version:    NeutronVersion,
					UidGid:     "1025:1025",
				},
			},
			ConfigFileOverrides: map[string]any{
				"config/config.toml": DefaultConfigToml(),
			},
			PreGenesis: func(consumer ibc.Chain) error {
				proposalWaiter.allowDeposit()
				proposalWaiter.waitForVotingPeriod()
				proposalWaiter.allowVote()
				proposalWaiter.waitForPassed()
				tCtx, tCancel := context.WithDeadline(ctx, spawnTime)
				defer tCancel()
				// interchaintest will set up the validator keys right before PreGenesis.
				// Now we just need to wait for the chain to spawn before interchaintest can get the ccv file.
				// This wait is here and not there because of changes we've made to interchaintest that need to be upstreamed in an orderly way.
				GetLogger(ctx).Sugar().Infof("waiting for chain %s to spawn at %s", NeutronChainID, spawnTime)
				<-tCtx.Done()
				if err := testutil.WaitForBlocks(ctx, 2, hubChain); err != nil {
					return err
				}

				return nil
			},
			Bech32Prefix:         NeutronBech32Prefix,
			ModifyGenesisAmounts: DefaultGenesisAmounts(Untrn),
			ModifyGenesis:        cosmos.ModifyGenesis(neutronModifiedGenesis()),
			InterchainSecurityConfig: ibc.ICSConfig{
				ConsumerCopyProviderKey: func(int) bool { return true },
				ProviderVerOverride:     "v4.1.0",
			},
			Env: []string{"TMPDIR=/var/cosmos-chain/neutron"}, // "v4.2.0" version missing /tmp folder for some reason
		},
	}
}

func neutronModifiedGenesis() []cosmos.GenesisKV {
	return []cosmos.GenesisKV{
		cosmos.NewGenesisKV("app_state.slashing.params.signed_blocks_window", strconv.Itoa(SlashingWindowConsumer)),
		cosmos.NewGenesisKV("consensus.params.block.max_gas", "50000000"),
		cosmos.NewGenesisKV("app_state.ccvconsumer.params.soft_opt_out_threshold", "0.0"),
		cosmos.NewGenesisKV("app_state.globalfee.params.minimum_gas_prices", []interface{}{
			map[string]interface{}{
				"amount": GasPrices,
				"denom":  Untrn,
			},
		}),
		cosmos.NewGenesisKV("app_state.feemarket.params.min_base_gas_price", GasPrices),
		cosmos.NewGenesisKV("app_state.feemarket.state.base_gas_price", GasPrices),
		cosmos.NewGenesisKV("app_state.feemarket.params.fee_denom", Untrn),
		cosmos.NewGenesisKV("app_state.interchainqueries.params.query_deposit", []interface{}{
			map[string]interface{}{
				"amount": strconv.Itoa(NeutronMinQueryDeposit),
				"denom":  Untrn,
			},
		}),
	}
}
