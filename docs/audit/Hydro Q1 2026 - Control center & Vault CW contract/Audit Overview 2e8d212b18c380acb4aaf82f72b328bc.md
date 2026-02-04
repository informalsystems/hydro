# Audit Overview

## The Project

In January 2026, Hydro engaged [Informal Systems](https://informal.systems/) to audit the inflow vaults and control center CosmWasm contracts.

## Scope of this report

Hydro is a decentralized platform that enables ATOM stakers to lock their ATOM to gain voting power, and then vote on bids for Protocol-Owned Liquidity (PoL) deployments. Projects submit bids and offer tribute in exchange for liquidity, and users allocate their voting power to the bids they want to support. Hydro’s smart contracts handle the liquidity deployments, and rewards from tribute are distributed back to voters. This creates a transparent, efficient, and community-driven process for managing the Cosmos Hub’s liquidity while generating value for ATOM holders.

The scope of this audit was the the `vault` and `control-center` CosmWasm contracts. In particular, the scope included:

- `vault` ([62068707e3705258b2bded634406fa6894148ae9](https://github.com/informalsystems/hydro/tree/62068707e3705258b2bded634406fa6894148ae9))
- `control-center` ([62068707e3705258b2bded634406fa6894148ae9](https://github.com/informalsystems/hydro/tree/62068707e3705258b2bded634406fa6894148ae9))

## Audit plan

The audit was conducted between 16th January. and 30th January, totalling to 2.5 person weeks by the following personnel:

- Aleksandar Ignjatijevic

## Conclusions

We performed the audit by manually inspecting the code. We found the code to be of generally good quality. We identified some problems in the implementation (more on that in the Findings section). These problems were reported to the dev team and the discussion on rectifying problems is ongoing.