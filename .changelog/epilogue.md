# CHANGELOG

# v1.0.0

Date: September 24th, 2023

This is the first version of the auction and tribute contracts for the Hydro liquidity auction platform.
It includes:
* The main _Hydro_ contract, where:
  * Users can lock, relock, and unlock staked ATOM in the form of LSM shares
  * Projects can create proposals to deploy liquidity
  * Users can vote on proposals
* The _Tribute_ contract, where:
  * Projects can add tributes to their proposals
  * Users can claim tributes for the proposals that they voted on