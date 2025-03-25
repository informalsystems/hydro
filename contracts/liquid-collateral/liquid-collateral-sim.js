// RUN THIS WITH `deno run liquid-collateral-sim.js`

import {
    ConcentratedLiquidityPool
} from "npm:defi-sim@1.0.3";

// Inputs
const ATOM_USD_price = 4.5
const WOBBLE_USD_price = .25

const currentPrice = WOBBLE_USD_price / ATOM_USD_price

const WOBBLESupplied = 1000
const ATOMSupplied = 100

const lowerBoundPrice = 1 / 25;
const upperBoundPrice = 1 / 15;

// Log inputs
console.log("ATOM_USD_price:", ATOM_USD_price)
console.log("WOBBLE_USD_price:", WOBBLE_USD_price)
console.log("Current price (ATOM/WOBBLE):", currentPrice)
console.log("WOBBLE supplied:", WOBBLESupplied)
console.log("ATOM supplied:", ATOMSupplied)
console.log("USD price of WOBBLE supplied:", WOBBLESupplied * WOBBLE_USD_price)
console.log("USD price of ATOM supplied:", ATOMSupplied * ATOM_USD_price)
console.log("Lower bound price (ATOM/WOBBLE):", lowerBoundPrice)
console.log("Upper bound price (ATOM/WOBBLE):", upperBoundPrice)

// Initialize pool
const liquidityPool = new ConcentratedLiquidityPool({
    initialPrice: currentPrice,
    feeRate: 0.003,
});

// Add liquidity
const position = liquidityPool.enterPosition({
    balance: {
        x: WOBBLESupplied,
        y: ATOMSupplied
    },
    range: [lowerBoundPrice, upperBoundPrice],
});

// Simulate price moving to lower bound
liquidityPool.movePrice(lowerBoundPrice);
console.log("Balance at lower bound:", position.balance);
console.log("Rewards accumulated:", position.rewards);

const totalWOBBLEAtLowerBound = position.balance.x + position.rewards.x
const ATOMforWOBBLE = totalWOBBLEAtLowerBound * lowerBoundPrice

console.log("WOBBLE in the position at lower bound:", totalWOBBLEAtLowerBound)
console.log("WOBBLE price at lower bound assuming ATOM_USD price has not changed:", WOBBLE_USD_price / ATOMforWOBBLE)
console.log("ATOM which can be bought back at lower bound:", ATOMforWOBBLE)