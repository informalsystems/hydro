package interchain

import (
	"fmt"
	"log"

	"strconv"
	"strings"
	"testing"

	"time"

	"hydro/test/interchain/chainsuite"

	"cosmossdk.io/math"
	sdk "github.com/cosmos/cosmos-sdk/types"
	"github.com/strangelove-ventures/interchaintest/v8/testutil"
	"github.com/stretchr/testify/suite"
)

const (
	DefaultDeploymentDuration  = 1
	DefaultMinLiquidityRequest = 100000000
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
func (s *HydroSuite) TestHappyPath() {
	log.Println("==== Running happy path test")
	hubNode := s.HubChain.GetNode()

	// delegate tokens
	log.Println("==== Delegating tokens")
	s.DelegateTokens(hubNode, s.HubChain.ValidatorWallets[0].Moniker, s.HubChain.ValidatorWallets[0].ValoperAddress, txAmountUatom(1000))
	s.DelegateTokens(hubNode, s.HubChain.ValidatorWallets[0].Moniker, s.HubChain.ValidatorWallets[1].ValoperAddress, txAmountUatom(1000))

	// liquid stake tokens
	log.Println("==== Tokenizing shares")
	recordId1 := s.LiquidStakeTokens(hubNode, s.HubChain.ValidatorWallets[0].Moniker, s.HubChain.ValidatorWallets[0].ValoperAddress,
		s.HubChain.ValidatorWallets[0].Address, txAmountUatom(500))
	recordId2 := s.LiquidStakeTokens(hubNode, s.HubChain.ValidatorWallets[0].Moniker, s.HubChain.ValidatorWallets[1].ValoperAddress,
		s.HubChain.ValidatorWallets[0].Address, txAmountUatom(500))

	// transfer share tokens to neutron chain
	log.Println("==== Transferring tokenized shares to Neutron")
	sourceIbcDenom1 := fmt.Sprintf("%s/%s", strings.ToLower(s.HubChain.ValidatorWallets[0].ValoperAddress), recordId1)
	dstIbcDenom1 := s.HubToNeutronShareTokenTransfer(0, math.NewInt(400), sourceIbcDenom1, s.NeutronChain.ValidatorWallets[0].Address)
	sourceIbcDenom2 := fmt.Sprintf("%s/%s", strings.ToLower(s.HubChain.ValidatorWallets[1].ValoperAddress), recordId2)
	dstIbcDenom2 := s.HubToNeutronShareTokenTransfer(0, math.NewInt(400), sourceIbcDenom2, s.NeutronChain.ValidatorWallets[0].Address)

	// deploy hydro contract - instantiate code
	contractAddr := s.InstantiateHydroContract(s.NeutronChain.ValidatorWallets[0].Moniker, hydroCodeId, s.NeutronChain.ValidatorWallets[0].Address, 3, 86400000000000)

	// register interchain query
	log.Println("==== Registering interchain queries")
	s.RegisterInterchainQueries([]string{s.HubChain.ValidatorWallets[0].ValoperAddress, s.HubChain.ValidatorWallets[1].ValoperAddress},
		contractAddr, s.NeutronChain.ValidatorWallets[0].Moniker)

	// lock tokens
	log.Println("==== Locking tokens in Hydro")
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
	log.Println("==== Creating proposals")
	err = s.SubmitHydroProposal(0, contractAddr, "tranche 1 prop 1", 1, DefaultDeploymentDuration, DefaultMinLiquidityRequest)
	s.Require().NoError(err)
	err = s.SubmitHydroProposal(0, contractAddr, "tranche 1 prop 2", 1, DefaultDeploymentDuration, DefaultMinLiquidityRequest)
	s.Require().NoError(err)
	err = s.SubmitHydroProposal(0, contractAddr, "tranche 2 prop 1", 2, DefaultDeploymentDuration, DefaultMinLiquidityRequest)
	s.Require().NoError(err)

	log.Println("==== Voting for proposals")
	// vote for tranche 1 proposal 1
	proposal, err := s.GetProposalByTitle(contractAddr, "tranche 1 prop 1", 1)
	s.Require().NoError(err)
	s.Require().Equal("0", proposal.Power)

	proposalsVotes := []ProposalToLockups{
		{
			ProposalId: proposal.ProposalID,
			LockIds:    []int{0, 1, 2, 3},
		},
	}
	err = s.VoteForHydroProposal(0, contractAddr, 1, proposalsVotes)
	s.Require().NoError(err)
	proposal, err = s.GetProposalByTitle(contractAddr, "tranche 1 prop 1", 1)
	s.Require().NoError(err)
	s.Require().Equal(votingPower, proposal.Power)

	// vote for tranche 2 proposal
	proposal, err = s.GetProposalByTitle(contractAddr, "tranche 2 prop 1", 2)
	s.Require().NoError(err)
	s.Require().Equal("0", proposal.Power)

	// vote for proposal in tranche 2 with the same lockups
	proposalsVotes[0].ProposalId = proposal.ProposalID
	err = s.VoteForHydroProposal(0, contractAddr, 2, proposalsVotes)
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

	// vote for proposal 2 in tranche 1 with the same lockups
	proposalsVotes[0].ProposalId = proposal.ProposalID
	err = s.VoteForHydroProposal(0, contractAddr, 1, proposalsVotes)
	s.Require().NoError(err)
	proposal, err = s.GetProposalByTitle(contractAddr, "tranche 1 prop 2", 1)
	s.Require().NoError(err)
	s.Require().Equal(votingPower, proposal.Power)

	// power of tranche 1 proposal 1 is now 0, since we revoted for the proposal 2 from the first tranche
	proposal, err = s.GetProposalByTitle(contractAddr, "tranche 1 prop 1", 1)
	s.Require().NoError(err)
	s.Require().Equal("0", proposal.Power)
}

func (s *HydroSuite) TestPauseContract() {
	log.Println("==== Running pause contract test")

	// delegate tokens
	log.Println("==== Delegating tokens")
	s.DelegateTokens(s.HubChain.Validators[0], s.HubChain.ValidatorWallets[0].Moniker, s.HubChain.ValidatorWallets[0].ValoperAddress, txAmountUatom(1000))

	// liquid stake tokens
	log.Println("==== Tokenizing shares")
	recordId := s.LiquidStakeTokens(s.HubChain.Validators[0], s.HubChain.ValidatorWallets[0].Moniker, s.HubChain.ValidatorWallets[0].ValoperAddress, s.HubChain.ValidatorWallets[0].Address, txAmountUatom(500))

	// transfer share tokens to neutron chain
	log.Println("==== Transferring tokenized shares to Neutron")
	sourceIbcDenom := fmt.Sprintf("%s/%s", strings.ToLower(s.HubChain.ValidatorWallets[0].ValoperAddress), recordId)
	dstIbcDenom := s.HubToNeutronShareTokenTransfer(0, math.NewInt(400), sourceIbcDenom, s.NeutronChain.ValidatorWallets[0].Address)

	// instantiate hydro contract
	contractAddr := s.InstantiateHydroContract(s.NeutronChain.ValidatorWallets[0].Moniker, hydroCodeId, s.NeutronChain.ValidatorWallets[0].Address, 2, 86400000000000)

	// pause the contract
	log.Println("==== Pausing contract")
	s.PauseTheHydroContract(s.NeutronChain.ValidatorWallets[0].Moniker, contractAddr)
	// confirm that calling contract returns an error
	log.Println("==== Confirming contract is paused")

	err := s.LockTokens(0, 100000000000, "10", dstIbcDenom, contractAddr)
	RequirePaused(s, err)
	err = s.UnlockTokens(contractAddr)
	RequirePaused(s, err)
	err = s.RefreshLock(contractAddr, 0, 0)
	RequirePaused(s, err)

	err = s.SubmitHydroProposal(0, contractAddr, "tranche 2 prop 2", 2, DefaultDeploymentDuration, DefaultMinLiquidityRequest)
	RequirePaused(s, err)
	err = s.VoteForHydroProposal(0, contractAddr, 1, []ProposalToLockups{})
	RequirePaused(s, err)

	err = s.WhitelistAccount(contractAddr, s.NeutronChain.ValidatorWallets[1].Address)
	RequirePaused(s, err)
	err = s.RemoveFromWhitelist(contractAddr, s.NeutronChain.ValidatorWallets[0].Address)
	RequirePaused(s, err)

	err = s.AddTranche(contractAddr, "test", "test")
	RequirePaused(s, err)
	err = s.EditTranche(contractAddr, "test", "test", 1)
	RequirePaused(s, err)

	err = s.AddICQManager(contractAddr, s.NeutronChain.ValidatorWallets[1].Address)
	RequirePaused(s, err)
	err = s.RemoveICQManager(contractAddr, s.NeutronChain.ValidatorWallets[1].Address)
	RequirePaused(s, err)

	err = s.UpdateMaxLockedTokens(contractAddr, 100000000000, time.Now().UTC().Add(time.Hour).UnixNano())
	RequirePaused(s, err)
}

func RequirePaused(s *HydroSuite, err error) {
	s.Require().Error(err)
	s.Require().Contains(err.Error(), "Paused")
}

// TestActiveValidatorChange tests dropping one validator from the active set, adding a new one, and checks its effect on the proposal voting power
func (s *HydroSuite) TestActiveValidatorChange() {
	log.Println("==== Running active validator change test")
	hubNode := s.HubChain.GetNode()

	// val1 delegate tokens to validator 1(self delegate), 2 and 3
	log.Println("==== Delegating tokens")
	s.DelegateTokens(hubNode, s.HubChain.ValidatorWallets[0].Moniker, s.HubChain.ValidatorWallets[0].ValoperAddress, txAmountUatom(1000))
	s.DelegateTokens(hubNode, s.HubChain.ValidatorWallets[0].Moniker, s.HubChain.ValidatorWallets[1].ValoperAddress, txAmountUatom(1000))
	s.DelegateTokens(hubNode, s.HubChain.ValidatorWallets[0].Moniker, s.HubChain.ValidatorWallets[2].ValoperAddress, txAmountUatom(1000))

	// liquid stake tokens
	log.Println("==== Tokenizing shares")
	recordId1 := s.LiquidStakeTokens(hubNode, s.HubChain.ValidatorWallets[0].Moniker, s.HubChain.ValidatorWallets[0].ValoperAddress,
		s.HubChain.ValidatorWallets[0].Address, txAmountUatom(500))
	recordId2 := s.LiquidStakeTokens(hubNode, s.HubChain.ValidatorWallets[0].Moniker, s.HubChain.ValidatorWallets[1].ValoperAddress,
		s.HubChain.ValidatorWallets[0].Address, txAmountUatom(500))
	recordId3 := s.LiquidStakeTokens(hubNode, s.HubChain.ValidatorWallets[0].Moniker, s.HubChain.ValidatorWallets[2].ValoperAddress,
		s.HubChain.ValidatorWallets[0].Address, txAmountUatom(500))

	// transfer share tokens to neutron chain
	log.Println("==== Transferring tokenized shares to Neutron")
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
	log.Println("==== Registering interchain queries for val1 and val2")
	s.RegisterInterchainQueries([]string{s.HubChain.ValidatorWallets[0].ValoperAddress, s.HubChain.ValidatorWallets[1].ValoperAddress},
		contractAddr, s.NeutronChain.ValidatorWallets[0].Moniker)

	// lock tokens
	log.Println("==== Locking tokens in Hydro")
	err := s.LockTokens(0, 86400000000000, "10", dstIbcDenom1, contractAddr)
	s.Require().NoError(err)
	err = s.LockTokens(0, 3*86400000000000, "10", dstIbcDenom2, contractAddr)
	s.Require().NoError(err)

	votingPowerVal1Denom := "10"
	votingPowerVal1Val2Denoms := "25"
	votingPowerVal1Val3Denoms := "30"

	// create hydro proposals
	log.Println("==== Creating proposals")
	err = s.SubmitHydroProposal(0, contractAddr, "tranche 1 prop 1", 1, DefaultDeploymentDuration, DefaultMinLiquidityRequest)
	s.Require().NoError(err)

	log.Println("==== Voting for proposals")
	// vote for tranche 1 proposal 1
	proposal, err := s.GetProposalByTitle(contractAddr, "tranche 1 prop 1", 1)
	s.Require().NoError(err)
	s.Require().Equal("0", proposal.Power)

	proposalsVotes := []ProposalToLockups{
		{
			ProposalId: proposal.ProposalID,
			LockIds:    []int{0, 1},
		},
	}
	err = s.VoteForHydroProposal(0, contractAddr, 1, proposalsVotes)
	s.Require().NoError(err)
	proposal, err = s.GetProposalByTitle(contractAddr, "tranche 1 prop 1", 1)
	s.Require().NoError(err)
	s.Require().Equal(votingPowerVal1Val2Denoms, proposal.Power)

	// increase stake for val3 on hub, so that val2 drops from active valset and val3 enters
	log.Println("==== Increasing stake for val3 to drop val2 from active valset")
	s.Require().NoError(s.HubChain.UpdateAndVerifyStakeChange(s.GetContext(), s.NeutronChain, s.Relayer, 1_000_000, 2))

	// register icq for val3
	log.Println("==== Registering interchain query for val3")
	s.RegisterInterchainQueries([]string{s.HubChain.ValidatorWallets[2].ValoperAddress}, contractAddr, s.NeutronChain.ValidatorWallets[0].Moniker)

	// check that voting power is equal to voting power of val1 because val2 is not among active vals anymore
	proposal, err = s.GetProposalByTitle(contractAddr, "tranche 1 prop 1", 1)
	s.Require().NoError(err)
	s.Require().Equal(votingPowerVal1Denom, proposal.Power)

	// lock token for val3
	log.Println("==== Locking tokens for val3")
	err = s.LockTokens(0, 6*86400000000000, "10", dstIbcDenom3, contractAddr)
	s.Require().NoError(err)

	// proposal power is increased with val3 shares right after the locking
	proposal, err = s.GetProposalByTitle(contractAddr, "tranche 1 prop 1", 1)
	s.Require().NoError(err)
	s.Require().Equal(votingPowerVal1Val3Denoms, proposal.Power)
}

// TestValidatorSlashing tests that voting power of user, proposal, round is changed after validator is slashed
func (s *HydroSuite) TestValidatorSlashing() {
	log.Println("==== Running validator slashing test")

	hubNode := s.HubChain.GetNode()

	// delegate tokens
	log.Println("==== Delegating tokens")
	s.DelegateTokens(hubNode, s.HubChain.ValidatorWallets[0].Moniker, s.HubChain.ValidatorWallets[3].ValoperAddress, txAmountUatom(1000))

	// liquid stake tokens
	log.Println("==== Tokenizing shares")
	recordId1 := s.LiquidStakeTokens(hubNode, s.HubChain.ValidatorWallets[0].Moniker, s.HubChain.ValidatorWallets[3].ValoperAddress,
		s.HubChain.ValidatorWallets[0].Address, txAmountUatom(500))

	// transfer share tokens to neutron chain
	log.Println("==== Transferring tokenized shares to Neutron")
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
	s.Require().NoError(s.SubmitHydroProposal(0, contractAddr, "tranche 1 prop 1", 1, DefaultDeploymentDuration, DefaultMinLiquidityRequest))

	// vote for proposal
	proposal, err := s.GetProposalByTitle(contractAddr, "tranche 1 prop 1", 1)
	s.Require().NoError(err)
	s.Require().Equal("0", proposal.Power)

	proposalsVotes := []ProposalToLockups{
		{
			ProposalId: proposal.ProposalID,
			LockIds:    []int{0},
		},
	}
	s.Require().NoError(s.VoteForHydroProposal(0, contractAddr, 1, proposalsVotes))
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
	log.Println("==== Running tribute test")

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
	s.Require().NoError(s.SubmitHydroProposal(0, hydroContractAddr, "tranche 1 prop 1", 1, DefaultDeploymentDuration, DefaultMinLiquidityRequest))
	s.Require().NoError(s.SubmitHydroProposal(0, hydroContractAddr, "tranche 1 prop 2", 1, DefaultDeploymentDuration, DefaultMinLiquidityRequest))
	s.Require().NoError(s.SubmitHydroProposal(0, hydroContractAddr, "tranche 1 prop 3", 1, DefaultDeploymentDuration, DefaultMinLiquidityRequest))
	s.Require().NoError(s.SubmitHydroProposal(0, hydroContractAddr, "tranche 1 prop 4", 1, DefaultDeploymentDuration, DefaultMinLiquidityRequest))

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

	// validator 1 adds tribute for the first three proposals
	tribute1Id, err := s.SubmitTribute(0, 10000, proposal1.RoundID, proposal1.TrancheID, proposal1.ProposalID, tributeContractAddr)
	s.Require().NoError(err)
	tribute2Id, err := s.SubmitTribute(0, 20000, proposal2.RoundID, proposal2.TrancheID, proposal2.ProposalID, tributeContractAddr)
	s.Require().NoError(err)
	tribute3Id, err := s.SubmitTribute(0, 30000, proposal3.RoundID, proposal3.TrancheID, proposal3.ProposalID, tributeContractAddr)
	s.Require().NoError(err)

	// validator 4 adds tribute for the fourth proposal
	tribute4Id, err := s.SubmitTribute(3, 40000, proposal4.RoundID, proposal4.TrancheID, proposal4.ProposalID, tributeContractAddr)
	s.Require().NoError(err)

	// val2 votes for proposal 1
	proposalsVotes := []ProposalToLockups{
		{
			ProposalId: proposal1.ProposalID,
			LockIds:    []int{0},
		},
	}
	s.Require().NoError(s.VoteForHydroProposal(1, hydroContractAddr, int64(proposal1.TrancheID), proposalsVotes))
	proposal1, err = s.GetProposalByTitle(hydroContractAddr, "tranche 1 prop 1", 1)
	s.Require().NoError(err)
	s.Require().Equal(lockAmountVal2, proposal1.Power)

	// val3 votes for proposal 2
	proposalsVotes = []ProposalToLockups{
		{
			ProposalId: proposal2.ProposalID,
			LockIds:    []int{1},
		},
	}
	s.Require().NoError(s.VoteForHydroProposal(2, hydroContractAddr, int64(proposal2.TrancheID), proposalsVotes))
	proposal2, err = s.GetProposalByTitle(hydroContractAddr, "tranche 1 prop 2", 1)
	s.Require().NoError(err)
	s.Require().Equal(lockAmountVal3, proposal2.Power)

	// val4 votes for proposal 3
	proposalsVotes = []ProposalToLockups{
		{
			ProposalId: proposal3.ProposalID,
			LockIds:    []int{2},
		},
	}
	s.Require().NoError(s.VoteForHydroProposal(3, hydroContractAddr, int64(proposal3.TrancheID), proposalsVotes))
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

	// verify that tributes can not be claimed nor refunded until information about liquidity deployment is stored in the hydro contract
	err = s.ClaimTribute(0, tributeContractAddr, s.NeutronChain.ValidatorWallets[1].Address, roundId, proposal1.TrancheID, tribute1Id)
	s.Require().Error(err)
	s.Require().Contains(err.Error(), "Tribute not claimable: Proposal did not have a liquidity deployment entered")

	err = s.RefundTribute(3, tributeContractAddr, roundId, proposal4.TrancheID, tribute4Id, proposal4.ProposalID)
	s.Require().Error(err)
	s.Require().Contains(err.Error(), "Can't refund tribute for proposal that didn't have a liquidity deployment entered")

	// enter the liquidity deployment info for each proposal from the previous round
	liquidityDeployments := []struct {
		TrancheID     int
		ProposalID    int
		DeployedFunds sdk.Coin
	}{
		{
			TrancheID:  proposal1.TrancheID,
			ProposalID: proposal1.ProposalID,
			DeployedFunds: sdk.Coin{
				Amount: math.NewInt(100000000),
				Denom:  "uatom",
			},
		},
		{
			TrancheID:  proposal2.TrancheID,
			ProposalID: proposal2.ProposalID,
			DeployedFunds: sdk.Coin{
				Amount: math.NewInt(100000000),
				Denom:  "uatom",
			},
		},
		{
			TrancheID:  proposal3.TrancheID,
			ProposalID: proposal3.ProposalID,
			DeployedFunds: sdk.Coin{
				Amount: math.NewInt(0),
				Denom:  "uatom",
			},
		},
		{
			TrancheID:  proposal4.TrancheID,
			ProposalID: proposal4.ProposalID,
			DeployedFunds: sdk.Coin{
				Amount: math.NewInt(0),
				Denom:  "uatom",
			},
		},
	}

	for _, liquidityDeployment := range liquidityDeployments {
		err = s.AddLiquidityDeployment(
			0,
			hydroContractAddr,
			roundId,
			liquidityDeployment.TrancheID,
			liquidityDeployment.ProposalID,
			liquidityDeployment.DeployedFunds)
		s.Require().NoError(err)
	}

	// verify that proposal that received liquidity cannot be refunded
	err = s.RefundTribute(0, tributeContractAddr, roundId, proposal1.TrancheID, tribute1Id, proposal1.ProposalID)
	s.Require().Error(err)
	s.Require().Contains(err.Error(), "Can't refund tribute for proposal that received a non-zero liquidity deployment")
	// claim reward for proposal that received liquidity
	err = s.ClaimTribute(0, tributeContractAddr, s.NeutronChain.ValidatorWallets[1].Address, roundId, proposal1.TrancheID, tribute1Id)
	s.Require().NoError(err)
	// claim reward for proposal that received liquidity
	err = s.ClaimTribute(0, tributeContractAddr, s.NeutronChain.ValidatorWallets[2].Address, roundId, proposal2.TrancheID, tribute2Id)
	s.Require().NoError(err)
	// can not claim tribute for proposal that didn't receive any liquidity
	err = s.ClaimTribute(0, tributeContractAddr, s.NeutronChain.ValidatorWallets[3].Address, roundId, proposal3.TrancheID, tribute3Id)
	s.Require().Error(err)
	s.Require().Contains(err.Error(), "Tribute not claimable: Proposal did not receive a non-zero liquidity deployment")
	// refund tribute for proposal that received votes, but didn't receive any liquidity
	err = s.RefundTribute(0, tributeContractAddr, roundId, proposal3.TrancheID, tribute3Id, proposal3.ProposalID)
	s.Require().NoError(err)
	// refund tribute for the proposal that has no votes at all, and didn't receive any liquidity
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
	// proposal 1 and 2 received the liquidity, which means that only those voters get the tribute reward and tribute from the other proposals can be refunded
	s.Require().True(newBalanceVal2.Sub(oldBalanceVal2).Equal(math.NewInt(10000))) // reward is tribute for proposal1 = 10000
	s.Require().True(newBalanceVal3.Sub(oldBalanceVal3).Equal(math.NewInt(20000))) // reward is tribute for proposal2 = 20000
	s.Require().True(newBalanceVal1.GT(oldBalanceVal1))                            // refunded proposal3
	s.Require().True(newBalanceVal4.GT(oldBalanceVal4))                            // refunded proposal4

	// verify that we can add a tribute to proposals even after the round has ended
	tribute5Id, err := s.SubmitTribute(0, 50000, proposal1.RoundID, proposal1.TrancheID, proposal1.ProposalID, tributeContractAddr)
	s.Require().NoError(err)
	tribute6Id, err := s.SubmitTribute(0, 50000, proposal3.RoundID, proposal3.TrancheID, proposal3.ProposalID, tributeContractAddr)
	s.Require().NoError(err)

	// users can claim immediately, since the voting period for the proposal is over
	// expect no error when claiming the tribute for prop 1
	err = s.ClaimTribute(0, tributeContractAddr, s.NeutronChain.ValidatorWallets[1].Address, proposal4.RoundID, proposal4.TrancheID, tribute5Id)
	s.Require().NoError(err)
	// expect an error when claiming the tribute for prop 4, since it it didn't get any liquidity deployed
	err = s.ClaimTribute(0, tributeContractAddr, s.NeutronChain.ValidatorWallets[3].Address, proposal4.RoundID, proposal4.TrancheID, tribute6Id)
	s.Require().Error(err)
	s.Require().Contains(err.Error(), "Tribute not claimable: Proposal did not receive a non-zero liquidity deployment")

	// also, tributes can be refund immediately, since the voting period for the proposal is over
	// expect an error refunding tribute 5 since it was for a proposal that received some liquidity
	err = s.RefundTribute(0, tributeContractAddr, proposal4.RoundID, proposal4.TrancheID, tribute5Id, proposal1.ProposalID)
	s.Require().Error(err)

	// expect no error when refunding tribute 6 since it was for a proposal that didn't receive any liquidity
	err = s.RefundTribute(0, tributeContractAddr, proposal4.RoundID, proposal4.TrancheID, tribute6Id, proposal3.ProposalID)
	s.Require().NoError(err)
}

// TestDaoVotingAdapter tests:
// deployment of Hydro contract
// deployment of DAO Voting Adapter contract
// locking of tokens by two different users in Hydro contract
// querying historical voting power on the DAO Voting Adapter contract
func (s *HydroSuite) TestDaoVotingAdapter() {
	log.Println("==== Running DAO voting adapter test")
	hubNode0 := s.HubChain.Validators[0]
	hubNode1 := s.HubChain.Validators[1]

	// delegate tokens
	log.Println("==== Delegating tokens")
	s.DelegateTokens(hubNode0, s.HubChain.ValidatorWallets[0].Moniker, s.HubChain.ValidatorWallets[0].ValoperAddress, txAmountUatom(1000))
	s.DelegateTokens(hubNode1, s.HubChain.ValidatorWallets[1].Moniker, s.HubChain.ValidatorWallets[1].ValoperAddress, txAmountUatom(1000))

	// liquid stake tokens
	log.Println("==== Tokenizing shares")
	recordId1 := s.LiquidStakeTokens(hubNode0, s.HubChain.ValidatorWallets[0].Moniker, s.HubChain.ValidatorWallets[0].ValoperAddress,
		s.HubChain.ValidatorWallets[0].Address, txAmountUatom(500))
	recordId2 := s.LiquidStakeTokens(hubNode1, s.HubChain.ValidatorWallets[1].Moniker, s.HubChain.ValidatorWallets[1].ValoperAddress,
		s.HubChain.ValidatorWallets[1].Address, txAmountUatom(500))

	// transfer share tokens to neutron chain
	log.Println("==== Transferring tokenized shares to Neutron")
	sourceIbcDenom1 := fmt.Sprintf("%s/%s", strings.ToLower(s.HubChain.ValidatorWallets[0].ValoperAddress), recordId1)
	dstIbcDenom1 := s.HubToNeutronShareTokenTransfer(0, math.NewInt(400), sourceIbcDenom1, s.NeutronChain.ValidatorWallets[0].Address)
	sourceIbcDenom2 := fmt.Sprintf("%s/%s", strings.ToLower(s.HubChain.ValidatorWallets[1].ValoperAddress), recordId2)
	dstIbcDenom2 := s.HubToNeutronShareTokenTransfer(1, math.NewInt(400), sourceIbcDenom2, s.NeutronChain.ValidatorWallets[1].Address)

	// instantiate hydro contract
	log.Println("==== Instantiating Hydro contract")
	hydroContractAddr := s.InstantiateHydroContract(s.NeutronChain.ValidatorWallets[0].Moniker, hydroCodeId, s.NeutronChain.ValidatorWallets[0].Address, 3, 86400000000000)

	// instantiate DAO voting power adapter contract
	log.Println("==== Instantiating DAO Voting Power Adapter contract")
	daoVotingAdapterContractAddr := s.InstantiateDaoVotingAdapterContract(daoVotingAdapterCodeId, hydroContractAddr, s.NeutronChain.ValidatorWallets[0].Address)

	// register interchain query
	log.Println("==== Registering interchain queries")
	s.RegisterInterchainQueries([]string{s.HubChain.ValidatorWallets[0].ValoperAddress, s.HubChain.ValidatorWallets[1].ValoperAddress},
		hydroContractAddr, s.NeutronChain.ValidatorWallets[0].Moniker)

	// first user locks tokens
	log.Println("==== Locking tokens in Hydro")
	err := s.LockTokens(0, 86400000000000, "10", dstIbcDenom1, hydroContractAddr)
	s.Require().NoError(err)
	err = s.LockTokens(0, 3*86400000000000, "10", dstIbcDenom1, hydroContractAddr)
	s.Require().NoError(err)

	historicalHeight, err := s.NeutronChain.Height(s.ctx)
	s.Require().NoError(err)

	var expectedTotalPower int64 = 25
	expectedUserPowers := map[string]int64{
		s.NeutronChain.ValidatorWallets[0].Address: 25,
		s.NeutronChain.ValidatorWallets[1].Address: 0,
	}

	s.VerifyHistoricalVotingPowers(daoVotingAdapterContractAddr, historicalHeight, expectedTotalPower, expectedUserPowers)

	// second user locks tokens
	log.Println("==== Locking tokens in Hydro")
	err = s.LockTokens(1, 86400000000000, "20", dstIbcDenom2, hydroContractAddr)
	s.Require().NoError(err)
	err = s.LockTokens(1, 3*86400000000000, "20", dstIbcDenom2, hydroContractAddr)
	s.Require().NoError(err)

	historicalHeight, err = s.NeutronChain.Height(s.ctx)
	s.Require().NoError(err)

	expectedTotalPower = 75
	expectedUserPowers = map[string]int64{
		s.NeutronChain.ValidatorWallets[0].Address: 25,
		s.NeutronChain.ValidatorWallets[1].Address: 50,
	}

	s.VerifyHistoricalVotingPowers(daoVotingAdapterContractAddr, historicalHeight, expectedTotalPower, expectedUserPowers)
}
