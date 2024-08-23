package chainsuite

import (
	"strconv"

	"github.com/strangelove-ventures/interchaintest/v8/chain/cosmos"
	"github.com/strangelove-ventures/interchaintest/v8/ibc"

	"github.com/strangelove-ventures/interchaintest/v8"
)

func GetHubSpec() *interchaintest.ChainSpec {
	fullNodes := FullNodeCount
	validators := ValidatorCount

	return &interchaintest.ChainSpec{
		Name:          HubChainID,
		NumFullNodes:  &fullNodes,
		NumValidators: &validators,
		Version:       HubImageVersion,
		ChainConfig: ibc.ChainConfig{
			Type:           CosmosChainType,
			Bin:            HubBin,
			Bech32Prefix:   HubBech32Prefix,
			Denom:          Uatom,
			GasPrices:      GasPrices + Uatom,
			GasAdjustment:  2.0,
			TrustingPeriod: "504h",
			ConfigFileOverrides: map[string]any{
				"config/config.toml": DefaultConfigToml(),
			},
			Images: []ibc.DockerImage{{
				Repository: HubImageName,
				UidGid:     "1025:1025", // this is the user in heighliner docker images
			}},
			ModifyGenesis:        cosmos.ModifyGenesis(hubModifiedGenesis()),
			ModifyGenesisAmounts: DefaultGenesisAmounts(Uatom),
		},
	}
}

func hubModifiedGenesis() []cosmos.GenesisKV {
	return []cosmos.GenesisKV{
		cosmos.NewGenesisKV("app_state.gov.params.voting_period", GovVotingPeriod.String()),
		cosmos.NewGenesisKV("app_state.gov.params.max_deposit_period", GovDepositPeriod.String()),
		cosmos.NewGenesisKV("app_state.gov.params.min_deposit.0.denom", Uatom),
		cosmos.NewGenesisKV("app_state.gov.params.min_deposit.0.amount", strconv.Itoa(GovMinDepositAmount)),
		cosmos.NewGenesisKV("app_state.slashing.params.signed_blocks_window", strconv.Itoa(ProviderSlashingWindow)),
		cosmos.NewGenesisKV("app_state.slashing.params.downtime_jail_duration", DowntimeJailDuration.String()),
		cosmos.NewGenesisKV("app_state.provider.params.slash_meter_replenish_period", "2s"),
		cosmos.NewGenesisKV("app_state.provider.params.slash_meter_replenish_fraction", "1.00"),
		cosmos.NewGenesisKV("app_state.provider.params.blocks_per_epoch", "1"),
		cosmos.NewGenesisKV("app_state.feemarket.params.min_base_gas_price", GasPrices),
		cosmos.NewGenesisKV("app_state.feemarket.state.base_gas_price", GasPrices),
		cosmos.NewGenesisKV("app_state.feemarket.params.fee_denom", Uatom),
	}
}
