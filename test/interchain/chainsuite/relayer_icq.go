package chainsuite

import (
	"context"
	"fmt"

	"github.com/strangelove-ventures/interchaintest/v8/chain/cosmos"
	"github.com/strangelove-ventures/interchaintest/v8/ibc"
)

func GetIcqSidecarConfig(hubChain *Chain, neutronChain *Chain) ibc.SidecarConfig {
	return ibc.SidecarConfig{
		ProcessName: IcqRelayerBin,
		HomeDir:     IcqRelayerHome,
		Image: ibc.DockerImage{
			Repository: IcqRelayerImageName,
			Version:    IcqRelayerVersion,
			UidGid:     "1025:1025",
		},
		Ports: []string{fmt.Sprintf("%d", IcqRelayerPort)},
		StartCmd: []string{
			fmt.Sprintf("/bin/%s", IcqRelayerBin),
			"start",
		},
		ValidatorProcess: false,
		PreStart:         false,
		Env:              SetIcqSidecarConfigEnv(hubChain, neutronChain),
	}
}

func SetIcqSidecarConfigEnv(hubChain *Chain, neutronChain *Chain) []string {
	return []string{
		fmt.Sprintf("NODE=%s", NeutronImageName),
		fmt.Sprintf("RELAYER_NEUTRON_CHAIN_CHAIN_PREFIX=%s", NeutronChainID),
		fmt.Sprintf("RELAYER_NEUTRON_CHAIN_RPC_ADDR=%s", neutronChain.GetRPCAddress()),
		fmt.Sprintf("RELAYER_NEUTRON_CHAIN_REST_ADDR=%s", neutronChain.GetAPIAddress()),
		fmt.Sprintf("RELAYER_NEUTRON_CHAIN_HOME_DIR=%s", IcqRelayerHome), // path to neutron key
		fmt.Sprintf("RELAYER_NEUTRON_CHAIN_SIGN_KEY_NAME=%s", IcqRelayerMoniker),
		fmt.Sprintf("RELAYER_NEUTRON_CHAIN_GAS_PRICES=%s", neutronChain.Config().GasPrices),
		fmt.Sprintf("RELAYER_NEUTRON_CHAIN_GAS_ADJUSTMENT=%f", neutronChain.Config().GasAdjustment),
		fmt.Sprintf("RELAYER_NEUTRON_CHAIN_DENOM=%s", Untrn),
		"RELAYER_NEUTRON_CHAIN_MAX_GAS_PRICE=1000",
		"RELAYER_NEUTRON_CHAIN_GAS_PRICE_MULTIPLIER=2",
		"RELAYER_NEUTRON_CHAIN_CONNECTION_ID=connection-0",
		"RELAYER_NEUTRON_CHAIN_DEBUG=true",
		fmt.Sprintf("RELAYER_NEUTRON_CHAIN_ACCOUNT_PREFIX=%s", NeutronBech32Prefix),
		"RELAYER_NEUTRON_CHAIN_KEYRING_BACKEND=test",
		fmt.Sprintf("RELAYER_TARGET_CHAIN_RPC_ADDR=%s", hubChain.GetRPCAddress()),
		fmt.Sprintf("RELAYER_TARGET_CHAIN_ACCOUNT_PREFIX=%s", HubBech32Prefix),
		fmt.Sprintf("RELAYER_TARGET_CHAIN_VALIDATOR_ACCOUNT_PREFIX=%s", HubValOperPrefix),
		"RELAYER_TARGET_CHAIN_DEBUG=true",
		"RELAYER_REGISTRY_ADDRESSES=",
		"RELAYER_ALLOW_TX_QUERIES=true",
		"RELAYER_ALLOW_KV_CALLBACKS=true",
		fmt.Sprintf("RELAYER_STORAGE_PATH=%s%s", IcqRelayerHome, "/leveldb"),
		fmt.Sprintf("RELAYER_LISTEN_ADDR=0.0.0.0:%d", IcqRelayerPort),
		"LOGGER_LEVEL=info",
		fmt.Sprintf("PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:%s", IcqRelayerHome),
	}
}

// CopyIcqRelayerKey copies the key that will be used for signing neutron txs. Since neutron_query_relayer does not have 'keys add' and the key
// cannot be recovered from the key mnemonic
func CopyIcqRelayerKey(ctx context.Context, icqSidecarProcess *cosmos.SidecarProcess) error {
	srcKeyInfoPath := fmt.Sprintf("testdata/%s", IcqRelayerKeyFile)
	srcKeyAddrPath := fmt.Sprintf("testdata/%s", IcqRelayerKeyAddrFile)
	destKeyInfoPath := fmt.Sprintf("/keyring-test/%s", IcqRelayerKeyFile)
	destKeyAddrPath := fmt.Sprintf("/keyring-test/%s", IcqRelayerKeyAddrFile)

	err := icqSidecarProcess.CopyFile(ctx, srcKeyInfoPath, destKeyInfoPath)
	if err != nil {
		return err
	}
	err = icqSidecarProcess.CopyFile(ctx, srcKeyAddrPath, destKeyAddrPath)
	if err != nil {
		return err
	}

	return nil
}
