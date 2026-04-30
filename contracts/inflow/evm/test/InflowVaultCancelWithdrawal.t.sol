// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "forge-std/Test.sol";
import {ERC20} from "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {InflowVault} from "../contracts/InflowVault.sol";
import {InflowWithdrawalQueueLib} from "../contracts/InflowWithdrawalQueueLib.sol";

contract MockUSDC is ERC20 {
    constructor() ERC20("USD Coin", "USDC") {}

    function decimals() public pure override returns (uint8) {
        return 6;
    }

    function mint(address to, uint256 amount) external {
        _mint(to, amount);
    }
}

contract InflowVaultCancelWithdrawalTest is Test {
    InflowVault vault;
    MockUSDC usdc;

    address admin = makeAddr("admin");
    address user  = makeAddr("user");
    address alice = makeAddr("alice");
    address bob   = makeAddr("bob");

    uint256 constant AMOUNT           = 100_000e6;  // 100,000 USDC (6 decimals)
    uint256 constant DEPOSIT_CAP      = 1_000_000e6;
    uint256 constant MAX_WITHDRAWALS  = 10;

    function setUp() public {
        usdc = new MockUSDC();

        address[] memory wl = new address[](1);
        wl[0] = admin;

        vault = new InflowVault(
            IERC20(address(usdc)),
            "Hydro Inflow Vault",
            "hvUSDC",
            DEPOSIT_CAP,
            MAX_WITHDRAWALS,
            wl,
            0,           // no performance fee
            address(0)
        );

        usdc.mint(user, AMOUNT);
        // Alice needs 150k: 100k initial + 50k for the second deposit.
        usdc.mint(alice, 150_000e6);
        usdc.mint(bob,   AMOUNT);
    }

    /// @notice Scenario:
    ///   1. User deposits 100,000 USDC → receives 100,000 shares.
    ///   2. Whitelisted admin calls withdrawForDeployment(100,000) → vault balance goes to 0.
    ///   3. User redeems 100,000 shares → vault has no free balance, so the
    ///      withdrawal is queued. Shares are locked inside the vault.
    ///   4. User cancels the queued withdrawal → shares are returned.
    function test_depositWithdrawForDeploymentRedeemAndCancelWithdrawal() public {
        // ── Step 1: deposit ──────────────────────────────────────────────────
        vm.startPrank(user);
        usdc.approve(address(vault), AMOUNT);
        uint256 sharesReceived = vault.deposit(AMOUNT, user);
        vm.stopPrank();

        assertEq(sharesReceived, AMOUNT,                    "shares minted should equal deposited assets (1:1)");
        assertEq(vault.balanceOf(user), AMOUNT,             "user should hold 100,000 shares");
        assertEq(usdc.balanceOf(address(vault)), AMOUNT,    "vault USDC balance should be 100,000");

        // ── Step 2: admin withdraws for deployment ───────────────────────────
        vm.prank(admin);
        vault.withdrawForDeployment(AMOUNT);

        assertEq(usdc.balanceOf(address(vault)), 0,         "vault balance should be 0 after deployment withdrawal");
        assertEq(vault.deployedAmount(), AMOUNT,            "deployedAmount should be 100,000 USDC");
        assertEq(usdc.balanceOf(admin), AMOUNT,             "admin should hold the deployed USDC");

        // ── Step 3: user redeems — vault has no free balance → queued ────────
        uint256 withdrawalId = vault.nextWithdrawalId();    // == 0 (first entry)

        vm.expectEmit(true, true, true, true, address(vault));
        emit InflowVault.WithdrawalQueued(withdrawalId, user, user, AMOUNT, AMOUNT);

        vm.prank(user);
        vault.redeem(AMOUNT, user, user);

        // Shares are locked inside the vault, not burned yet.
        assertEq(vault.balanceOf(user), 0,                  "user shares should be locked in vault");

        // Verify the queue entry.
        InflowWithdrawalQueueLib.WithdrawalEntry memory entry = vault.withdrawalRequest(withdrawalId);
        assertEq(entry.id,              withdrawalId,       "entry id mismatch");
        assertEq(entry.owner,           user,               "entry owner should be user");
        assertEq(entry.receiver,        user,               "entry receiver should be user");
        assertEq(entry.sharesLocked,    AMOUNT,             "entry sharesLocked should be 100,000");
        assertEq(entry.amountToReceive, AMOUNT,             "entry amountToReceive should be 100,000 USDC");
        assertFalse(entry.isFunded,                         "entry should not yet be funded");

        uint256[] memory queuedIds = vault.getUserWithdrawalIds(user);
        assertEq(queuedIds.length,  1,            "user should have exactly 1 queued withdrawal");
        assertEq(queuedIds[0],      withdrawalId, "queued id should match");

        // ── Step 4: user cancels the queued withdrawal ────────────────────────
        uint256[] memory cancelIds = new uint256[](1);
        cancelIds[0] = withdrawalId;

        vm.expectEmit(true, true, false, false, address(vault));
        emit InflowVault.WithdrawalCancelled(withdrawalId, user);

        vm.prank(user);
        vault.cancelWithdrawal(cancelIds);

        // Shares returned to user.
        assertEq(vault.balanceOf(user), AMOUNT,             "user should have 100,000 shares restored");

        // Queue entry cleared.
        InflowWithdrawalQueueLib.WithdrawalEntry memory cancelled = vault.withdrawalRequest(withdrawalId);
        assertEq(cancelled.initiatedAt, 0,                  "cancelled entry should be deleted");

        uint256[] memory remainingIds = vault.getUserWithdrawalIds(user);
        assertEq(remainingIds.length, 0,                    "user withdrawal queue should be empty");
    }

    /// @notice Interleaved-deployment scenario with yield simulation:
    ///   1. Alice deposits 100,000 USDC  → 100,000 shares (1:1).
    ///   2. Admin withdrawForDeployment(100,000) [between deposits]
    ///      → vault: 0, deployedAmount: 100k.
    ///   2b.Admin submitDeployedAmount(120,000) — reports 20k yield earned during deployment.
    ///      deployedAmount: 120k. Share price rises to 1.2 (120k assets / 100k shares).
    ///   3. Bob deposits 60,000 USDC → 50,000 shares (price 1.2:1, not 1:1).
    ///      Vault: 60k. Supply: 150k. TotalAssets: 180k.
    ///   4. Alice redeems 100,000 shares → assets ≈ 120k (100k × 1.2);
    ///      vault free balance = 60k < 120k → queued (id 0).
    ///      Effective supply = 150k − 100k (locked) = 50k.
    ///      Effective assets ≈ 180k − 120k = 60k.
    ///   5. Admin attempts withdrawForDeployment(60,000) [between redeem and cancel].
    ///      Queue reserves ≈ 120k (> vault balance of 60k) → available = 0 → REVERTS.
    ///   6. Alice cancels the queued withdrawal → 100,000 shares returned. Queue cleared.
    ///      Supply: 150k. TotalAssets: 180k.
    ///   7. Admin withdrawForDeployment(60,000) [after cancel] → SUCCEEDS.
    ///      Vault: 0. DeployedAmount: 180k. TotalAssets: 180k.
    function test_interleaved_deployments_queueProtectsAndCancelUnlocks() public {
        uint256 bobDeposit       = 60_000e6;
        uint256 deployedWithYield = 120_000e6; // 100k deployed + 20k yield

        // ── Step 1: Alice deposits 100,000 USDC ──────────────────────────────
        vm.startPrank(alice);
        usdc.approve(address(vault), AMOUNT);
        uint256 aliceShares = vault.deposit(AMOUNT, alice);
        vm.stopPrank();

        assertEq(aliceShares,         AMOUNT, "alice: 100k shares at 1:1");
        assertEq(vault.totalSupply(), AMOUNT, "total supply = 100k");
        assertEq(vault.totalAssets(), AMOUNT, "total assets = 100k");

        // ── Step 2: Admin deploys Alice's full deposit [between deposits] ─────
        vm.prank(admin);
        vault.withdrawForDeployment(AMOUNT);

        assertEq(usdc.balanceOf(address(vault)), 0,      "vault balance = 0 after first deployment");
        assertEq(vault.deployedAmount(),         AMOUNT, "deployedAmount = 100k");
        assertEq(vault.totalAssets(),            AMOUNT, "totalAssets = 100k");

        // ── Step 2b: Simulate yield — admin reports updated deployment value ──
        // Deployment earned 20k: submit 120k instead of 100k.
        // This sets deployedAmount = 120k, raising the share price to 1.2 (120k / 100k).
        vm.prank(admin);
        vault.submitDeployedAmount(deployedWithYield);

        assertEq(vault.deployedAmount(), deployedWithYield, "deployedAmount = 120k (100k + 20k yield)");
        assertEq(vault.totalAssets(),    deployedWithYield, "totalAssets = 120k");
        assertEq(vault.totalSupply(),    AMOUNT,            "supply unchanged = 100k");
        // Share price: 120k assets / 100k shares = 1.2 USDC per share.

        // ── Step 3: Bob deposits 60,000 USDC ─────────────────────────────────
        // Price is now 1.2:1 → Bob receives 60k / 1.2 = 50k shares.
        // shares = floor(60k × (100k+1) / (120k+1)) = 50,000e6 (exact).
        vm.startPrank(bob);
        usdc.approve(address(vault), bobDeposit);
        uint256 bobShares = vault.deposit(bobDeposit, bob);
        vm.stopPrank();

        uint256 bobExpectedShares = 50_000e6;
        assertEq(bobShares,                      bobExpectedShares,                       "bob: 50k shares (price 1.2:1)");
        assertEq(usdc.balanceOf(address(vault)), bobDeposit,                              "vault holds 60k");
        assertEq(vault.totalSupply(),            AMOUNT + bobExpectedShares,              "total supply = 150k");
        assertEq(vault.totalAssets(),            bobDeposit + deployedWithYield,          "total assets = 180k (60k vault + 120k deployed)");

        // ── Step 4: Alice redeems 100k shares → queued ────────────────────────
        // previewRedeem(100k) = floor(100k × (180k+1) / (150k+1)) = 119,999.866 ≈ 120k.
        // Free balance = vault (60k) < ~120k needed → queued.
        uint256 aliceRedemptionAssets = vault.previewRedeem(AMOUNT);
        uint256 withdrawalId = vault.nextWithdrawalId(); // 0

        vm.expectEmit(true, true, true, true, address(vault));
        emit InflowVault.WithdrawalQueued(withdrawalId, alice, alice, AMOUNT, aliceRedemptionAssets);

        vm.prank(alice);
        vault.redeem(AMOUNT, alice, alice);

        // Shares locked in vault, not burned yet.
        assertEq(vault.balanceOf(alice), 0, "alice shares locked in vault");

        // Effective supply / assets reflect only Bob's position.
        assertEq(vault.totalSupply(), bobExpectedShares,                                   "effective supply = 50k (bob only)");
        assertEq(vault.totalAssets(), bobDeposit + deployedWithYield - aliceRedemptionAssets, "effective assets = 180k - queued (~60k)");

        InflowWithdrawalQueueLib.WithdrawalEntry memory entry = vault.withdrawalRequest(withdrawalId);
        assertEq(entry.owner,           alice,                 "entry owner = alice");
        assertEq(entry.sharesLocked,    AMOUNT,                "sharesLocked = 100k");
        assertEq(entry.amountToReceive, aliceRedemptionAssets, "amountToReceive ~120k");
        assertFalse(entry.isFunded,                            "not funded");

        // ── Step 5: Admin attempts second deployment while withdrawal is pending ─
        // totalWithdrawalAmount ≈ 120k; vaultBalance = 60k → available = 0 → reverts.
        // The queue shields all 60k vault funds (they are covered by the pending claim).
        vm.expectRevert(InflowVault.ZeroAmount.selector);
        vm.prank(admin);
        vault.withdrawForDeployment(bobDeposit);

        // Vault state unchanged after the failed call.
        assertEq(usdc.balanceOf(address(vault)), bobDeposit,        "vault balance unchanged after failed deployment");
        assertEq(vault.deployedAmount(),         deployedWithYield, "deployedAmount unchanged after failed deployment");

        // ── Step 6: Alice cancels the queued withdrawal ───────────────────────
        uint256[] memory cancelIds = new uint256[](1);
        cancelIds[0] = withdrawalId;

        vm.expectEmit(true, true, false, false, address(vault));
        emit InflowVault.WithdrawalCancelled(withdrawalId, alice);

        vm.prank(alice);
        vault.cancelWithdrawal(cancelIds);

        assertEq(vault.balanceOf(alice),    AMOUNT,                          "alice shares restored");
        assertEq(vault.balanceOf(bob),      bobExpectedShares,               "bob shares unaffected");
        assertEq(vault.totalSupply(),       AMOUNT + bobExpectedShares,      "total supply = 150k");
        assertEq(vault.totalAssets(),       bobDeposit + deployedWithYield,  "total assets = 180k");
        assertEq(vault.withdrawalRequest(withdrawalId).initiatedAt, 0, "entry deleted");
        assertEq(vault.getUserWithdrawalIds(alice).length, 0,          "alice queue empty");

        // ── Step 7: Admin can now deploy the 60k freed by the cancellation ────
        vm.prank(admin);
        vault.withdrawForDeployment(bobDeposit);

        assertEq(usdc.balanceOf(address(vault)), 0,                                "vault = 0 after final deployment");
        assertEq(vault.deployedAmount(),         deployedWithYield + bobDeposit,   "deployedAmount = 180k");
        assertEq(vault.totalAssets(),            deployedWithYield + bobDeposit,   "totalAssets = 180k (fully deployed)");
    }

    /// @notice Two-user scenario:
    ///   1. Alice deposits 100,000 USDC  → 100,000 shares (price 1:1).
    ///   2. Bob   deposits 100,000 USDC  → 100,000 shares (price 1:1).
    ///      Vault holds 200,000 USDC; total supply = 200,000 shares.
    ///   3. Admin withdrawForDeployment(200,000) → vault balance = 0,
    ///      deployedAmount = 200,000. Share price unchanged (1:1).
    ///   4. Alice redeems 100,000 shares → no free balance → queued.
    ///      Shares locked in vault.
    ///      Effective supply = 200k − 100k (locked) = 100k (Bob only).
    ///      Effective totalAssets = 200k − 100k (pending) = 100k.
    ///   5. Alice cancels the queued withdrawal → 100,000 shares returned.
    ///      Supply and totalAssets both restored to 200,000.
    ///   6. Alice deposits 50,000 USDC → 50,000 shares (price still 1:1).
    ///      Alice total: 150,000 shares. Bob: 100,000 shares.
    ///      totalSupply = 250,000; totalAssets = 250,000.
    function test_twoUsers_depositRedeemCancelAndRedeposit() public {
        // ── Step 1: Alice deposits 100,000 USDC ──────────────────────────────
        vm.startPrank(alice);
        usdc.approve(address(vault), AMOUNT);
        uint256 aliceShares = vault.deposit(AMOUNT, alice);
        vm.stopPrank();

        assertEq(aliceShares,                    AMOUNT, "alice: 100k shares at 1:1");
        assertEq(vault.balanceOf(alice),         AMOUNT, "alice share balance");
        assertEq(usdc.balanceOf(address(vault)), AMOUNT, "vault holds 100k USDC");
        assertEq(vault.totalSupply(),            AMOUNT, "total supply = 100k");
        assertEq(vault.totalAssets(),            AMOUNT, "total assets = 100k");

        // ── Step 2: Bob deposits 100,000 USDC ────────────────────────────────
        vm.startPrank(bob);
        usdc.approve(address(vault), AMOUNT);
        uint256 bobShares = vault.deposit(AMOUNT, bob);
        vm.stopPrank();

        assertEq(bobShares,                      AMOUNT,       "bob: 100k shares at 1:1");
        assertEq(vault.balanceOf(bob),           AMOUNT,       "bob share balance");
        assertEq(usdc.balanceOf(address(vault)), 2 * AMOUNT,   "vault holds 200k USDC");
        assertEq(vault.totalSupply(),            2 * AMOUNT,   "total supply = 200k");
        assertEq(vault.totalAssets(),            2 * AMOUNT,   "total assets = 200k");

        // ── Step 3: admin withdraws all 200,000 for deployment ────────────────
        vm.prank(admin);
        vault.withdrawForDeployment(2 * AMOUNT);

        assertEq(usdc.balanceOf(address(vault)), 0,           "vault balance = 0");
        assertEq(vault.deployedAmount(),         2 * AMOUNT,  "deployedAmount = 200k");
        assertEq(vault.totalAssets(),            2 * AMOUNT,  "totalAssets unchanged (tracked via deployedAmount)");
        assertEq(vault.totalSupply(),            2 * AMOUNT,  "total supply unchanged");

        // ── Step 4: Alice redeems 100,000 shares → queued ─────────────────────
        // previewRedeem(100k): 100k * (200k+1)/(200k+1) = 100k assets
        uint256 withdrawalId = vault.nextWithdrawalId(); // == 0

        vm.expectEmit(true, true, true, true, address(vault));
        emit InflowVault.WithdrawalQueued(withdrawalId, alice, alice, AMOUNT, AMOUNT);

        vm.prank(alice);
        vault.redeem(AMOUNT, alice, alice);

        // Alice's shares are locked inside the vault, not burned.
        assertEq(vault.balanceOf(alice), 0,      "alice shares locked in vault");
        assertEq(vault.balanceOf(bob),   AMOUNT, "bob shares unaffected");

        // Effective supply deducts locked shares; effective assets deduct pending claim.
        assertEq(vault.totalSupply(), AMOUNT,    "effective supply = 100k (bob only)");
        assertEq(vault.totalAssets(), AMOUNT,    "effective assets = 100k (bob's portion)");

        // Verify queue entry.
        InflowWithdrawalQueueLib.WithdrawalEntry memory entry = vault.withdrawalRequest(withdrawalId);
        assertEq(entry.owner,           alice,  "withdrawal owner = alice");
        assertEq(entry.receiver,        alice,  "withdrawal receiver = alice");
        assertEq(entry.sharesLocked,    AMOUNT, "sharesLocked = 100k");
        assertEq(entry.amountToReceive, AMOUNT, "amountToReceive = 100k USDC");
        assertFalse(entry.isFunded,             "not yet funded");

        uint256[] memory aliceIds = vault.getUserWithdrawalIds(alice);
        assertEq(aliceIds.length, 1,            "alice has 1 queued withdrawal");
        assertEq(aliceIds[0],     withdrawalId, "queued withdrawal id = 0");

        // Bob's queue is untouched.
        assertEq(vault.getUserWithdrawalIds(bob).length, 0, "bob has no queued withdrawals");

        // ── Step 5: Alice cancels the queued withdrawal ───────────────────────
        uint256[] memory cancelIds = new uint256[](1);
        cancelIds[0] = withdrawalId;

        vm.expectEmit(true, true, false, false, address(vault));
        emit InflowVault.WithdrawalCancelled(withdrawalId, alice);

        vm.prank(alice);
        vault.cancelWithdrawal(cancelIds);

        // Alice's shares are restored; totals back to post-step-3 levels.
        assertEq(vault.balanceOf(alice),    AMOUNT,       "alice shares restored to 100k");
        assertEq(vault.balanceOf(bob),      AMOUNT,       "bob shares unchanged");
        assertEq(vault.totalSupply(),       2 * AMOUNT,   "total supply = 200k");
        assertEq(vault.totalAssets(),       2 * AMOUNT,   "total assets = 200k");

        // Queue entry deleted.
        assertEq(vault.withdrawalRequest(withdrawalId).initiatedAt, 0, "cancelled entry cleared");
        assertEq(vault.getUserWithdrawalIds(alice).length, 0,          "alice queue empty");

        // ── Step 6: Alice deposits another 50,000 USDC ────────────────────────
        // Share price is still 1:1 (200k assets / 200k supply).
        // previewDeposit(50k): 50k * (200k+1)/(200k+1) = 50k shares.
        uint256 secondDeposit = 50_000e6;

        vm.startPrank(alice);
        usdc.approve(address(vault), secondDeposit);
        uint256 newShares = vault.deposit(secondDeposit, alice);
        vm.stopPrank();

        assertEq(newShares, secondDeposit,                         "50k new shares at 1:1");
        assertEq(vault.balanceOf(alice),         150_000e6,        "alice total shares = 150k");
        assertEq(vault.balanceOf(bob),           AMOUNT,           "bob shares still 100k");
        assertEq(vault.totalSupply(),            250_000e6,        "total supply = 250k");
        assertEq(usdc.balanceOf(address(vault)), secondDeposit,    "vault USDC balance = 50k");
        assertEq(vault.deployedAmount(),         2 * AMOUNT,       "deployedAmount unchanged at 200k");
        assertEq(vault.totalAssets(),            250_000e6,        "total assets = 250k (50k vault + 200k deployed)");
    }
}
