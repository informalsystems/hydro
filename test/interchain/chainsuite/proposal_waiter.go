package chainsuite

type proposalWaiter struct {
	canDeposit chan struct{}
	isInVoting chan struct{}
	canVote    chan struct{}
	isPassed   chan struct{}
}

func (pw *proposalWaiter) waitForDepositAllowed() {
	<-pw.canDeposit
}

func (pw *proposalWaiter) allowDeposit() {
	close(pw.canDeposit)
}

func (pw *proposalWaiter) waitForVotingPeriod() {
	<-pw.isInVoting
}

func (pw *proposalWaiter) startVotingPeriod() {
	close(pw.isInVoting)
}

func (pw *proposalWaiter) waitForVoteAllowed() {
	<-pw.canVote
}

func (pw *proposalWaiter) allowVote() {
	close(pw.canVote)
}

func (pw *proposalWaiter) waitForPassed() {
	<-pw.isPassed
}

func (pw *proposalWaiter) pass() {
	close(pw.isPassed)
}

func newProposalWaiter() *proposalWaiter {
	return &proposalWaiter{
		canDeposit: make(chan struct{}),
		isInVoting: make(chan struct{}),
		canVote:    make(chan struct{}),
		isPassed:   make(chan struct{}),
	}
}
