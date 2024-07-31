
**Side excourse: How do delegator shares work?**

delegator shares correspond to an amount of tokens that the validator has locked
|tokens|/|delegator_shares| = number of tokens each delegator share is worth

outstanding rewards are accured to the validator pool

what happens when you claim rewards?
- undelegates -> converts your shares into tokens, PLUS your share of the rewards
- re-delegate your initial tokens

when you delegate:
- you add your tokens to the validators tokens
- but you get *less* shares than you put tokens in, because each share also is worth a portion of the rewards



1kk shares
950k tokens

when you redeem a share, you only get 0.95 atom back for it


**How do we get the data into the contract?**

LSM token has denom cosmosvaloper1clpqr4nrk4khgkxj78fcwwh6dl3uw4epsluffn/456546

keep denoms by validator, kick out the ID



We get the validator address

Query for the validator

Will return val info, including
 "tokens": "11384790386882"
 "delegatorShares": "11384790386882000000000000000000"

This lets us calculate how many locked ATOM each share corresponds to
Disregarding rewards, this is the amount of voting power the user should have

if we want to take into account rewards, we query them
gaiad q distribution validator-outstanding-rewards cosmosvaloper1clpqr4nrk4khgkxj78fcwwh6dl3uw4epsluffn
each share is entitled to the same portion of the rewards, so we can calulate the principal atom + outstanding rewards


###Do we take into account pending rewards?###


How do we test this?
CW orchestrator
CW multi test
Can we mock it out?


**How do we modify the contract to handle multiple tokens?**


score for each proposal -> [num of token A voted for proposal, num of token B, ...]
=> when do we convert the numbers of tokens into voting power? this is pretty expensive

where do we need this?
  > for the frontend
  > when the proposal is queried
    - it would be optimal if this was the voting power at the end of the round, instead of right now, but that seems hard
    - fundamental boundary: at least once per round, we need to compute the weight for each denom (~num of validators)

proposal:
-> anyone can execute a message that updates weights for a single validator (/denom)
-> we run this via cronjob to update each token once per round, but other people can also do it
- tokenAWeight: 1, tokenBWeight: 0.98 -> tokenAWeight: 0.95, tokenBWeight: 0.98
- keep the token weights per round, only update the current round
- are we ok with the voting powers being messed up during the round?

alternatively, we could also have a simple module that does this on endblock.
disadvantage: extra module
advantage: we don't pay gas


issue: it's easy to create (millions of) new LSM denoms (make a validator, tokenize 0.000001 atom, repeat)
-> is this actually true? can inactive validators have tokenized shares?
-> it is probably problematic because we need to iterate over all denoms


total locked amount? -> just sum up the tokens, ignore rewards?
--
this is a sanity check anyways, we are ok if it's not 100% accurate, the committee can update it


deploying on Neutron: realistic, it needs ~180 interchain queries per round