// RUN THIS WITH `deno run liquid-collateral-sim.ts`

import {
  ConcentratedLiquidityPool,
  getMaxTokenAmounts,
  getLiquidity,
  getTokenAmounts,
} from "npm:defi-sim@1.0.3";

// Inputs
const starting_ATOM_USD_price: number = 4.5;
const starting_WOBBLE_USD_price: number = 0.25;

const currentPrice: number =
  starting_WOBBLE_USD_price / starting_ATOM_USD_price;

// Input the amount of ATOM you want to provide
const ATOMSupplied: number = 100;

const lowerBoundPrice: number = 0.03;
const upperBoundPrice: number = 0.1;

// Calculate optimal amounts
const sqrtPrice: number = Math.sqrt(currentPrice);
const sqrtRange: [number, number] = [
  Math.sqrt(lowerBoundPrice),
  Math.sqrt(upperBoundPrice),
];

// Calculate optimal WOBBLE amount for the given ATOM amount using concentrated liquidity formulas
function calculateOptimalWobble(atomAmount: number): number {
  // The liquidity L at the current price is determined by the ATOM amount:
  // atomAmount = L * (√P - √Plow)
  // Therefore L = atomAmount / (√P - √Plow)
  const L: number = atomAmount / (sqrtPrice - sqrtRange[0]);

  // Once we have L, we can calculate the required WOBBLE:
  // wobbleAmount = L * (1/√P - 1/√Phigh)
  return L * (1 / sqrtPrice - 1 / sqrtRange[1]);
}

const optimalWOBBLE: number = calculateOptimalWobble(ATOMSupplied);

// Log all calculations
console.log("ATOM_USD_price:", starting_ATOM_USD_price);
console.log("WOBBLE_USD_price:", starting_WOBBLE_USD_price);
console.log("Current price (ATOM/WOBBLE):", currentPrice);

console.log("\nInput amount:");
console.log("ATOM amount:", ATOMSupplied);
console.log("ATOM USD value:", ATOMSupplied * starting_ATOM_USD_price);

console.log("\nCalculated optimal amounts:");
console.log("Required WOBBLE amount:", optimalWOBBLE);
console.log(
  "Required WOBBLE USD value:",
  optimalWOBBLE * starting_WOBBLE_USD_price
);

console.log("\nRange settings:");
console.log("Lower bound price (ATOM/WOBBLE):", lowerBoundPrice);
console.log("Upper bound price (ATOM/WOBBLE):", upperBoundPrice);

// Verify the ratio is optimal
const verificationAmounts = getMaxTokenAmounts({
  tokens: { x: optimalWOBBLE, y: ATOMSupplied },
  sqrtRange,
  sqrtPrice,
});

console.log("\nVerification (amounts that would actually be used):");
console.log("WOBBLE used:", verificationAmounts.x);
console.log("ATOM used:", verificationAmounts.y);
console.log(
  "USD value of WOBBLE used:",
  verificationAmounts.x * starting_WOBBLE_USD_price
);
console.log(
  "USD value of ATOM used:",
  verificationAmounts.y * starting_ATOM_USD_price
);

// Initialize pool
const liquidityPool = new ConcentratedLiquidityPool({
  initialPrice: currentPrice,
  feeRate: 0.003,
});

// Add liquidity
const position = liquidityPool.enterPosition({
  balance: {
    x: optimalWOBBLE,
    y: ATOMSupplied,
  },
  range: [lowerBoundPrice, upperBoundPrice],
});

// Simulate price moving to lower bound
liquidityPool.movePrice(lowerBoundPrice);
console.log("\nBalance at lower bound:", position.balance);
console.log("Rewards accumulated:", position.rewards);

const totalWOBBLEAtLowerBound: number = position.balance.x + position.rewards.x;
const ATOMforWOBBLE: number = totalWOBBLEAtLowerBound * lowerBoundPrice;

console.log("WOBBLE in the position at lower bound:", totalWOBBLEAtLowerBound);
console.log(
  "WOBBLE price at lower bound assuming ATOM_USD price has not changed:",
  (ATOMforWOBBLE * starting_ATOM_USD_price) / totalWOBBLEAtLowerBound
);
console.log("ATOM which can be bought back at lower bound:", ATOMforWOBBLE);
