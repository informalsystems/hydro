package chainsuite

import (
	"context"

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

	// setup relayer
	relayer, err := NewRelayer(s.GetContext(), s.T())
	s.Require().NoError(err)
	s.Relayer = relayer
	err = relayer.SetupChainKeys(s.GetContext(), s.HubChain)
	s.Require().NoError(err)

	// create and start neutron chain
	s.NeutronChain, err = s.HubChain.AddConsumerChain(s.GetContext(), relayer, NeutronChainID, GetNeutronSpec)
	s.Require().NoError(err)

	s.Require().NoError(s.HubChain.CheckCCV(s.GetContext(), s.NeutronChain, relayer, 1_000_000, 0, 1))
}

func (s *Suite) GetContext() context.Context {
	s.Require().NotNil(s.ctx, "Tried to GetContext before it was set. SetupSuite must run first")
	return s.ctx
}
