package interchain

import (
	"fmt"
	"hydro/test/interchain/chainsuite"
	"strconv"
	"strings"
	"testing"

	"cosmossdk.io/math"
	"github.com/strangelove-ventures/interchaintest/v8/testutil"
	"github.com/stretchr/testify/suite"
)

func TestHydroSuite(t *testing.T) {
	s := &HydroSuite{}
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
	dstIbcDenom1 := s.HubToNeutronShareTokenTransfer(0, math.NewInt(400), sourceIbcDenom1, s.NeutronChain.ValidatorWallets[0].Address)
	sourceIbcDenom2 := fmt.Sprintf("%s/%s", strings.ToLower(s.HubChain.ValidatorWallets[1].ValoperAddress), recordId2)
	dstIbcDenom2 := s.HubToNeutronShareTokenTransfer(0, math.NewInt(400), sourceIbcDenom2, s.NeutronChain.ValidatorWallets[0].Address)

	// deploy hydro contract - instantiate code
	contractAddr := s.InstantiateHydroContract(s.NeutronChain.ValidatorWallets[0].Moniker, hydroCodeId, s.NeutronChain.ValidatorWallets[0].Address, 3, 86400000000000)

	// register interchain query
	s.RegisterInterchainQueries([]string{s.HubChain.ValidatorWallets[0].ValoperAddress, s.HubChain.ValidatorWallets[1].ValoperAddress},
		contractAddr, s.NeutronChain.ValidatorWallets[0].Moniker)

	// lock tokens
	err := s.LockTokens(0, 86400000000000, "10", dstIbcDenom1, contractAddr)
	s.Require().NoError(err)
	err = s.LockTokens(0, 3*86400000000000, "10", dstIbcDenom2, contractAddr)
	s.Require().NoError(err)
	err = s.LockTokens(0, 6*86400000000000, "10", dstIbcDenom1, contractAddr)
	s.Require().NoError(err)
	err = s.LockTokens(0, 12*86400000000000, "10", dstIbcDenom2, contractAddr)
	s.Require().NoError(err)

	// Scale lockup power
	// 1x if lockup is between 0 and 1 epochs
	// 1.5x if lockup is between 1 and 3 epochs
	// 2x if lockup is between 3 and 6 epochs
	// 4x if lockup is between 6 and 12 epochs
	votingPower := "85" // 10*1+10*1.5+10*2+10*4

	// create hydro proposals
	err = s.SubmitHydroProposal(0, contractAddr, "tranche 1 prop 1", 1)
	s.Require().NoError(err)
	err = s.SubmitHydroProposal(0, contractAddr, "tranche 1 prop 2", 1)
	s.Require().NoError(err)
	err = s.SubmitHydroProposal(0, contractAddr, "tranche 2 prop 1", 2)
	s.Require().NoError(err)

	// vote for tranche 1 proposal 1
	proposal, err := s.GetProposalByTitle(contractAddr, "tranche 1 prop 1", 1)
	s.Require().NoError(err)
	s.Require().Equal("0", proposal.Power)

	err = s.VoteForHydroProposal(0, contractAddr, proposal.ProposalID, 1)
	s.Require().NoError(err)
	proposal, err = s.GetProposalByTitle(contractAddr, "tranche 1 prop 1", 1)
	s.Require().NoError(err)
	s.Require().Equal(votingPower, proposal.Power)

	// vote for tranche 2 proposal
	proposal, err = s.GetProposalByTitle(contractAddr, "tranche 2 prop 1", 2)
	s.Require().NoError(err)
	s.Require().Equal("0", proposal.Power)

	err = s.VoteForHydroProposal(0, contractAddr, proposal.ProposalID, 2)
	s.Require().NoError(err)
	proposal, err = s.GetProposalByTitle(contractAddr, "tranche 2 prop 1", 2)
	s.Require().NoError(err)
	s.Require().Equal(votingPower, proposal.Power)

	// power of tranche 1 proposal 1 is not changed after voting for proposal from different tranche
	proposal, err = s.GetProposalByTitle(contractAddr, "tranche 1 prop 1", 1)
	s.Require().NoError(err)
	s.Require().Equal(votingPower, proposal.Power)

	// revote for tranche 1 proposal 2
	proposal, err = s.GetProposalByTitle(contractAddr, "tranche 1 prop 2", 1)
	s.Require().NoError(err)
	s.Require().Equal("0", proposal.Power)

	err = s.VoteForHydroProposal(0, contractAddr, proposal.ProposalID, 1)
	s.Require().NoError(err)
	proposal, err = s.GetProposalByTitle(contractAddr, "tranche 1 prop 2", 1)
	s.Require().NoError(err)
	s.Require().Equal(votingPower, proposal.Power)

	// power of tranche 1 proposal 1 is now 0, since we revoted for the proposal 2 from the first tranche
	proposal, err = s.GetProposalByTitle(contractAddr, "tranche 1 prop 1", 1)
	s.Require().NoError(err)
	s.Require().Equal("0", proposal.Power)

	// pausing the contract
	s.PauseTheHydroContract(s.NeutronChain.ValidatorWallets[0].Moniker, contractAddr)
	// confirm that calling contract returns an error
	err = s.LockTokens(0, 86400000000000, "10", dstIbcDenom1, contractAddr)
	s.Require().Error(err)
	s.Require().Contains(err.Error(), "Paused")
	err = s.SubmitHydroProposal(0, contractAddr, "tranche 2 prop 2", 2)
	s.Require().Error(err)
	s.Require().Contains(err.Error(), "Paused")
	err = s.VoteForHydroProposal(0, contractAddr, proposal.ProposalID, 1)
	s.Require().Error(err)
	s.Require().Contains(err.Error(), "Paused")
	err = s.WhitelistAccount(contractAddr, s.NeutronChain.ValidatorWallets[1].Address)
	s.Require().Error(err)
	s.Require().Contains(err.Error(), "Paused")
	err = s.RemoveFromWhitelist(contractAddr, s.NeutronChain.ValidatorWallets[0].Address)
	s.Require().Error(err)
	s.Require().Contains(err.Error(), "Paused")
	err = s.AddTranche(contractAddr, "test", "test")
	s.Require().Error(err)
	s.Require().Contains(err.Error(), "Paused")
	err = s.EditTranche(contractAddr, "test", "test", 1)
	s.Require().Error(err)
	s.Require().Contains(err.Error(), "Paused")
}

