package chainsuite

import (
	"context"
	"os"

	"github.com/stretchr/testify/suite"
)

type Suite struct {
	suite.Suite
	HubChain     *Chain
	NeutronChain *Chain
	Relayer      *Relayer
	ctx          context.Context
}

func (s *Suite) SetupSuite() {
	ctx, err := NewSuiteContext(&s.Suite)
	s.Require().NoError(err)
	s.ctx = ctx

	// create and start hub chain
	s.HubChain, err = CreateChain(s.GetContext(), s.T(), GetHubSpec())
	s.Require().NoError(err)

	// setup hermes relayer
	relayer, err := NewRelayer(s.GetContext(), s.T())
	s.Require().NoError(err)
	s.Relayer = relayer
	err = relayer.SetupChainKeys(s.GetContext(), s.HubChain)
	s.Require().NoError(err)

	// create and start neutron chain
	s.NeutronChain, err = s.HubChain.AddConsumerChain(s.GetContext(), relayer, NeutronChainID, GetNeutronSpec)
	s.Require().NoError(err)
	s.Require().NoError(s.HubChain.UpdateAndVerifyStakeChange(s.GetContext(), s.NeutronChain, relayer, 1_000_000, 0, 1))

	// copy hydro contract to neutron validator
	hydroContract, err := os.ReadFile("../../artifacts/hydro.wasm")
	s.Require().NoError(err)
	s.Require().NoError(s.NeutronChain.GetNode().WriteFile(s.GetContext(), hydroContract, "hydro.wasm"))

	// start icq relayer
	sidecarConfig := GetIcqSidecarConfig(s.HubChain, s.NeutronChain)
	dockerClient, dockerNetwork := GetDockerContext(ctx)
	err = s.NeutronChain.NewSidecarProcess(
		s.ctx,
		sidecarConfig.PreStart,
		sidecarConfig.ProcessName,
		s.T().Name(),
		dockerClient,
		dockerNetwork,
		sidecarConfig.Image,
		sidecarConfig.HomeDir,
		0,
		sidecarConfig.Ports,
		sidecarConfig.StartCmd,
		sidecarConfig.Env,
	)
	s.Require().NoError(err)
	err = CopyIcqRelayerKey(s, s.NeutronChain.Sidecars[0])
	s.Require().NoError(err)
	err = s.NeutronChain.StartAllSidecars(s.ctx)
	s.Require().NoError(err)
}

func (s *Suite) GetContext() context.Context {
	s.Require().NotNil(s.ctx, "Tried to GetContext before it was set. SetupSuite must run first")
	return s.ctx
}
