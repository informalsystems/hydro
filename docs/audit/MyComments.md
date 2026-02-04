## Finding `Withdrawal Cancellation Allows Share Inflation`:
* The finding would also apply like this:
  * Assume the vault has all funds in an automatic adapter, and all withdrawals are fulfilled instantly.
  * The attacker can monitor for pending loss events, and withdraw before loss events occur.
  * The attacker can re-enter the pool after the loss occurs.
The method in which withdrawal cancellations are handled is not really important to the actual behaviour observed here.
  
The recommendation is not right - if we mint back the same number of shares, an attacker can do the exact same thing, just in the other direction. They start a withdrawal for N shares, which we locked in will give them M base tokens.
If the pool decreases in value, they keep their withdrawal active. Even though their N shares would now only be worth M' (less than M) base tokens, they receive M base tokens.
If the pool increases in value, they cancel their withdrawal and immediately start the withdrawal again. Now, their withdrawal will net them M'' (greater M) base tokens for their N shares.

The way we handle withdrawal cancellations essentially treats the withdrawn amount as finalized as soon as the withdrawal is placed.


## Attacker Manipulates Share Value:

Thanks, good finding. I think we should just pre-mint shares to admin.

## Whitelist

The whitelist already is a committee with multiple members.

Comments on concrete params:
### Token Info Provider
Sanity bounds could make sense, as could timelocks. But depends on the specific token in question.
Don't agree with caching ratio at withdrawal time, I don't agree with the cancel arbitrage finding overall as explained above

### Adapters
No changes

### Deployment tracking toggle
good point, should couple

### withdraw_for_deployment
everything is already a multisig

### Whitelist management
everything is already a multisig

### Adapter query failure
seems plausible to me

## Adapter unregistration

Yep makes sense

## Unrestricted on behalf

minimum amount for withdrawals is ux-impacting potentially. I think not important enough, withdrawal queue can be cleared easily if this starts happening

## Cancelling withdrawals

same, it is because we assume the withdrawal is "complete" when it goes into the queue

## Silent adapter failures

Attributes are a good idea. we don't want to fail the transaction imo, not a good idea

## Adapter documentation

Yeah it makes sense to have this.