// TestActiveValidatorChange tests dropping one validator from the active set, adding a new one, and checks its effect on the proposal voting power
func (s *HydroSuite) TestActiveValidatorChange() {
	hubNode := s.HubChain.GetNode()

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
	dstIbcDenom1 := s.HubToNeutronShareTokenTransfer(0, math.NewInt(400), sourceIbcDenom1, s.NeutronChain.ValidatorWallets[0].Address)
	sourceIbcDenom2 := fmt.Sprintf("%s/%s", strings.ToLower(s.HubChain.ValidatorWallets[1].ValoperAddress), recordId2)
	dstIbcDenom2 := s.HubToNeutronShareTokenTransfer(0, math.NewInt(400), sourceIbcDenom2, s.NeutronChain.ValidatorWallets[0].Address)
	sourceIbcDenom3 := fmt.Sprintf("%s/%s", strings.ToLower(s.HubChain.ValidatorWallets[2].ValoperAddress), recordId3)
	dstIbcDenom3 := s.HubToNeutronShareTokenTransfer(0, math.NewInt(400), sourceIbcDenom3, s.NeutronChain.ValidatorWallets[0].Address)

	// deploy hydro contract - instantiate code
	// active valset consists of 2 validators, currently val1 and val2
	contractAddr := s.InstantiateHydroContract(s.NeutronChain.ValidatorWallets[0].Moniker, hydroCodeId, s.NeutronChain.ValidatorWallets[0].Address, 2, 86400000000000)

	// register interchain query for val1 and val2
	s.RegisterInterchainQueries([]string{s.HubChain.ValidatorWallets[0].ValoperAddress, s.HubChain.ValidatorWallets[1].ValoperAddress},
		contractAddr, s.NeutronChain.ValidatorWallets[0].Moniker)

	// lock tokens
	err := s.LockTokens(0, 86400000000000, "10", dstIbcDenom1, contractAddr)
	s.Require().NoError(err)
	err = s.LockTokens(0, 3*86400000000000, "10", dstIbcDenom2, contractAddr)
	s.Require().NoError(err)

	votingPowerVal1Val1 := "10"
	votingPowerVal1Val2 := "25"
	votingPowerVal1Val3 := "30"

	// create hydro proposals
	err = s.SubmitHydroProposal(0, contractAddr, "tranche 1 prop 1", 1)
	s.Require().NoError(err)

	// vote for tranche 1 proposal 1
	proposal, err := s.GetProposalByTitle(contractAddr, "tranche 1 prop 1", 1)
	s.Require().NoError(err)
	s.Require().Equal("0", proposal.Power)

	err = s.VoteForHydroProposal(0, contractAddr, proposal.ProposalID, 1)
	s.Require().NoError(err)
	proposal, err = s.GetProposalByTitle(contractAddr, "tranche 1 prop 1", 1)
	s.Require().NoError(err)
	s.Require().Equal(votingPowerVal1Val2, proposal.Power)

	// increase stake for val3 on hub, so that val2 drops from active valset and val3 enters
	s.Require().NoError(s.HubChain.UpdateAndVerifyStakeChange(s.GetContext(), s.NeutronChain, s.Relayer, 1_000_000, 2))

	// register icq for val3
	s.RegisterInterchainQueries([]string{s.HubChain.ValidatorWallets[2].ValoperAddress}, contractAddr, s.NeutronChain.ValidatorWallets[0].Moniker)

	// lock token for val3
	err = s.LockTokens(0, 6*86400000000000, "10", dstIbcDenom3, contractAddr)
	s.Require().NoError(err)

	// check that voting power is equal to voting power of val1 because val2 is not among active vals anymore
	proposal, err = s.GetProposalByTitle(contractAddr, "tranche 1 prop 1", 1)
	s.Require().NoError(err)
	s.Require().Equal(votingPowerVal1Val1, proposal.Power)

	// vote again so that val3 power is taken into account
	err = s.VoteForHydroProposal(0, contractAddr, proposal.ProposalID, 1)
	s.Require().NoError(err)
	proposal, err = s.GetProposalByTitle(contractAddr, "tranche 1 prop 1", 1)
	s.Require().NoError(err)
	s.Require().Equal(votingPowerVal1Val3, proposal.Power)
}

