# Hydro: Technical Specification
This document describes the technical specification of the Hydro protocol. 

## Main contract
The main contract is the Hydro contract, which is responsible for managing token locks,
proposals, voting, and the execution of proposals.

Hydro keeps the following state:
* locks: a collection of locks, each of which has an id, sender, tokens, and lock_end_time
* proposals: a collection of proposals, each of which has an id, round_id, tranche_id, metadata (e.g. description, covenant parameters, etc), whether the proposal was already executed, and current score.
* votes: a collection of votes, each of which has a sender, round_id, proposal_id, and power

Hydro provides the following methods:
* LockTokens(sender: Account, lock_duration: Time, tokens: Coin)
    * Escrow sent tokens (which must be stAtoms for the first design). They cannot be reclaimed until lock_duration has passed.
    * Change to the state: creates a new lock with the sender, tokens, and computes the lock_end_time as the current time plus lock_duration.
* IncreaseLock(sender: Account, lock_id: int, lock_duration: Time)
    * Increase the lock_duration of the lock with the given lock_id. The lock_end_time is postponed by lock_duration.
    * Changes to the state: updates the lock with the given lock_id, and computes the new lock_end_time as the previous lock_end_time plus the new lock_duration.
* ReclaimUnlockedTokens(sender: Account)
	* Reclaims all the tokens escrowed on behalf of the sender for which the lock_duration has passed.
	* Changes to the state: Deletes all the locks for the sender for which the lock_end_time has passed, and transfers the no-longer locked tokens back to the sender.
* SubmitProposal(data: ?, tranche_id: int):
    * Submit a new proposal in the current round. This stores the proposal data, and creates a proposal_id to identify it. Submitting a proposal also requires a configurable entry fee, which is paid, according to the
    voting power distribution at the time of payment, to all the locked token accounts.
    * Changes to the state: creates a new proposal with the given data and tranche_id, and sets the initial score to 0.
* Vote(sender: Account, round_id: int, proposal_id: int):
    * Adds the sender_power=senders tokens * time_lock_factor [computed with respect to the end of the round] to the score of the proposal with the given proposal_id. If the sender already voted, the previous vote is overwritten, i.e. the previous vote is removed from the score of the old proposal.
    * Changes to the state: creates a new vote with the sender, round_id, proposal_id, and sender_power, and updates the score of the proposal with the given proposal_id.
* ExecuteProposals(round_id: int):
    * Once a round has ended, this function can be called to execute the proposals in the round. This function will distribute the funding for the tranche to the proposals according to their score via integration with Timewave.
    Each proposal will only be executed once, even if this function is called multiple times. If the round did not reach enough total votes to reach some pre-defined quorum, the proposals will not be executed.
    * Changes to the state: distributes the funding for the tranche to the proposals according to their score via integration with Timewave, and sets the executed flag for each funded proposal to true.

## Voting power formula:
    round_locked_multiplier = (lock_end_time - current_round_end_time) / round_duration
        * For example, say lock_end_time = 5, current_round_end_time = 1, round_duration = 2. Then round_locked_multiplier = 2, beacuse at the end of the round, the funds will be locked for 2 more rounds.
        * Then, we look up the power_factor according to the round_locked_multiplier. The function for computing the power_factor is
    power_factor = 
            * 0 if round_locked_multiplier < 1
            * 1 if 2 > round_locked_multiplier >= 1
            * 1.5 if 4 > round_locked_multiplier >= 2
            * 2 if 7 > round_locked_multiplier >= 4
            * 4 if round_locked_multiplier >= 7

### Open Questions

* How important is it for rounds and lock_durations to align? In the properties below, I assume they closely align, e.g. if you lock for 12 round_durations, you should also
get to have 12 rounds of voting power; this is not the case in the implementati right now.
* Is it important that tokens stay locked while the LP position they voted for is being executed? In the spec, I assume that it's the case because that is the way it is in the litepaper.


### Correctness Properties

#### All IDs are unique - there cannot ever be two proposals with the same ID, nor two locks with the same lock_id, etc.

#### The score of a proposal should be the sum of the lock-weighted tokens that voted for it.

#### For each round, the sum of assigned funding for all proposals should be the total available funding * the proportion of time-weighted voting power that voted.

#### For every tranche, the total voting power should be at most the total time-weighted locked tokens.

#### If you voted with power from N tokens in round R, then your N tokens remain locked until round R+1 is over. 

#### If you lock during round R for M rounds, you should have M rounds of non-zero voting power: R, R+1, ..., R+M-1

#### Voting power is zero if the lock_end_time has passed.

#### If tokens are reclaimed during round R+1, then the tokens must add zero voting power in round R.


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

### Open questions:
* What happens to unclaimed tribute (could become refundable by the sender after some time; could be reallocated to other claimers after a time; could just be locked forever/essentially burned?) Locking without fallback is easiest, reallocating might be most exciting for community.


### Correctness Properties

#### All IDs are unique - there cannot ever be two tributes with the same ID.

#### The sum of the claimed tokens for a tribute should be at most the total tokens locked for the tribute.

#### The claim amount for each voter is defined as claimed_tokens = total_tribute_amount * sender_vote_power_for_prop / total_prop_score
