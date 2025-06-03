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

	// Predefined wallets used in JSON files to generate merkle roots that will be used in one of the tests.
	// These wallets are restored during the suite setup and used in tests to lock tokens, per predefined criteria.
	HydroWalletMnemonic1  = "apart inside paper race eager reject mechanic stable cloth plunge file metal hire caught behind story thrive subject swift sausage upper country deliver barrel"
	HydroWalletMoniker1   = "HydroEligibleUser1"
	NeutronWalletAddress1 = "neutron12kentvdxyxff9d8q5llksekm5cxvhucy62asdf"
	CosmosWalletAddress1  = "cosmos12kentvdxyxff9d8q5llksekm5cxvhucy745jhw"
	HydroWalletMnemonic2  = "work vanish fatal thumb ketchup luxury nice family replace pretty plate penalty source coconut process category oil region enrich wink smooth forest learn naive"
	HydroWalletMoniker2   = "HydroEligibleUser2"
	NeutronWalletAddress2 = "neutron1rfv5q5z8am9qh9ayfhfyz70fhcpeqvgr9euw0q"
	CosmosWalletAddress2  = "cosmos1rfv5q5z8am9qh9ayfhfyz70fhcpeqvgrpx4v48"
	// Merkle root generated for the two above addresses, from the following JSON input file:
	// [
	//   { "address": "neutron12kentvdxyxff9d8q5llksekm5cxvhucy62asdf", "amount": "80"},
	//   { "address": "neutron1rfv5q5z8am9qh9ayfhfyz70fhcpeqvgr9euw0q", "amount": "70"}
	// ]
	Stage1MerkleRoot   = "b7e52a5b0036ce5f6d1c42efa5cb6a218374f8c7f38fa6cecd69c67c3ebbb0f9"
	Stage1Wallet1Proof = "ef1797cb598de8d87184bed7dbb5d37e0a478dbb5d0de7525043569993daa473"
	Stage1Wallet2Proof = "4e57c896cd84f0e866e2bcd97ec142336a4f2bb7bc6fcb546699e27c389b9cda"
	// Merkle root generated for the two above addresses, from the following JSON input file:
	// [
	//   { "address": "cosmos12kentvdxyxff9d8q5llksekm5cxvhucy745jhw", "amount": "160"},
	//   { "address": "cosmos1rfv5q5z8am9qh9ayfhfyz70fhcpeqvgrpx4v48", "amount": "140"}
	// ]
	Stage2MerkleRoot   = "d7e3edd8e2219f77b4bf80f562e5ce3ba534b8e2ccb19ccedb7f5ec8d45550ae"
	Stage2Wallet1Proof = "80e1a6b46a70f11009141438622351f160d4ed249590b0e307e5c8acad538e01"
	Stage2Wallet2Proof = "d7535a7d936a1746e2b2c283746834464a1ee2f35b1b1fdc068956a3b30f2291"
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
