# Hydro: Technical Specification

Hydro provides the following methods:
* LockTokens(sender: Account, lock_duration: Time)
	* Escrow the tokens. They cannot be reclaimed until lock_duration has passed. A lock_id is given to the locked tokens
* ReclaimUnlockedTokens(sender: Account, lock_id: int)
	* If it is past the lock_end_time for the lock with the given lock_id, and the sender equals the creator of the lock, send the tokens to the sender.
    Otherwise, the tokens stay escrowed.
* SubmitProposal(data: ?, gauge_id: int):
    * Submit a new proposal. This stores the proposal data, and creates a proposal_id to identify it.
* Vote(sender: Account, proposal_id: int):
    * Adds senders tokens * time_lock_factor [computed with respect to the end of the round] to the score of the proposal with the given proposal_id. if the sender already voted, the previous vote is overwritten, i.e. the previous vote is removed from the score of the old proposal, and the current power of the voter is added to the score of this proposal.
* 

### Correctness Properties

#### All IDs are unique - there cannot ever be two proposals with the same ID, nor two locks with the same lock_id

#### The score of a proposal should be the sum of the time-weighted tokens that voted for it

#### For each round, the sum of assigned funding for all proposals should be equal to the total funding for the gauge

#### For every gauge, the score should be less than the total time-weighted locked tokens

#### 
