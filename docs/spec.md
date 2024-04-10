# Hydro: Technical Specification
This document describes the technical specification of the Hydro protocol. 

## Main contract
The main contract is the Hydro contract, which is responsible for managing token locks,
proposals, voting, and the execution of proposals.

Hydro keeps the following state:
* locks: a collection of locks, each of which has an id, sender, tokens, and lock_end_round
* proposals: a collection of proposals, each of which has an id, round_id, tranche_id, metadata (e.g. description, covenant parameters, etc), whether the proposal was executed, whether the proposal was resolved, and current score.
* votes: a collection of votes, each of which has a sender, round_id, proposal_id, and power

Hydro provides the following methods:
* LockTokens(sender: Account, num_rounds: int, tokens: Coin)
    * Escrow sent tokens to get voting power in the next rounds_locked rounds (this includes the current round).
    * The tokens will be locked until at least num_rounds rounds have been passed since locking, and further until for all proposals that the sender voted for with voting power from these tokens, the proposal has been marked as resolved.
* IncreaseLock(sender: Account, lock_id: int, num_rounds: int)
    * Prerequisite: lock_end_round of the lock with the given lock_id must be less than current_round + num_rounds
    * Increase the lock_duration of the lock with the given lock_id. The lock_end_round is updated to the current_round + num_rounds.
* ReclaimUnlockedTokens(sender: Account)
	* Reclaims all the tokens escrowed on behalf of the sender for which all proposals that were voted on with voting power from these tokens are marked as resolved. The tokens are sent back to the sender and the locks are deleted.
* SubmitProposal(data: ?, tranche_id: int):
    * Submit a new proposal in the current round. This stores the proposal data, and creates a proposal_id to identify it. Submitting a proposal also requires a configurable entry fee, which is paid, according to the
    voting power distribution at the time of payment, to all the locked token accounts.
    * Changes to the state: creates a new proposal with the given data and tranche_id, and sets the initial score to 0.
* Vote(sender: Account, round_id: int, proposal_id: int):
    * Adds the sender_power=senders tokens * power_factor (see below) to the score of the proposal with the given proposal_id. If the sender already voted in the tranche that proposal this is in, the previous vote is overwritten, i.e. the previous vote is removed from the score of the old proposal.
    * Changes to the state: creates a new vote with the sender, round_id, proposal_id, and sender_power, and updates the score of the proposal with the given proposal_id.
* ExecuteProposals(round_id: int):
    * Once a round has ended, this function can be called to execute the proposals in the round. This function will distribute the funding for the tranche to the proposals according to their score via integration with Timewave.
    Each proposal will only be executed once, even if this function is called multiple times. If the round did not reach enough total votes to reach some pre-defined quorum, the proposals will not be executed.
    * Changes to the state: distributes the funding for the tranche to the proposals according to their score via integration with Timewave, and sets the executed flag for each funded proposal to true.
* ResolveProposal(proposal_id: int):
    * Once a proposal has been executed, this function should be called once the liquidity has been returned; or rebalanced according to the new proposal. This function will mark the proposal as resolved, and tokens that voted for this proposal can
    be unlocked. This function should only be callable by authorized accounts: Either a multisig that handles the funds, or automatically by the Timewave covenant that enters/rebalances the LP positions.

## Voting power formula:
    rounds_locked = (lock_end_round - current_round)
    power_factor = 
            * 0 if rounds_locked < 1
            * 1 if 2 > rounds_locked >= 1
            * 1.5 if 4 > rounds_locked >= 2
            * 2 if 7 > rounds_locked >= 4
            * 4 if rounds_locked >= 7


### Correctness Properties

#### All IDs are unique - there cannot ever be two proposals with the same ID, nor two locks with the same lock_id, etc.

#### The score of a proposal should be the sum of the lock-weighted tokens that voted for it.

#### For each round, the sum of assigned funding for all proposals should be the total available funding * the proportion of lock_weighted tokens that voted.

#### For every tranche, the total voting power should be at most the total lock_weighted locked tokens.

#### If you voted with power from N tokens in round R, then your N tokens remain locked until all proposals that you voted for in round R are resolved.

#### If you lock during round R for M rounds, you should have exactly M rounds of non-zero voting power: R, R+1, ..., R+M-1

#### The voting power of locked tokens where current_round >= lock_end_round is zero.

#### If tokens are reclaimed during round R+1, then those tokens must give you no voting power in round R.

#### If all proposals that the owner of a lock has voted for are resolved and the lock_end_round is in the past, the tokens should be reclaimable.


## Tribute
The tribute contract handles the incentivization of voters by allowing anyone to
lock a tribute for a specified proposal, which, upon the end of a round, will be distributed to
the voters of the proposal. The tribute contract keeps the following state:
* tributes: a collection of tributes, each of which has an id, sender, tokens, proposal_id, and round_id.
* claimers: a collection of claimers, each of which has a tribute_id and claimer_address. Used to keep track of who has claimed their share of the tribute, to avoid people claiming multiple times.

The tribute contract provides the following methods:
* LockTribute(sender: Account, tokens: Coin, proposal_id: int, round_id: int)
    * Locks the given tokens for the given proposal_id and round_id. The tokens will become claimable after the round ends and proposals were executed; who can claim them depends on the outcome
    for the proposal.
    * Changes to the state: creates a new tribute with the given sender, tokens, proposal_id, and round_id.

* ClaimTribute(sender: Account, tribute_id: int):
    * If the sender voted for the proposal associated with the tribute_id, the sender can claim their share of the tribute. The share is proportional to the proportion of the power of the
    sender's vote to the total power of the votes for the proposal. In particular, claimed_tokens = total_tribute_amount * sender_vote_power_for_prop / total_prop_score
    * Changes to the state: transfers the tokens to the sender, adds the sender to the claimers list for the tribute_id.

* RefundTribute(sender: Account, tribute_id: int):
    * If the proposal associated with the tribute_id received no support at all, the sender can reclaim their tokens. This is only possible after the round has ended, and we know the final scores.
    * Changes to the state: transfers the tribute tokens back to the sender.

### Correctness Properties

#### All IDs are unique - there cannot ever be two tributes with the same ID.

#### The sum of the claimed tokens for a tribute should be at most the total tokens locked for the tribute.

#### The claim amount for each voter is defined as claimed_tokens = total_tribute_amount * sender_vote_power_for_prop / total_prop_score
