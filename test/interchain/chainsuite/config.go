package chainsuite

import (
	"time"

	"github.com/strangelove-ventures/interchaintest/v8/testutil"

	sdkmath "cosmossdk.io/math"
	sdktypes "github.com/cosmos/cosmos-sdk/types"
)

const (
	//hub params
	HubImageName    = "ghcr.io/hyphacoop/gaia"
	HubImageVersion = "v19.0.0"
	HubBin          = "gaiad"
	HubBech32Prefix = "cosmos"
	HubChainID      = "gaia"
	Uatom           = "uatom"
	//neutron params
	NeutronImageName    = "ghcr.io/strangelove-ventures/heighliner/neutron"
	NeutronVersion      = "v4.2.0"
	NeutronBin          = "neutrond"
	NeutronBech32Prefix = "neutron"
	NeutronChainID      = "neutron"
	Untrn               = "untrn"
	// relayer params
	RelayerImageName    = "ghcr.io/informalsystems/hermes"
	RelayerImageVersion = "v1.8.0"
	// common params
	GovMinDepositAmount    = 1000
	GovDepositPeriod       = 60 * time.Second
	GovVotingPeriod        = 80 * time.Second
	DowntimeJailDuration   = 10 * time.Second
	ProviderSlashingWindow = 10
	GasPrices              = "0.005"
	UpgradeDelta           = 30
	SlashingWindowConsumer = 20
	CommitTimeout          = 4 * time.Second
	TotalValidatorFunds    = 11_000_000_000
	ValidatorFunds         = 30_000_000
	ValidatorCount         = 2
	FullNodeCount          = 0
	ChainSpawnWait         = 155 * time.Second
	CosmosChainType        = "cosmos"
)

func DefaultConfigToml() testutil.Toml {
	configToml := make(testutil.Toml)
	consensusToml := make(testutil.Toml)
	consensusToml["timeout_commit"] = CommitTimeout
	configToml["consensus"] = consensusToml
	configToml["block_sync"] = false
	configToml["fast_sync"] = false
	return configToml
}

func DefaultGenesisAmounts(denom string) func(i int) (sdktypes.Coin, sdktypes.Coin) {
	return func(i int) (sdktypes.Coin, sdktypes.Coin) {
		return sdktypes.Coin{
				Denom:  denom,
				Amount: sdkmath.NewInt(TotalValidatorFunds),
			}, sdktypes.Coin{
				Denom:  denom,
				Amount: sdkmath.NewInt(ValidatorFunds),
			}
	}
}
