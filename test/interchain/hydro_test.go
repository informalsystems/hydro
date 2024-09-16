package interchain

import (
	"fmt"
	"hydro/test/interchain/chainsuite"
	"path"
	"strconv"
	"strings"
	"testing"

	"cosmossdk.io/math"
	"github.com/strangelove-ventures/interchaintest/v8/testutil"
	"github.com/stretchr/testify/suite"
)

func TestHydroSuite(t *testing.T) {
	s := &HydroSuite{&chainsuite.Suite{}}
	suite.Run(t, s)
}

// TestHappyPath tests:
// deployment of hydro contract
// registering of interchain queries for validators
// locking of liquid staked tokens on hydro contract
// creating and voting/revoting for hydro proposals
// pausing/disabling contract
func (s *HydroSuite) TestHappyPath() {
	hubNode := s.HubChain.GetNode()
	neutronNode := s.NeutronChain.GetNode()

	// delegate tokens
	s.DelegateTokens(hubNode, s.HubChain.ValidatorWallets[0].Moniker, s.HubChain.ValidatorWallets[0].ValoperAddress, txAmountUatom(1000))
	s.DelegateTokens(hubNode, s.HubChain.ValidatorWallets[0].Moniker, s.HubChain.ValidatorWallets[1].ValoperAddress, txAmountUatom(1000))

	// liquid stake tokens
	recordId1 := s.LiquidStakeTokens(hubNode, s.HubChain.ValidatorWallets[0].Moniker, s.HubChain.ValidatorWallets[0].ValoperAddress,
		s.HubChain.ValidatorWallets[0].Address, txAmountUatom(500))
	recordId2 := s.LiquidStakeTokens(hubNode, s.HubChain.ValidatorWallets[0].Moniker, s.HubChain.ValidatorWallets[1].ValoperAddress,
		s.HubChain.ValidatorWallets[0].Address, txAmountUatom(500))

	// transfer share tokens to neutron chain
	sourceIbcDenom1 := fmt.Sprintf("%s/%s", strings.ToLower(s.HubChain.ValidatorWallets[0].ValoperAddress), recordId1)
	dstIbcDenom1 := s.HubToNeutronShareTokenTransfer(s.HubChain.ValidatorWallets[0].Moniker, math.NewInt(400), sourceIbcDenom1, s.NeutronChain.ValidatorWallets[0].Address)
	sourceIbcDenom2 := fmt.Sprintf("%s/%s", strings.ToLower(s.HubChain.ValidatorWallets[1].ValoperAddress), recordId2)
	dstIbcDenom2 := s.HubToNeutronShareTokenTransfer(s.HubChain.ValidatorWallets[0].Moniker, math.NewInt(400), sourceIbcDenom2, s.NeutronChain.ValidatorWallets[0].Address)

	// deploy hydro contract - store code
	contractPath := path.Join(s.NeutronChain.GetNode().HomeDir(), "hydro.wasm")
	codeId := s.StoreCode(neutronNode, s.HubChain.ValidatorWallets[0].Moniker, contractPath)

	// deploy hydro contract - instantiate code
	contractAddr := s.InstantiateHydroContract(s.NeutronChain.ValidatorWallets[0].Moniker, codeId, s.NeutronChain.ValidatorWallets[0].Address, 3)

	// register interchain query
	s.RegisterInterchainQueries([]string{s.HubChain.ValidatorWallets[0].ValoperAddress, s.HubChain.ValidatorWallets[1].ValoperAddress},
		contractAddr, s.NeutronChain.ValidatorWallets[0].Moniker)

	// lockTxData tokens
	err := s.LockTokens(s.NeutronChain.ValidatorWallets[0].Moniker, s.NeutronChain.ValidatorWallets[0].Address, 86400000000000, "10", dstIbcDenom1, contractAddr)
	s.Require().NoError(err)
	err = s.LockTokens(s.NeutronChain.ValidatorWallets[0].Moniker, s.NeutronChain.ValidatorWallets[0].Address, 3*86400000000000, "10", dstIbcDenom2, contractAddr)
	s.Require().NoError(err)
	err = s.LockTokens(s.NeutronChain.ValidatorWallets[0].Moniker, s.NeutronChain.ValidatorWallets[0].Address, 6*86400000000000, "10", dstIbcDenom1, contractAddr)
	s.Require().NoError(err)
	err = s.LockTokens(s.NeutronChain.ValidatorWallets[0].Moniker, s.NeutronChain.ValidatorWallets[0].Address, 12*86400000000000, "10", dstIbcDenom2, contractAddr)
	s.Require().NoError(err)

	// Scale lockup power
	// 1x if lockup is between 0 and 1 epochs
	// 1.5x if lockup is between 1 and 3 epochs
	// 2x if lockup is between 3 and 6 epochs
	// 4x if lockup is between 6 and 12 epochs
	votingPower := "85" // 10*1+10*1.5+10*2+10*4

	// create hydro proposals
	err = s.SubmitHydroProposal(s.NeutronChain.ValidatorWallets[0].Moniker, contractAddr, "trenche 1 prop 1", 1)
	s.Require().NoError(err)
	err = s.SubmitHydroProposal(s.NeutronChain.ValidatorWallets[0].Moniker, contractAddr, "trenche 1 prop 2", 1)
	s.Require().NoError(err)
	err = s.SubmitHydroProposal(s.NeutronChain.ValidatorWallets[0].Moniker, contractAddr, "trenche 2 prop 1", 2)
	s.Require().NoError(err)

	// vote for trenche 1 proposal 1
	proposal, err := s.GetProposalByTitle(s.NeutronChain.ValidatorWallets[0].Moniker, contractAddr, "trenche 1 prop 1", 1)
	s.Require().NoError(err)
	s.Require().Equal("0", proposal.Power)

	err = s.VoteForHydroProposal(s.NeutronChain.ValidatorWallets[0].Moniker, contractAddr, proposal.ProposalID, 1)
	s.Require().NoError(err)
	proposal, err = s.GetProposalByTitle(s.NeutronChain.ValidatorWallets[0].Moniker, contractAddr, "trenche 1 prop 1", 1)
	s.Require().NoError(err)
	s.Require().Equal(votingPower, proposal.Power)

	// vote for trenche 2 proposal
	proposal, err = s.GetProposalByTitle(s.NeutronChain.ValidatorWallets[0].Moniker, contractAddr, "trenche 2 prop 1", 2)
	s.Require().NoError(err)
	s.Require().Equal("0", proposal.Power)

	err = s.VoteForHydroProposal(s.NeutronChain.ValidatorWallets[0].Moniker, contractAddr, proposal.ProposalID, 2)
	s.Require().NoError(err)
	proposal, err = s.GetProposalByTitle(s.NeutronChain.ValidatorWallets[0].Moniker, contractAddr, "trenche 2 prop 1", 2)
	s.Require().NoError(err)
	s.Require().Equal(votingPower, proposal.Power)

	// power of trenche 1 proposal 1 is not changed after voting for proposal from different trenche
	proposal, err = s.GetProposalByTitle(s.NeutronChain.ValidatorWallets[0].Moniker, contractAddr, "trenche 1 prop 1", 1)
	s.Require().NoError(err)
	s.Require().Equal(votingPower, proposal.Power)

	// revote for trenche 1 proposal 2
	proposal, err = s.GetProposalByTitle(s.NeutronChain.ValidatorWallets[0].Moniker, contractAddr, "trenche 1 prop 2", 1)
	s.Require().NoError(err)
	s.Require().Equal("0", proposal.Power)

	err = s.VoteForHydroProposal(s.NeutronChain.ValidatorWallets[0].Moniker, contractAddr, proposal.ProposalID, 1)
	s.Require().NoError(err)
	proposal, err = s.GetProposalByTitle(s.NeutronChain.ValidatorWallets[0].Moniker, contractAddr, "trenche 1 prop 2", 1)
	s.Require().NoError(err)
	s.Require().Equal(votingPower, proposal.Power)

	// power of trenche 1 proposal 1 is now 0, since we revoted for the proposal 2 from the first trenche
	proposal, err = s.GetProposalByTitle(s.NeutronChain.ValidatorWallets[0].Moniker, contractAddr, "trenche 1 prop 1", 1)
	s.Require().NoError(err)
	s.Require().Equal("0", proposal.Power)

	// pausing the contract
	s.PauseTheHydroContract(s.NeutronChain.ValidatorWallets[0].Moniker, contractAddr)
	// confirm that calling contract returns an error
	err = s.LockTokens(s.NeutronChain.ValidatorWallets[0].Moniker, s.NeutronChain.ValidatorWallets[0].Address, 86400000000000, "10", dstIbcDenom1, contractAddr)
	s.Require().Error(err)
	s.Require().Contains(err.Error(), "Paused")
	err = s.SubmitHydroProposal(s.NeutronChain.ValidatorWallets[0].Moniker, contractAddr, "trenche 2 prop 2", 2)
	s.Require().Error(err)
	s.Require().Contains(err.Error(), "Paused")
	err = s.VoteForHydroProposal(s.NeutronChain.ValidatorWallets[0].Moniker, contractAddr, proposal.ProposalID, 1)
	s.Require().Error(err)
	s.Require().Contains(err.Error(), "Paused")
	err = s.WhitelistAccount(s.NeutronChain.ValidatorWallets[0].Moniker, contractAddr, s.NeutronChain.ValidatorWallets[1].Address)
	s.Require().Error(err)
	s.Require().Contains(err.Error(), "Paused")
	err = s.RemoveFromWhitelist(s.NeutronChain.ValidatorWallets[0].Moniker, contractAddr, s.NeutronChain.ValidatorWallets[0].Address)
	s.Require().Error(err)
	s.Require().Contains(err.Error(), "Paused")
	err = s.AddTranche(s.NeutronChain.ValidatorWallets[0].Moniker, contractAddr, "test", "test")
	s.Require().Error(err)
	s.Require().Contains(err.Error(), "Paused")
	err = s.EditTranche(s.NeutronChain.ValidatorWallets[0].Moniker, contractAddr, "test", "test", 1)
	s.Require().Error(err)
	s.Require().Contains(err.Error(), "Paused")
}

