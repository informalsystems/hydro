The purpose of liquid-collateral is to manage an AMM LP position on Osmosis pairing ATOM with another asset which is expected to be more volatile. The priority for this liquid-collateral is to never lose the ATOM side of the position. To do this, it auctions off the other side of the position if the price moves below a certain point.

1. Liquid-collateral controls a concentrated liquidity position on Osmosis (an AMM also using CosmWasm).
   - We need to decide whether this is done by liquid-collateral accepting the assets and then creating the position, or by liquid-collateral being transferred ownership of the position.
   - Here's an example of a contract that holds Osmosis positions: https://github.com/magma-vaults which we can learn from
2. There is a threshold defined at a certain price (ratio of ATOM:other) in the AMM. When the price moves below this threshold, the position is open for liquidation. While liquidation is open, anyone can call the liquidate endpoint.
   - The liquidation endpoint first checks that the price is below the threshold.
   - The liquidator must supply enough ATOM with the call to bring the number of ATOM held by Liquid-collateral back to the number that was in the position at the beginning, preventing a loss by Hydro.
   - The CL position is pulled by liquid-collateral.
   - The liquidator receives the entire other side of the position, while liquid-collateral receives the ATOM.
   - Morpho Blue is an excellent lending protocol that we could learn about liquidation from, although it is written in Solidity instead of CosmWasm: https://github.com/morpho-org/morpho-blue

## Implementation plan

1.
