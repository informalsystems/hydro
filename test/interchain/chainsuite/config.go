package chainsuite

import (
	"time"

	"github.com/strangelove-ventures/interchaintest/v8/testutil"

	sdkmath "cosmossdk.io/math"
	sdktypes "github.com/cosmos/cosmos-sdk/types"
)

const (
	//hub params
	HubImageName     = "ghcr.io/strangelove-ventures/heighliner/gaia"
	HubImageVersion  = "v19.2.0"
	HubBin           = "gaiad"
	HubBech32Prefix  = "cosmos"
	HubValOperPrefix = "cosmosvaloper"
	HubChainID       = "gaia"
	Uatom            = "uatom"
	//neutron params
	NeutronImageName       = "ghcr.io/strangelove-ventures/heighliner/neutron"
	NeutronVersion         = "v4.2.3"
	NeutronBin             = "neutrond"
	NeutronBech32Prefix    = "neutron"
	NeutronChainID         = "neutron"
	NeutronMinQueryDeposit = 1000000
	Untrn                  = "untrn"
	// relayer params
	RelayerImageName    = "ghcr.io/informalsystems/hermes"
	RelayerImageVersion = "v1.8.0"
	// icq relayer params
	// The neutron-org/neutron-query-relayer docker image can be built locally by running 'make build-docker-relayer' command
	IcqRelayerImageName   = "neutron-org/neutron-query-relayer"
	IcqRelayerVersion     = "latest"
	IcqRelayerBin         = "neutron_query_relayer"
	IcqRelayerPort        = 9999
	IcqRelayerHome        = "/home/icq_relayer"
	IcqRelayerMoniker     = "icq_relayer"
	IcqRelayerKeyFile     = "icq_relayer.info"
	IcqRelayerKeyAddrFile = "76eb3077f2292f283d05154bb1f3d037ff366a81.address"
	IcqRelayerAddress     = "neutron1wm4nqalj9yhjs0g9z49mru7sxllnv65pqg88xu"
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
	ValidatorCount         = 4
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
				Amount: sdkmath.NewInt(ValidatorFunds - int64(i*1000)),
			}
	}
}