// TestActiveValidatorChange tests dropping one validator from the active set, adding a new one, and checks its effect on the proposal voting power
func (s *HydroSuite) TestActiveValidatorChange() {
	hubNode := s.HubChain.GetNode()
	neutronNode := s.NeutronChain.GetNode()

	// val1 delegate tokens to validator 1(self delegate), 2 and 3
	s.DelegateTokens(hubNode, s.HubChain.ValidatorWallets[0].Moniker, s.HubChain.ValidatorWallets[0].ValoperAddress, txAmountUatom(1000))
	s.DelegateTokens(hubNode, s.HubChain.ValidatorWallets[0].Moniker, s.HubChain.ValidatorWallets[1].ValoperAddress, txAmountUatom(1000))
	s.DelegateTokens(hubNode, s.HubChain.ValidatorWallets[0].Moniker, s.HubChain.ValidatorWallets[2].ValoperAddress, txAmountUatom(1000))

	// liquid stake tokens
	recordId1 := s.LiquidStakeTokens(hubNode, s.HubChain.ValidatorWallets[0].Moniker, s.HubChain.ValidatorWallets[0].ValoperAddress,
		s.HubChain.ValidatorWallets[0].Address, txAmountUatom(500))
	recordId2 := s.LiquidStakeTokens(hubNode, s.HubChain.ValidatorWallets[0].Moniker, s.HubChain.ValidatorWallets[1].ValoperAddress,
		s.HubChain.ValidatorWallets[0].Address, txAmountUatom(500))
	recordId3 := s.LiquidStakeTokens(hubNode, s.HubChain.ValidatorWallets[0].Moniker, s.HubChain.ValidatorWallets[2].ValoperAddress,
		s.HubChain.ValidatorWallets[0].Address, txAmountUatom(500))

	// transfer share tokens to neutron chain
	sourceIbcDenom1 := fmt.Sprintf("%s/%s", strings.ToLower(s.HubChain.ValidatorWallets[0].ValoperAddress), recordId1)
	dstIbcDenom1 := s.HubToNeutronShareTokenTransfer(s.HubChain.ValidatorWallets[0].Moniker, math.NewInt(400), sourceIbcDenom1, s.NeutronChain.ValidatorWallets[0].Address)
	sourceIbcDenom2 := fmt.Sprintf("%s/%s", strings.ToLower(s.HubChain.ValidatorWallets[1].ValoperAddress), recordId2)
	dstIbcDenom2 := s.HubToNeutronShareTokenTransfer(s.HubChain.ValidatorWallets[0].Moniker, math.NewInt(400), sourceIbcDenom2, s.NeutronChain.ValidatorWallets[0].Address)
	sourceIbcDenom3 := fmt.Sprintf("%s/%s", strings.ToLower(s.HubChain.ValidatorWallets[2].ValoperAddress), recordId3)
	dstIbcDenom3 := s.HubToNeutronShareTokenTransfer(s.HubChain.ValidatorWallets[0].Moniker, math.NewInt(400), sourceIbcDenom3, s.NeutronChain.ValidatorWallets[0].Address)

	// deploy hydro contract - store code
	contractPath := path.Join(s.NeutronChain.GetNode().HomeDir(), "hydro.wasm")
	codeId := s.StoreCode(neutronNode, s.HubChain.ValidatorWallets[0].Moniker, contractPath)

	// deploy hydro contract - instantiate code
	// active valset consists of 2 validators, currently val1 and val2
	contractAddr := s.InstantiateHydroContract(s.NeutronChain.ValidatorWallets[0].Moniker, codeId, s.NeutronChain.ValidatorWallets[0].Address, 2)

	// register interchain query for val1 and val2
	s.RegisterInterchainQueries([]string{s.HubChain.ValidatorWallets[0].ValoperAddress, s.HubChain.ValidatorWallets[1].ValoperAddress},
		contractAddr, s.NeutronChain.ValidatorWallets[0].Moniker)

	// lockTxData tokens
	err := s.LockTokens(s.NeutronChain.ValidatorWallets[0].Moniker, s.NeutronChain.ValidatorWallets[0].Address, 86400000000000, "10", dstIbcDenom1, contractAddr)
	s.Require().NoError(err)
	err = s.LockTokens(s.NeutronChain.ValidatorWallets[0].Moniker, s.NeutronChain.ValidatorWallets[0].Address, 3*86400000000000, "10", dstIbcDenom2, contractAddr)
	s.Require().NoError(err)

	votingPowerVal1Val1 := "10"
	votingPowerVal1Val2 := "25"
	votingPowerVal1Val3 := "30"

	// create hydro proposals
	err = s.SubmitHydroProposal(s.NeutronChain.ValidatorWallets[0].Moniker, contractAddr, "trenche 1 prop 1", 1)
	s.Require().NoError(err)

	// vote for trenche 1 proposal 1
	proposal, err := s.GetProposalByTitle(s.NeutronChain.ValidatorWallets[0].Moniker, contractAddr, "trenche 1 prop 1", 1)
	s.Require().NoError(err)
	s.Require().Equal("0", proposal.Power)

	err = s.VoteForHydroProposal(s.NeutronChain.ValidatorWallets[0].Moniker, contractAddr, proposal.ProposalID, 1)
	s.Require().NoError(err)
	proposal, err = s.GetProposalByTitle(s.NeutronChain.ValidatorWallets[0].Moniker, contractAddr, "trenche 1 prop 1", 1)
	s.Require().NoError(err)
	s.Require().Equal(votingPowerVal1Val2, proposal.Power)

	// increase stake for val3 on hub, so that val2 drops from active valset and val3 enters
	s.Require().NoError(s.HubChain.UpdateAndVerifyStakeChange(s.GetContext(), s.NeutronChain, s.Relayer, 1_000_000, 2, 1))

	// register icq for val3
	s.RegisterInterchainQueries([]string{s.HubChain.ValidatorWallets[2].ValoperAddress}, contractAddr, s.NeutronChain.ValidatorWallets[0].Moniker)

	// lock token for val3
	err = s.LockTokens(s.NeutronChain.ValidatorWallets[0].Moniker, s.NeutronChain.ValidatorWallets[0].Address, 6*86400000000000, "10", dstIbcDenom3, contractAddr)
	s.Require().NoError(err)

	// check that voting power is equal to voting power of val1 because val2 is not among active vals anymore
	proposal, err = s.GetProposalByTitle(s.NeutronChain.ValidatorWallets[0].Moniker, contractAddr, "trenche 1 prop 1", 1)
	s.Require().NoError(err)
	s.Require().Equal(votingPowerVal1Val1, proposal.Power)

	// vote again so that val3 power is taken into account
	err = s.VoteForHydroProposal(s.NeutronChain.ValidatorWallets[0].Moniker, contractAddr, proposal.ProposalID, 1)
	s.Require().NoError(err)
	proposal, err = s.GetProposalByTitle(s.NeutronChain.ValidatorWallets[0].Moniker, contractAddr, "trenche 1 prop 1", 1)
	s.Require().NoError(err)
	s.Require().Equal(votingPowerVal1Val3, proposal.Power)
}