// TestValidatorSlashing tests that voting power of user, proposal, round is changed after validator is slashed
func (s *HydroSuite) TestValidatorSlashing() {
	hubNode := s.HubChain.GetNode()

	// delegate tokens
	s.DelegateTokens(hubNode, s.HubChain.ValidatorWallets[0].Moniker, s.HubChain.ValidatorWallets[3].ValoperAddress, txAmountUatom(1000))

	// liquid stake tokens
	recordId1 := s.LiquidStakeTokens(hubNode, s.HubChain.ValidatorWallets[0].Moniker, s.HubChain.ValidatorWallets[3].ValoperAddress,
		s.HubChain.ValidatorWallets[0].Address, txAmountUatom(500))

	// transfer share tokens to neutron chain
	sourceIbcDenom1 := fmt.Sprintf("%s/%s", strings.ToLower(s.HubChain.ValidatorWallets[3].ValoperAddress), recordId1)
	dstIbcDenom1 := s.HubToNeutronShareTokenTransfer(0, math.NewInt(400), sourceIbcDenom1, s.NeutronChain.ValidatorWallets[0].Address)

	// deploy hydro contract - instantiate code
	contractAddr := s.InstantiateHydroContract(s.NeutronChain.ValidatorWallets[0].Moniker, hydroCodeId, s.NeutronChain.ValidatorWallets[0].Address, 4, 86400000000000)

	// register interchain query
	s.RegisterInterchainQueries([]string{s.HubChain.ValidatorWallets[3].ValoperAddress},
		contractAddr, s.NeutronChain.ValidatorWallets[0].Moniker)

	// lock tokens
	lockAmount := "300"
	powerAfterSlashing := 271 // ceil rounding is used, which is why the power after slashing 10% of the tokens is 271 instead of 270
	s.Require().NoError(s.LockTokens(0, 86400000000000, lockAmount, dstIbcDenom1, contractAddr))

	// create hydro proposals
	s.Require().NoError(s.SubmitHydroProposal(0, contractAddr, "tranche 1 prop 1", 1))

	// vote for proposal
	proposal, err := s.GetProposalByTitle(contractAddr, "tranche 1 prop 1", 1)
	s.Require().NoError(err)
	s.Require().Equal("0", proposal.Power)

	s.Require().NoError(s.VoteForHydroProposal(0, contractAddr, proposal.ProposalID, 1))
	proposal, err = s.GetProposalByTitle(contractAddr, "tranche 1 prop 1", 1)
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
	proposal, err = s.GetProposalByTitle(contractAddr, "tranche 1 prop 1", 1)
	s.Require().NoError(err)
	s.Require().Equal(strconv.Itoa(powerAfterSlashing), proposal.Power)

	// check user total voting power
	userPower := s.GetUserVotingPower(contractAddr, s.NeutronChain.ValidatorWallets[0].Address)
	s.Require().Equal(int64(powerAfterSlashing), userPower)

	// check round total voting power
	roundPower := s.GetRoundVotingPower(contractAddr, 0)
	s.Require().Equal(int64(powerAfterSlashing), roundPower)
}

