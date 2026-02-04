Users can vote on governance proposals here:
* https://daodao.zone/dao/neutron1lefyfl55ntp7j58k8wy7x3yq9dngsj73s5syrreq55hu4xst660s5p2jtj/home

Each time there is a new governance proposal, we want to snapshot the lockups that users hold at the point it becomes open for voting.

After voting is completed, we will query which users have voted. Then, we will increase the governance score for the lockups that, at the time voting began, were
held by users who ended up voting.

One complexity is around splitting/merging. When two lockups are merged, we want to make the governance score of the resulting lockup equal to the weighted average between the merged lockups.
We already have an "ancestor map" that you can query to get all of a lockups ancestors, but we can't take the weighted average because we don't know the lockups weights anymore.

Missing:
* lockup_weight(lock_id) that goes back in time and gets the LAST amount of funds that the lockup held (amount+denom). amount would technically be enough because in a split/merge, all inputs and outputs have the same denom.