// TestValidatorSlashing tests that voting power of user, proposal, round is changed after validator is slashed
func (s *HydroSuite) TestValidatorSlashing() {
	hubNode := s.HubChain.GetNode()
	neutronNode := s.NeutronChain.GetNode()

	// delegate tokens
	s.DelegateTokens(hubNode, s.HubChain.ValidatorWallets[0].Moniker, s.HubChain.ValidatorWallets[3].ValoperAddress, txAmountUatom(1000))

	// liquid stake tokens
	recordId1 := s.LiquidStakeTokens(hubNode, s.HubChain.ValidatorWallets[0].Moniker, s.HubChain.ValidatorWallets[3].ValoperAddress,
		s.HubChain.ValidatorWallets[0].Address, txAmountUatom(500))

	// transfer share tokens to neutron chain
	sourceIbcDenom1 := fmt.Sprintf("%s/%s", strings.ToLower(s.HubChain.ValidatorWallets[3].ValoperAddress), recordId1)
	dstIbcDenom1 := s.HubToNeutronShareTokenTransfer(s.HubChain.ValidatorWallets[0].Moniker, math.NewInt(400), sourceIbcDenom1, s.NeutronChain.ValidatorWallets[0].Address)

	// deploy hydro contract - store code
	contractPath := path.Join(s.NeutronChain.GetNode().HomeDir(), "hydro.wasm")
	codeId := s.StoreCode(neutronNode, s.HubChain.ValidatorWallets[0].Moniker, contractPath)

	// deploy hydro contract - instantiate code
	contractAddr := s.InstantiateHydroContract(s.NeutronChain.ValidatorWallets[0].Moniker, codeId, s.NeutronChain.ValidatorWallets[0].Address, 4)

	// register interchain query
	s.RegisterInterchainQueries([]string{s.HubChain.ValidatorWallets[3].ValoperAddress},
		contractAddr, s.NeutronChain.ValidatorWallets[0].Moniker)

	// lockTxData tokens
	lockAmount := "300"
	powerAfterSlashing := 271 // ceil rounding is used, which is why the power after slashing 10% of the tokens is 271 instead of 270
	s.Require().NoError(s.LockTokens(s.NeutronChain.ValidatorWallets[0].Moniker, s.NeutronChain.ValidatorWallets[0].Address, 86400000000000, lockAmount, dstIbcDenom1, contractAddr))

	// create hydro proposals
	s.Require().NoError(s.SubmitHydroProposal(s.NeutronChain.ValidatorWallets[0].Moniker, contractAddr, "trenche 1 prop 1", 1))

	// vote for proposal
	proposal, err := s.GetProposalByTitle(s.NeutronChain.ValidatorWallets[0].Moniker, contractAddr, "trenche 1 prop 1", 1)
	s.Require().NoError(err)
	s.Require().Equal("0", proposal.Power)

	s.Require().NoError(s.VoteForHydroProposal(s.NeutronChain.ValidatorWallets[0].Moniker, contractAddr, proposal.ProposalID, 1))
	proposal, err = s.GetProposalByTitle(s.NeutronChain.ValidatorWallets[0].Moniker, contractAddr, "trenche 1 prop 1", 1)
	s.Require().NoError(err)
	s.Require().Equal(lockAmount, proposal.Power)

	// stop the val4 and wait for it to be slashed
	s.Require().NoError(s.HubChain.Validators[3].StopContainer(s.GetContext()))
	// wait for confirmation that the node is slashed
	s.Require().NoError(testutil.WaitForBlocks(s.GetContext(), chainsuite.ProviderSlashingWindow+1, s.HubChain))
	// wait for icq to get the updated data
	height, err := s.HubChain.Height(s.GetContext())
	s.Require().NoError(err)
	s.WaitForQueryUpdate(contractAddr, height)

	// restart the node - not mandatory for this test
	s.Require().NoError(s.HubChain.Validators[3].StartContainer(s.GetContext()))

	// check power after slashing
	// power decrease is based on slash_fraction_downtime from genesis
	proposal, err = s.GetProposalByTitle(s.NeutronChain.ValidatorWallets[0].Moniker, contractAddr, "trenche 1 prop 1", 1)
	s.Require().NoError(err)
	s.Require().Equal(strconv.Itoa(powerAfterSlashing), proposal.Power)

	// todo: uncomment once the contract is fixed so that it updates user power as well
	// check user total voting power
	//userPower := s.GetUserVotingPower(contractAddr, s.NeutronChain.ValidatorWallets[0].Address)
	//s.Require().Equal(powerAfterSlashing, userPower)

	// check round total voting power
	roundPower := s.GetRoundVotingPower(contractAddr, 0)
	s.Require().Equal(int64(powerAfterSlashing), roundPower)
}
