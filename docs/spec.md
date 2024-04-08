# Hydro: Technical Specification

Hydro provides the following methods:
* LockTokens(sender: Account, lock_duration: Time, tokens: Coin)
    * Escrow sent tokens (which must be stAtoms). They cannot be reclaimed until lock_duration has passed.
* ReclaimUnlockedTokens(sender: Account)
	* Reclaims all the tokens escrowed on behalf of the sender for which the lock_duration has passed.
* SubmitProposal(data: ?, tranche_id: int):
    * Submit a new proposal in the current round. This stores the proposal data, and creates a proposal_id to identify it.
* Vote(sender: Account, proposal_id: int):
    * Adds the sender_power=senders tokens * time_lock_factor [computed with respect to the end of the round] to the score of the proposal with the given proposal_id. If the sender already voted, the previous vote is overwritten, i.e. the previous vote is removed from the score of the old proposal.
* EndRound():
    * Ends the current round by incrementing the round_id and setting the end_time for the next round.

At the end of a round, in each tranche, the N proposals with the highest scores are funded, where N is a pre-determined parameter.
Each proposal gets funded in proportion to its score, i.e. the funding for a proposal is the total funding for the
tranche multiplied by the score of the proposal divided by the sum of the scores of the N proposals that are funded.

### Correctness Properties

#### All IDs are unique - there cannot ever be two proposals with the same ID, nor two locks with the same lock_id

#### The score of a proposal should be the sum of the time-weighted tokens that voted for it

#### For each round, the sum of assigned funding for all proposals should be equal to the total funding for the tranche

#### For every tranche, the score should be less than the total time-weighted locked tokens

#### 
