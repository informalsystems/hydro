DEPRECATED IN FAVOR OF 002_points.md

We want to distribute part of the performance fees to users/partners.

There are 3 main ways this will happen. Shares of the performance fees will be distributed to:
* Hydro NFT lockup holders with high governance scores
* Affiliates who refer users to the vault
* Early users who get a rebate on their performance fee (which is distributed as a "refund" of some paid performance fees)

Performance fees for each control center are distributed independently.

To do this, we will run an off-chain process that periodically (e.g. hourly) snapshots the holdings of every vault depositor.

When performance fees are accrued, it computes:
For each holder, we compute what proportion of performance fees originate from this holder, as `holder_fees = performance_fees * (holder_shares/total_shares)`.
For holders who have a fee rebate, we compute their refund as `refund = holder_fees * rebate_percentage`, and the remainder as `non_rebated_fees = holder_fees - refund`.

For each user, we also have any number of `fee_sharer`s, each with a certain fraction as `fee_share`. Each fee sharer receives the stated fraction of the performance fee remaining after the rebate is applied. The fractions of all `fee_sharer`s should add up to at most 1.

On a configurable interval, we move some vault shares from the performance fees into a "claim" contract, and enter the amount each user may claim.
We only do this for users that have accrued some minimal amount of rewards, to save on gas.
Users then interact with the smart contract to claim their share of the performance fees.
Before this, users can already query the backend to query their currently-collected share of the fees.

Note:
* Small potential issue: in `holder_fees = performance_fees * (holder_shares/total_shares)`, `total_shares` should not include the shares issued by the last performance fee issuance round
* To combat people who mint right before fee distribution, should we take the average number of shares held since last fee distribution (based on the regular snapshots, which are more frequent than fee distributions)?