// TestTributeContract tests tribute creation and distribution
func (s *HydroSuite) TestTributeContract() {
	// delegate tokens
	s.DelegateTokens(s.HubChain.Validators[0], s.HubChain.ValidatorWallets[0].Moniker, s.HubChain.ValidatorWallets[0].ValoperAddress, txAmountUatom(10000))

	// liquid stake tokens
	recordId := s.LiquidStakeTokens(s.HubChain.Validators[0], s.HubChain.ValidatorWallets[0].Moniker, s.HubChain.ValidatorWallets[0].ValoperAddress,
		s.HubChain.ValidatorWallets[0].Address, txAmountUatom(5000))

	// transfer share tokens to neutron chain
	sourceIbcDenom := fmt.Sprintf("%s/%s", strings.ToLower(s.HubChain.ValidatorWallets[0].ValoperAddress), recordId)
	dstIbcDenom := s.HubToNeutronShareTokenTransfer(0, math.NewInt(1000), sourceIbcDenom, s.NeutronChain.ValidatorWallets[1].Address)
	s.HubToNeutronShareTokenTransfer(0, math.NewInt(1000), sourceIbcDenom, s.NeutronChain.ValidatorWallets[2].Address)
	s.HubToNeutronShareTokenTransfer(0, math.NewInt(1000), sourceIbcDenom, s.NeutronChain.ValidatorWallets[3].Address)

	// deploy hydro contract - instantiate code
	roundLength := 300000000000
	hydroContractAddr := s.InstantiateHydroContract(s.NeutronChain.ValidatorWallets[0].Moniker, hydroCodeId, s.NeutronChain.ValidatorWallets[0].Address, 4, roundLength)

	// deploy tribute contract - instantiate code
	tributeContractAddr := s.InstantiateTributeContract(tributeCodeId, hydroContractAddr, s.NeutronChain.ValidatorWallets[0].Address)

	// register interchain query
	s.RegisterInterchainQueries([]string{s.HubChain.ValidatorWallets[0].ValoperAddress},
		hydroContractAddr, s.NeutronChain.ValidatorWallets[0].Moniker)

	// lock tokens
	lockAmountVal2 := "800"
	lockAmountVal3 := "400"
	lockAmountVal4 := "200"
	s.Require().NoError(s.LockTokens(1, roundLength, lockAmountVal2, dstIbcDenom, hydroContractAddr))
	s.Require().NoError(s.LockTokens(2, roundLength, lockAmountVal3, dstIbcDenom, hydroContractAddr))
	s.Require().NoError(s.LockTokens(3, roundLength, lockAmountVal4, dstIbcDenom, hydroContractAddr))

	// validator 1 creates hydro proposals
	s.Require().NoError(s.SubmitHydroProposal(0, hydroContractAddr, "tranche 1 prop 1", 1))
	s.Require().NoError(s.SubmitHydroProposal(0, hydroContractAddr, "tranche 1 prop 2", 1))
	s.Require().NoError(s.SubmitHydroProposal(0, hydroContractAddr, "tranche 1 prop 3", 1))
	s.Require().NoError(s.SubmitHydroProposal(0, hydroContractAddr, "tranche 1 prop 4", 1))

	proposal1, err := s.GetProposalByTitle(hydroContractAddr, "tranche 1 prop 1", 1)
	s.Require().NoError(err)
	s.Require().Equal("0", proposal1.Power)
	proposal2, err := s.GetProposalByTitle(hydroContractAddr, "tranche 1 prop 2", 1)
	s.Require().NoError(err)
	s.Require().Equal("0", proposal2.Power)
	proposal3, err := s.GetProposalByTitle(hydroContractAddr, "tranche 1 prop 3", 1)
	s.Require().NoError(err)
	s.Require().Equal("0", proposal3.Power)
	proposal4, err := s.GetProposalByTitle(hydroContractAddr, "tranche 1 prop 4", 1)
	s.Require().NoError(err)
	s.Require().Equal("0", proposal4.Power)

	roundId := s.GetCurrentRound(hydroContractAddr) // all proposals are expected to be submitted in the same round

	// validator 1 adds tribute for proposals
	tribute1Id, err := s.SubmitTribute(0, 10000, proposal1.TrancheID, proposal1.ProposalID, tributeContractAddr)
	s.Require().NoError(err)
	tribute2Id, err := s.SubmitTribute(0, 20000, proposal2.TrancheID, proposal2.ProposalID, tributeContractAddr)
	s.Require().NoError(err)
	tribute3Id, err := s.SubmitTribute(0, 30000, proposal3.TrancheID, proposal3.ProposalID, tributeContractAddr)
	s.Require().NoError(err)
	tribute4Id, err := s.SubmitTribute(3, 40000, proposal4.TrancheID, proposal4.ProposalID, tributeContractAddr)
	s.Require().NoError(err)

	// val2 votes for proposal 1
	s.Require().NoError(s.VoteForHydroProposal(1, hydroContractAddr, proposal1.ProposalID, int64(proposal1.TrancheID)))
	proposal1, err = s.GetProposalByTitle(hydroContractAddr, "tranche 1 prop 1", 1)
	s.Require().NoError(err)
	s.Require().Equal(lockAmountVal2, proposal1.Power)

	// val3 votes for proposal 2
	s.Require().NoError(s.VoteForHydroProposal(2, hydroContractAddr, proposal2.ProposalID, int64(proposal2.TrancheID)))
	proposal2, err = s.GetProposalByTitle(hydroContractAddr, "tranche 1 prop 2", 1)
	s.Require().NoError(err)
	s.Require().Equal(lockAmountVal3, proposal2.Power)

	// val4 votes for proposal 3
	s.Require().NoError(s.VoteForHydroProposal(3, hydroContractAddr, proposal3.ProposalID, int64(proposal3.TrancheID)))
	proposal3, err = s.GetProposalByTitle(hydroContractAddr, "tranche 1 prop 3", 1)
	s.Require().NoError(err)
	s.Require().Equal(lockAmountVal4, proposal3.Power)

	// balance of the accounts before the round is finished
	oldBalanceVal1, err := s.NeutronChain.GetBalance(s.GetContext(), s.NeutronChain.ValidatorWallets[0].Address, chainsuite.Untrn)
	s.Require().NoError(err)
	oldBalanceVal2, err := s.NeutronChain.GetBalance(s.GetContext(), s.NeutronChain.ValidatorWallets[1].Address, chainsuite.Untrn)
	s.Require().NoError(err)
	oldBalanceVal3, err := s.NeutronChain.GetBalance(s.GetContext(), s.NeutronChain.ValidatorWallets[2].Address, chainsuite.Untrn)
	s.Require().NoError(err)
	oldBalanceVal4, err := s.NeutronChain.GetBalance(s.GetContext(), s.NeutronChain.ValidatorWallets[3].Address, chainsuite.Untrn)
	s.Require().NoError(err)

	// verify that reward cannot be claimed nor refunded before the round is finished
	err = s.ClaimTribute(0, tributeContractAddr, s.NeutronChain.ValidatorWallets[1].Address, roundId, proposal1.TrancheID, tribute1Id)
	s.Require().Error(err)
	s.Require().Contains(err.Error(), "Round has not ended yet")
	err = s.ClaimTribute(0, tributeContractAddr, s.NeutronChain.ValidatorWallets[2].Address, roundId, proposal2.TrancheID, tribute2Id)
	s.Require().Error(err)
	s.Require().Contains(err.Error(), "Round has not ended yet")
	err = s.ClaimTribute(0, tributeContractAddr, s.NeutronChain.ValidatorWallets[3].Address, roundId, proposal3.TrancheID, tribute3Id)
	s.Require().Error(err)
	s.Require().Contains(err.Error(), "Round has not ended yet")
	err = s.RefundTribute(3, tributeContractAddr, roundId, proposal4.TrancheID, tribute4Id, proposal4.ProposalID)
	s.Require().Error(err)
	s.Require().Contains(err.Error(), "Round has not ended yet")

	// wait for new round to start, so that we can claim tribute rewards gained in previous round
	s.WaitForRound(hydroContractAddr, roundId+1)

	// verify that top N proposal cannot be refunded
	err = s.RefundTribute(0, tributeContractAddr, roundId, proposal1.TrancheID, tribute1Id, proposal1.ProposalID)
	s.Require().Error(err)
	// claim reward for top N proposal
	err = s.ClaimTribute(0, tributeContractAddr, s.NeutronChain.ValidatorWallets[1].Address, roundId, proposal1.TrancheID, tribute1Id)
	s.Require().NoError(err)
	// claim reward for top N proposal
	err = s.ClaimTribute(0, tributeContractAddr, s.NeutronChain.ValidatorWallets[2].Address, roundId, proposal2.TrancheID, tribute2Id)
	s.Require().NoError(err)
	// proposal out of top N proposal cannot be claimed
	err = s.ClaimTribute(0, tributeContractAddr, s.NeutronChain.ValidatorWallets[3].Address, roundId, proposal3.TrancheID, tribute3Id)
	s.Require().Error(err)
	s.Require().Contains(err.Error(), "outside of top N proposals")
	// refund tribute for proposal that is not in top N
	err = s.RefundTribute(0, tributeContractAddr, roundId, proposal3.TrancheID, tribute3Id, proposal3.ProposalID)
	s.Require().NoError(err)
	// refund tribute for the proposal that has no votes at all
	err = s.RefundTribute(3, tributeContractAddr, roundId, proposal4.TrancheID, tribute4Id, proposal4.ProposalID)
	s.Require().NoError(err)

	// verify that the same proposal cannot be claimed twice
	err = s.ClaimTribute(0, tributeContractAddr, s.NeutronChain.ValidatorWallets[1].Address, roundId, proposal1.TrancheID, tribute1Id)
	s.Require().Error(err)
	// verify that same proposal cannot be refunded twice
	err = s.RefundTribute(0, tributeContractAddr, roundId, proposal3.TrancheID, tribute3Id, proposal3.ProposalID)
	s.Require().Error(err)
	// refunded proposal cannot be claimed
	err = s.ClaimTribute(0, tributeContractAddr, s.NeutronChain.ValidatorWallets[1].Address, roundId, proposal3.TrancheID, tribute3Id)
	s.Require().Error(err)

	newBalanceVal1, err := s.NeutronChain.GetBalance(s.GetContext(), s.NeutronChain.ValidatorWallets[0].Address, chainsuite.Untrn)
	s.Require().NoError(err)
	newBalanceVal2, err := s.NeutronChain.GetBalance(s.GetContext(), s.NeutronChain.ValidatorWallets[1].Address, chainsuite.Untrn)
	s.Require().NoError(err)
	newBalanceVal3, err := s.NeutronChain.GetBalance(s.GetContext(), s.NeutronChain.ValidatorWallets[2].Address, chainsuite.Untrn)
	s.Require().NoError(err)
	newBalanceVal4, err := s.NeutronChain.GetBalance(s.GetContext(), s.NeutronChain.ValidatorWallets[3].Address, chainsuite.Untrn)
	s.Require().NoError(err)

	// proposal power after voting: proposal_1=800, proposal_2: 400, proposal_3: 200,  proposal_4: 0
	// proposal 1 and 2 are in top N proposals(N=2), which means that only those voters got the tribute reward and tribute from the other proposals can be refunded
	s.Require().True(newBalanceVal2.Sub(oldBalanceVal2).Equal(math.NewInt(9000)))  // reward is tribute for proposal1 - communityTax = 10000-10%=9000
	s.Require().True(newBalanceVal3.Sub(oldBalanceVal3).Equal(math.NewInt(18000))) // reward is tribute for proposal2 - communityTax = 20000-10%=18000
	s.Require().True(newBalanceVal1.GT(oldBalanceVal1))                            // refunded proposal3
	s.Require().True(newBalanceVal4.GT(oldBalanceVal4))                            // refunded proposal4
}
