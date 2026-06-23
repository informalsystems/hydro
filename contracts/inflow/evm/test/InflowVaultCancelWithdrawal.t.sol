// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Test} from "forge-std/Test.sol";
import {ERC20} from "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {ERC1967Proxy} from "@openzeppelin/contracts/proxy/ERC1967/ERC1967Proxy.sol";
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
    address user = makeAddr("user");
    address alice = makeAddr("alice");
    address bob = makeAddr("bob");

    uint256 constant AMOUNT = 100_000e6; // 100,000 USDC (6 decimals)
    uint256 constant DEPOSIT_CAP = 1_000_000e6;
    uint256 constant MAX_WITHDRAWALS = 10;

    function setUp() public {
        usdc = new MockUSDC();

        address[] memory wl = new address[](1);
        wl[0] = admin;

        address[] memory deployedAmountWl = new address[](1);
        deployedAmountWl[0] = admin;

        InflowVault impl = new InflowVault();
        bytes memory initData = abi.encodeCall(
            InflowVault.initialize,
            (
                IERC20(address(usdc)),
                "Hydro Inflow Vault",
                "hvUSDC",
                DEPOSIT_CAP,
                MAX_WITHDRAWALS,
                wl,
                deployedAmountWl,
                0,
                address(0)
            )
        );
        vault = InflowVault(address(new ERC1967Proxy(address(impl), initData)));

        usdc.mint(user, AMOUNT);
        // Alice needs 150k: 100k initial + 50k for the second deposit.
        usdc.mint(alice, 150_000e6);
        usdc.mint(bob, AMOUNT);
    }

    /// @notice Scenario:
    ///   1. User deposits 100,000 USDC -> receives 100,000 shares.
    ///   2. Whitelisted admin calls withdrawForDeployment(100,000) -> vault balance goes to 0.
    ///   3. User redeems 100,000 shares -> vault has no free balance, so the
    ///      withdrawal is queued. Shares are burned immediately.
    ///   4. User cancels the queued withdrawal -> shares are re-minted.
    function test_depositWithdrawForDeploymentRedeemAndCancelWithdrawal() public {
        // ── Step 1: deposit ──────────────────────────────────────────────────
        vm.startPrank(user);
        usdc.approve(address(vault), AMOUNT);
        uint256 sharesReceived = vault.deposit(AMOUNT, user);
        vm.stopPrank();

        assertEq(sharesReceived, AMOUNT, "shares minted should equal deposited assets (1:1)");
        assertEq(vault.balanceOf(user), AMOUNT, "user should hold 100,000 shares");
        assertEq(usdc.balanceOf(address(vault)), AMOUNT, "vault USDC balance should be 100,000");

        // ── Step 2: admin withdraws for deployment ───────────────────────────
        vm.prank(admin);
        vault.withdrawForDeployment(AMOUNT);

        assertEq(usdc.balanceOf(address(vault)), 0, "vault balance should be 0 after deployment withdrawal");
        assertEq(vault.deployedAmount(), AMOUNT, "deployedAmount should be 100,000 USDC");
        assertEq(usdc.balanceOf(admin), AMOUNT, "admin should hold the deployed USDC");

        // ── Step 3: user redeems — vault has no free balance -> queued ────────
        uint256 withdrawalId = vault.nextWithdrawalId(); // == 0 (first entry)

        vm.expectEmit(true, true, true, true, address(vault));
        emit InflowVault.WithdrawalQueued(withdrawalId, user, user, AMOUNT, AMOUNT);

        vm.prank(user);
        vault.redeem(AMOUNT, user, user);

        // Shares are burned immediately when queued.
        assertEq(vault.balanceOf(user), 0, "user shares should be burned");

        // Verify the queue entry.
        InflowWithdrawalQueueLib.WithdrawalEntry memory entry = vault.withdrawalRequest(withdrawalId);
        assertEq(entry.id, withdrawalId, "entry id mismatch");
        assertEq(entry.owner, user, "entry owner should be user");
        assertEq(entry.receiver, user, "entry receiver should be user");
        assertEq(entry.sharesBurned, AMOUNT, "entry sharesBurned should be 100,000");
        assertEq(entry.amountToReceive, AMOUNT, "entry amountToReceive should be 100,000 USDC");
        assertFalse(entry.isFunded, "entry should not yet be funded");

        uint256[] memory queuedIds = vault.getUserWithdrawalIds(user);
        assertEq(queuedIds.length, 1, "user should have exactly 1 queued withdrawal");
        assertEq(queuedIds[0], withdrawalId, "queued id should match");

        // ── Step 4: user cancels the queued withdrawal ────────────────────────
        uint256[] memory cancelIds = new uint256[](1);
        cancelIds[0] = withdrawalId;

        vm.expectEmit(true, false, false, true, address(vault));
        emit InflowVault.WithdrawalCancelled(user, cancelIds, AMOUNT, AMOUNT);

        vm.prank(user);
        vault.cancelWithdrawal(cancelIds);

        // Shares returned to user.
        assertEq(vault.balanceOf(user), AMOUNT, "user should have 100,000 shares restored");

        // Queue entry cleared.
        InflowWithdrawalQueueLib.WithdrawalEntry memory cancelled = vault.withdrawalRequest(withdrawalId);
        assertEq(cancelled.initiatedAt, 0, "cancelled entry should be deleted");

        uint256[] memory remainingIds = vault.getUserWithdrawalIds(user);
        assertEq(remainingIds.length, 0, "user withdrawal queue should be empty");
    }

    /// @notice Interleaved-deployment scenario with yield simulation:
    ///   1. Alice deposits 100,000 USDC  -> 100,000 shares (1:1).
    ///   2. Admin withdrawForDeployment(100,000) [between deposits]
    ///      -> vault: 0, deployedAmount: 100k.
    ///   2b.Admin submitDeployedAmount(120,000) — reports 20k yield earned during deployment.
    ///      deployedAmount: 120k. Share price rises to 1.2 (120k assets / 100k shares).
    ///   3. Bob deposits 60,000 USDC -> 50,000 shares (price 1.2:1, not 1:1).
    ///      Vault: 60k. Supply: 150k. TotalAssets: 180k.
    ///   4. Alice redeems 100,000 shares -> assets ≈ 120k (100k × 1.2);
    ///      vault free balance = 60k < 120k -> queued (id 0). Shares burned immediately.
    ///      Effective supply = 150k − 100k (burned) = 50k.
    ///      Effective assets ≈ 180k − 120k = 60k.
    ///   5. Admin attempts withdrawForDeployment(60,000) [between redeem and cancel].
    ///      Queue reserves ≈ 120k (> vault balance of 60k) -> available = 0 -> REVERTS.
    ///   6. Alice cancels the queued withdrawal -> 100,000 shares returned. Queue cleared.
    ///      Supply: 150k. TotalAssets: 180k.
    ///   7. Admin withdrawForDeployment(60,000) [after cancel] -> SUCCEEDS.
    ///      Vault: 0. DeployedAmount: 180k. TotalAssets: 180k.
    function test_interleaved_deployments_queueProtectsAndCancelUnlocks() public {
        uint256 bobDeposit = 60_000e6;
        uint256 deployedWithYield = 120_000e6; // 100k deployed + 20k yield

        // ── Step 1: Alice deposits 100,000 USDC ──────────────────────────────
        vm.startPrank(alice);
        usdc.approve(address(vault), AMOUNT);
        uint256 aliceShares = vault.deposit(AMOUNT, alice);
        vm.stopPrank();

        assertEq(aliceShares, AMOUNT, "alice: 100k shares at 1:1");
        assertEq(vault.totalSupply(), AMOUNT, "total supply = 100k");
        assertEq(vault.totalAssets(), AMOUNT, "total assets = 100k");

        // ── Step 2: Admin deploys Alice's full deposit [between deposits] ─────
        vm.prank(admin);
        vault.withdrawForDeployment(AMOUNT);

        assertEq(usdc.balanceOf(address(vault)), 0, "vault balance = 0 after first deployment");
        assertEq(vault.deployedAmount(), AMOUNT, "deployedAmount = 100k");
        assertEq(vault.totalAssets(), AMOUNT, "totalAssets = 100k");

        // ── Step 2b: Simulate yield — admin reports updated deployment value ──
        // Deployment earned 20k: submit 120k instead of 100k.
        // This sets deployedAmount = 120k, raising the share price to 1.2 (120k / 100k).
        vm.prank(admin);
        vault.submitDeployedAmount(deployedWithYield);

        assertEq(vault.deployedAmount(), deployedWithYield, "deployedAmount = 120k (100k + 20k yield)");
        assertEq(vault.totalAssets(), deployedWithYield, "totalAssets = 120k");
        assertEq(vault.totalSupply(), AMOUNT, "supply unchanged = 100k");
        // Share price: 120k assets / 100k shares = 1.2 USDC per share.

        // ── Step 3: Bob deposits 60,000 USDC ─────────────────────────────────
        // Price is now 1.2:1 -> Bob receives 60k / 1.2 = 50k shares.
        // shares = floor(60k × (100k+1) / (120k+1)) = 50,000e6 (exact).
        vm.startPrank(bob);
        usdc.approve(address(vault), bobDeposit);
        uint256 bobShares = vault.deposit(bobDeposit, bob);
        vm.stopPrank();

        uint256 bobExpectedShares = 50_000e6;
        assertEq(bobShares, bobExpectedShares, "bob: 50k shares (price 1.2:1)");
        assertEq(usdc.balanceOf(address(vault)), bobDeposit, "vault holds 60k");
        assertEq(vault.totalSupply(), AMOUNT + bobExpectedShares, "total supply = 150k");
        assertEq(vault.totalAssets(), bobDeposit + deployedWithYield, "total assets = 180k (60k vault + 120k deployed)");

        // ── Step 4: Alice redeems 100k shares -> queued ────────────────────────
        // previewRedeem(100k) = floor(100k × (180k+1) / (150k+1)) = 119,999.866 ≈ 120k.
        // Free balance = vault (60k) < ~120k needed -> queued.
        uint256 aliceRedemptionAssets = vault.previewRedeem(AMOUNT);
        uint256 withdrawalId = vault.nextWithdrawalId(); // 0

        vm.expectEmit(true, true, true, true, address(vault));
        emit InflowVault.WithdrawalQueued(withdrawalId, alice, alice, AMOUNT, aliceRedemptionAssets);

        vm.prank(alice);
        vault.redeem(AMOUNT, alice, alice);

        // Shares are burned immediately when queued.
        assertEq(vault.balanceOf(alice), 0, "alice shares burned");

        // Effective supply / assets reflect only Bob's position.
        assertEq(vault.totalSupply(), bobExpectedShares, "effective supply = 50k (bob only)");
        assertEq(
            vault.totalAssets(),
            bobDeposit + deployedWithYield - aliceRedemptionAssets,
            "effective assets = 180k - queued (~60k)"
        );

        InflowWithdrawalQueueLib.WithdrawalEntry memory entry = vault.withdrawalRequest(withdrawalId);
        assertEq(entry.owner, alice, "entry owner = alice");
        assertEq(entry.sharesBurned, AMOUNT, "sharesBurned = 100k");
        assertEq(entry.amountToReceive, aliceRedemptionAssets, "amountToReceive ~120k");
        assertFalse(entry.isFunded, "not funded");

        // ── Step 5: Admin attempts second deployment while withdrawal is pending ─
        // totalWithdrawalAmount ≈ 120k; vaultBalance = 60k -> available = 0 -> reverts.
        // The queue shields all 60k vault funds (they are covered by the pending claim).
        vm.expectRevert(InflowVault.ZeroAmount.selector);
        vm.prank(admin);
        vault.withdrawForDeployment(bobDeposit);

        // Vault state unchanged after the failed call.
        assertEq(usdc.balanceOf(address(vault)), bobDeposit, "vault balance unchanged after failed deployment");
        assertEq(vault.deployedAmount(), deployedWithYield, "deployedAmount unchanged after failed deployment");

        // ── Step 6: Alice cancels the queued withdrawal ───────────────────────
        // Because the vault share price is ~1.2 at cancel time, convertToShares(aliceRedemptionAssets)
        // yields slightly fewer shares than the 100k originally burned (double floor rounding).
        // The vault mints min(recalcShares, sharesBurned), so Alice gets back recalcShares.
        uint256 recalcShares = vault.convertToShares(aliceRedemptionAssets);
        uint256 aliceSharesRestored = recalcShares < AMOUNT ? recalcShares : AMOUNT;

        uint256[] memory cancelIds = new uint256[](1);
        cancelIds[0] = withdrawalId;

        vm.expectEmit(true, false, false, true, address(vault));
        emit InflowVault.WithdrawalCancelled(alice, cancelIds, AMOUNT, aliceSharesRestored);

        vm.prank(alice);
        vault.cancelWithdrawal(cancelIds);

        assertEq(vault.balanceOf(alice), aliceSharesRestored, "alice shares restored");
        assertEq(vault.balanceOf(bob), bobExpectedShares, "bob shares unaffected");
        assertEq(vault.totalSupply(), aliceSharesRestored + bobExpectedShares, "total supply after cancel");
        assertEq(vault.totalAssets(), bobDeposit + deployedWithYield, "total assets = 180k");
        assertEq(vault.withdrawalRequest(withdrawalId).initiatedAt, 0, "entry deleted");
        assertEq(vault.getUserWithdrawalIds(alice).length, 0, "alice queue empty");

        // ── Step 7: Admin can now deploy the 60k freed by the cancellation ────
        vm.prank(admin);
        vault.withdrawForDeployment(bobDeposit);

        assertEq(usdc.balanceOf(address(vault)), 0, "vault = 0 after final deployment");
        assertEq(vault.deployedAmount(), deployedWithYield + bobDeposit, "deployedAmount = 180k");
        assertEq(vault.totalAssets(), deployedWithYield + bobDeposit, "totalAssets = 180k (fully deployed)");
    }

    /// @notice Two-user scenario:
    ///   1. Alice deposits 100,000 USDC  -> 100,000 shares (price 1:1).
    ///   2. Bob   deposits 100,000 USDC  -> 100,000 shares (price 1:1).
    ///      Vault holds 200,000 USDC; total supply = 200,000 shares.
    ///   3. Admin withdrawForDeployment(200,000) -> vault balance = 0,
    ///      deployedAmount = 200,000. Share price unchanged (1:1).
    ///   4. Alice redeems 100,000 shares -> no free balance -> queued. Shares burned immediately.
    ///      Effective supply = 200k − 100k (burned) = 100k (Bob only).
    ///      Effective totalAssets = 200k − 100k (pending) = 100k.
    ///   5. Alice cancels the queued withdrawal -> 100,000 shares re-minted.
    ///      Supply and totalAssets both restored to 200,000.
    ///   6. Alice deposits 50,000 USDC -> 50,000 shares (price still 1:1).
    ///      Alice total: 150,000 shares. Bob: 100,000 shares.
    ///      totalSupply = 250,000; totalAssets = 250,000.
    function test_twoUsers_depositRedeemCancelAndRedeposit() public {
        // ── Step 1: Alice deposits 100,000 USDC ──────────────────────────────
        vm.startPrank(alice);
        usdc.approve(address(vault), AMOUNT);
        uint256 aliceShares = vault.deposit(AMOUNT, alice);
        vm.stopPrank();

        assertEq(aliceShares, AMOUNT, "alice: 100k shares at 1:1");
        assertEq(vault.balanceOf(alice), AMOUNT, "alice share balance");
        assertEq(usdc.balanceOf(address(vault)), AMOUNT, "vault holds 100k USDC");
        assertEq(vault.totalSupply(), AMOUNT, "total supply = 100k");
        assertEq(vault.totalAssets(), AMOUNT, "total assets = 100k");

        // ── Step 2: Bob deposits 100,000 USDC ────────────────────────────────
        vm.startPrank(bob);
        usdc.approve(address(vault), AMOUNT);
        uint256 bobShares = vault.deposit(AMOUNT, bob);
        vm.stopPrank();

        assertEq(bobShares, AMOUNT, "bob: 100k shares at 1:1");
        assertEq(vault.balanceOf(bob), AMOUNT, "bob share balance");
        assertEq(usdc.balanceOf(address(vault)), 2 * AMOUNT, "vault holds 200k USDC");
        assertEq(vault.totalSupply(), 2 * AMOUNT, "total supply = 200k");
        assertEq(vault.totalAssets(), 2 * AMOUNT, "total assets = 200k");

        // ── Step 3: admin withdraws all 200,000 for deployment ────────────────
        vm.prank(admin);
        vault.withdrawForDeployment(2 * AMOUNT);

        assertEq(usdc.balanceOf(address(vault)), 0, "vault balance = 0");
        assertEq(vault.deployedAmount(), 2 * AMOUNT, "deployedAmount = 200k");
        assertEq(vault.totalAssets(), 2 * AMOUNT, "totalAssets unchanged (tracked via deployedAmount)");
        assertEq(vault.totalSupply(), 2 * AMOUNT, "total supply unchanged");

        // ── Step 4: Alice redeems 100,000 shares -> queued ─────────────────────
        // previewRedeem(100k): 100k * (200k+1)/(200k+1) = 100k assets
        uint256 withdrawalId = vault.nextWithdrawalId(); // == 0

        vm.expectEmit(true, true, true, true, address(vault));
        emit InflowVault.WithdrawalQueued(withdrawalId, alice, alice, AMOUNT, AMOUNT);

        vm.prank(alice);
        vault.redeem(AMOUNT, alice, alice);

        // Shares are burned immediately when queued.
        assertEq(vault.balanceOf(alice), 0, "alice shares burned");
        assertEq(vault.balanceOf(bob), AMOUNT, "bob shares unaffected");

        // Burned shares reduce totalSupply; pending claim reduces totalAssets.
        assertEq(vault.totalSupply(), AMOUNT, "effective supply = 100k (bob only)");
        assertEq(vault.totalAssets(), AMOUNT, "effective assets = 100k (bob's portion)");

        // Verify queue entry.
        InflowWithdrawalQueueLib.WithdrawalEntry memory entry = vault.withdrawalRequest(withdrawalId);
        assertEq(entry.owner, alice, "withdrawal owner = alice");
        assertEq(entry.receiver, alice, "withdrawal receiver = alice");
        assertEq(entry.sharesBurned, AMOUNT, "sharesBurned = 100k");
        assertEq(entry.amountToReceive, AMOUNT, "amountToReceive = 100k USDC");
        assertFalse(entry.isFunded, "not yet funded");

        uint256[] memory aliceIds = vault.getUserWithdrawalIds(alice);
        assertEq(aliceIds.length, 1, "alice has 1 queued withdrawal");
        assertEq(aliceIds[0], withdrawalId, "queued withdrawal id = 0");

        // Bob's queue is untouched.
        assertEq(vault.getUserWithdrawalIds(bob).length, 0, "bob has no queued withdrawals");

        // ── Step 5: Alice cancels the queued withdrawal ───────────────────────
        uint256[] memory cancelIds = new uint256[](1);
        cancelIds[0] = withdrawalId;

        vm.expectEmit(true, false, false, true, address(vault));
        emit InflowVault.WithdrawalCancelled(alice, cancelIds, AMOUNT, AMOUNT);

        vm.prank(alice);
        vault.cancelWithdrawal(cancelIds);

        // Alice's shares are restored; totals back to post-step-3 levels.
        assertEq(vault.balanceOf(alice), AMOUNT, "alice shares restored to 100k");
        assertEq(vault.balanceOf(bob), AMOUNT, "bob shares unchanged");
        assertEq(vault.totalSupply(), 2 * AMOUNT, "total supply = 200k");
        assertEq(vault.totalAssets(), 2 * AMOUNT, "total assets = 200k");

        // Queue entry deleted.
        assertEq(vault.withdrawalRequest(withdrawalId).initiatedAt, 0, "cancelled entry cleared");
        assertEq(vault.getUserWithdrawalIds(alice).length, 0, "alice queue empty");

        // ── Step 6: Alice deposits another 50,000 USDC ────────────────────────
        // Share price is still 1:1 (200k assets / 200k supply).
        // previewDeposit(50k): 50k * (200k+1)/(200k+1) = 50k shares.
        uint256 secondDeposit = 50_000e6;

        vm.startPrank(alice);
        usdc.approve(address(vault), secondDeposit);
        uint256 newShares = vault.deposit(secondDeposit, alice);
        vm.stopPrank();

        assertEq(newShares, secondDeposit, "50k new shares at 1:1");
        assertEq(vault.balanceOf(alice), 150_000e6, "alice total shares = 150k");
        assertEq(vault.balanceOf(bob), AMOUNT, "bob shares still 100k");
        assertEq(vault.totalSupply(), 250_000e6, "total supply = 250k");
        assertEq(usdc.balanceOf(address(vault)), secondDeposit, "vault USDC balance = 50k");
        assertEq(vault.deployedAmount(), 2 * AMOUNT, "deployedAmount unchanged at 200k");
        assertEq(vault.totalAssets(), 250_000e6, "total assets = 250k (50k vault + 200k deployed)");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Edge-case cancel tests
    // Corresponds to: cancel_withdrawal_test (vault/testing.rs)
    // ═══════════════════════════════════════════════════════════════════════

    // ── skipping rules ────────────────────────────────────────────────────────

    /// Funded IDs (at or below lastFundedWithdrawalId) must be skipped.
    function test_cancel_skips_funded_ids() public {
        // User deposits and redeems — vault has no balance, so withdrawal is queued.
        vm.startPrank(user);
        usdc.approve(address(vault), AMOUNT);
        vault.deposit(AMOUNT, user);
        vm.stopPrank();

        vm.prank(admin);
        vault.withdrawForDeployment(AMOUNT);

        vm.prank(user);
        vault.redeem(AMOUNT, user, user); // id 0 queued

        // Admin returns funds and fulfills.
        usdc.mint(admin, AMOUNT);
        vm.prank(admin);
        usdc.approve(address(vault), AMOUNT);
        vm.prank(admin);
        vault.depositFromDeployment(AMOUNT);
        vault.fulfillPendingWithdrawals(10); // id 0 funded

        // Attempt to cancel id 0 — it's funded -> skipped -> nothing happens.
        uint256[] memory ids = new uint256[](1);
        ids[0] = 0;
        vm.prank(user);
        vault.cancelWithdrawal(ids);

        // Shares must NOT be re-minted (user still has 0).
        assertEq(vault.balanceOf(user), 0, "funded id must not be cancellable");

        // Entry still exists and is funded.
        assertTrue(vault.withdrawalRequest(0).isFunded, "entry still funded");
    }

    /// IDs owned by a different address are silently skipped.
    function test_cancel_skips_non_owner_ids() public {
        // Alice queues a withdrawal.
        vm.startPrank(alice);
        usdc.approve(address(vault), AMOUNT);
        vault.deposit(AMOUNT, alice);
        vm.stopPrank();

        vm.prank(admin);
        vault.withdrawForDeployment(AMOUNT);

        vm.prank(alice);
        vault.redeem(AMOUNT, alice, alice); // id 0, owner = alice

        // Bob tries to cancel alice's withdrawal.
        uint256[] memory ids = new uint256[](1);
        ids[0] = 0;
        vm.prank(bob);
        vault.cancelWithdrawal(ids); // silently skipped — bob does not own id 0

        // alice's entry unchanged.
        InflowWithdrawalQueueLib.WithdrawalEntry memory e = vault.withdrawalRequest(0);
        assertEq(e.owner, alice, "alice's entry intact");
        assertEq(vault.balanceOf(bob), 0, "bob received nothing");
    }

    /// Non-existent IDs are silently skipped.
    function test_cancel_skips_nonexistent_ids() public {
        uint256[] memory ids = new uint256[](2);
        ids[0] = 999;
        ids[1] = 12345;
        // Must not revert.
        vm.prank(user);
        vault.cancelWithdrawal(ids);
    }

    /// Passing the same ID twice should only cancel it once.
    function test_cancel_deduplicates_ids() public {
        vm.startPrank(user);
        usdc.approve(address(vault), AMOUNT);
        vault.deposit(AMOUNT, user);
        vm.stopPrank();

        vm.prank(admin);
        vault.withdrawForDeployment(AMOUNT);

        vm.prank(user);
        vault.redeem(AMOUNT, user, user); // id 0

        uint256[] memory ids = new uint256[](3);
        ids[0] = 0;
        ids[1] = 0;
        ids[2] = 0;

        vm.prank(user);
        vault.cancelWithdrawal(ids);

        // Exactly AMOUNT shares re-minted (not 3×AMOUNT).
        assertEq(vault.balanceOf(user), AMOUNT, "shares minted exactly once");
    }

    // ── share recalculation ────────────────────────────────────────────────────

    /// When share price is unchanged, the user gets back exactly sharesBurned.
    function test_cancel_remints_original_shares_when_price_unchanged() public {
        vm.startPrank(user);
        usdc.approve(address(vault), AMOUNT);
        vault.deposit(AMOUNT, user);
        vm.stopPrank();

        vm.prank(admin);
        vault.withdrawForDeployment(AMOUNT);

        vm.prank(user);
        vault.redeem(AMOUNT, user, user); // id 0, price still 1:1

        uint256[] memory ids = new uint256[](1);
        ids[0] = 0;
        vm.prank(user);
        vault.cancelWithdrawal(ids);

        assertEq(vault.balanceOf(user), AMOUNT, "all shares restored at unchanged price");
    }

    /// When share price has increased since queuing, the user gets back fewer shares
    /// (min of sharesBurned and recalculated shares).
    function test_cancel_remints_min_of_burned_and_recalculated_shares() public {
        // Alice deposits 100k, admin deploys, reports 120k yield -> price = 1.2.
        vm.startPrank(alice);
        usdc.approve(address(vault), AMOUNT);
        vault.deposit(AMOUNT, alice);
        vm.stopPrank();

        vm.prank(admin);
        vault.withdrawForDeployment(AMOUNT);

        vm.prank(admin);
        vault.submitDeployedAmount(120_000e6); // price now 1.2

        // Alice redeems -> queued (no free balance).
        vm.prank(alice);
        vault.redeem(AMOUNT, alice, alice); // id 0, burned 100k shares; amountToReceive ~120k

        // Now cancel: recalc = convertToShares(~120k) at price 1.2 ≈ 100k shares.
        // min(100k burned, ~100k recalc) = recalc.
        uint256 recalc = vault.convertToShares(vault.withdrawalRequest(0).amountToReceive);
        uint256 expectedMint = recalc < AMOUNT ? recalc : AMOUNT;

        uint256[] memory ids = new uint256[](1);
        ids[0] = 0;
        vm.prank(alice);
        vault.cancelWithdrawal(ids);

        assertEq(vault.balanceOf(alice), expectedMint, "min(burned, recalc) shares minted");
        assertLe(vault.balanceOf(alice), AMOUNT, "never exceeds original shares burned");
    }

    // ── deposit cap check ─────────────────────────────────────────────────────

    /// cancelWithdrawal must revert if restoring the assets would push totalAssets above depositCap.
    function test_cancel_reverts_if_deposit_cap_exceeded() public {
        vm.startPrank(user);
        usdc.approve(address(vault), AMOUNT);
        vault.deposit(AMOUNT, user);
        vm.stopPrank();

        vm.prank(admin);
        vault.withdrawForDeployment(AMOUNT);

        vm.prank(user);
        vault.redeem(AMOUNT, user, user); // id 0

        // Lower deposit cap below what the cancel would restore.
        // Currently: totalAssets = deployedAmount(100k) - pendingWithdrawal(100k) = 0.
        // After cancel: totalAssets would become 100k again.
        vm.prank(admin);
        vault.updateDepositCap(50e6); // cap = 50 USDC — below 100k

        uint256[] memory ids = new uint256[](1);
        ids[0] = 0;
        vm.prank(user);
        vm.expectRevert(InflowVault.DepositCapReached.selector);
        vault.cancelWithdrawal(ids);
    }

    // ── partial batch ─────────────────────────────────────────────────────────

    /// A batch where some IDs are valid and some are funded — only valid ones processed.
    function test_cancel_partial_batch() public {
        _mintUsers();

        // Two users queue withdrawals.
        vm.startPrank(alice);
        usdc.approve(address(vault), AMOUNT);
        vault.deposit(AMOUNT, alice);
        vm.stopPrank();

        vm.startPrank(bob);
        usdc.approve(address(vault), AMOUNT);
        vault.deposit(AMOUNT, bob);
        vm.stopPrank();

        vm.prank(admin);
        vault.withdrawForDeployment(2 * AMOUNT);

        vm.prank(alice);
        vault.redeem(AMOUNT, alice, alice); // id 0

        vm.prank(bob);
        vault.redeem(AMOUNT, bob, bob); // id 1

        // Fund id 0.
        usdc.mint(admin, AMOUNT);
        vm.prank(admin);
        usdc.approve(address(vault), AMOUNT);
        vm.prank(admin);
        vault.depositFromDeployment(AMOUNT);
        vault.fulfillPendingWithdrawals(1); // funds id 0

        // Alice passes both IDs — id 0 is funded (skipped), id 1 belongs to bob (skipped).
        uint256[] memory ids = new uint256[](2);
        ids[0] = 0; // funded — skipped
        ids[1] = 1; // owned by bob — skipped for alice
        vm.prank(alice);
        vault.cancelWithdrawal(ids);

        // Nothing should change.
        assertEq(vault.balanceOf(alice), 0, "alice: nothing minted");
    }

    // ── queue info update ─────────────────────────────────────────────────────

    function test_cancel_updates_queue_info() public {
        vm.startPrank(user);
        usdc.approve(address(vault), AMOUNT);
        vault.deposit(AMOUNT, user);
        vm.stopPrank();

        vm.prank(admin);
        vault.withdrawForDeployment(AMOUNT);

        vm.prank(user);
        vault.redeem(AMOUNT, user, user);

        InflowWithdrawalQueueLib.WithdrawalQueueInfo memory before = vault.withdrawalQueueInfo();
        assertEq(before.totalWithdrawalAmount, AMOUNT);
        assertEq(before.nonFundedWithdrawalAmount, AMOUNT);

        uint256[] memory ids = new uint256[](1);
        ids[0] = 0;
        vm.prank(user);
        vault.cancelWithdrawal(ids);

        InflowWithdrawalQueueLib.WithdrawalQueueInfo memory after_ = vault.withdrawalQueueInfo();
        assertEq(after_.totalWithdrawalAmount, 0, "totalWithdrawalAmount cleared");
        assertEq(after_.nonFundedWithdrawalAmount, 0, "nonFundedWithdrawalAmount cleared");
    }

    // ── empty array ───────────────────────────────────────────────────────────

    function test_cancel_empty_ids_no_op() public {
        uint256[] memory ids = new uint256[](0);
        vm.prank(user);
        vault.cancelWithdrawal(ids); // must not revert
        assertEq(vault.balanceOf(user), 0);
    }

    // ── duplicate id deduplication ────────────────────────────────────────────

    /// @dev Passing the same ID twice must cancel it exactly once.
    ///
    /// The bug: the deduplication inner loop was missing braces around its body,
    /// so `break` was an unconditional statement rather than part of the `if`.
    /// The loop always exited after checking only cancelableIds[0], so a
    /// duplicate whose first occurrence was NOT at position 0 of the accumulator
    /// went undetected and was counted twice.
    ///
    /// Concretely, ids=[0,1,1] triggers the bug because:
    ///   - after id=0 and id=1 are accepted, cancelableIds=[0,1]
    ///   - when id=1 appears again, the loop checks cancelableIds[0]=0 ≠ 1,
    ///     then breaks without setting isDuplicate — so id=1 is accepted a
    ///     second time, inflating totalSharesBurned and thus sharesToMint.
    function test_cancel_duplicate_id_not_at_position_zero_counted_once() public {
        usdc.mint(user, AMOUNT * 2);
        vm.startPrank(user);
        usdc.approve(address(vault), AMOUNT * 2);
        vault.deposit(AMOUNT * 2, user);
        vm.stopPrank();

        // Drain vault so both redeems are queued rather than fulfilled immediately.
        vm.prank(admin);
        vault.withdrawForDeployment(AMOUNT * 2);

        uint256 halfShares = vault.balanceOf(user) / 2;
        vm.startPrank(user);
        vault.redeem(halfShares, user, user); // queued as id=0
        vault.redeem(vault.balanceOf(user), user, user); // queued as id=1
        vm.stopPrank();

        uint256 shares0 = vault.withdrawalRequest(0).sharesBurned;
        uint256 shares1 = vault.withdrawalRequest(1).sharesBurned;

        // ids=[0,1,1]: id=1 duplicate sits at position 2, past cancelableIds[0].
        uint256[] memory ids = new uint256[](3);
        ids[0] = 0;
        ids[1] = 1;
        ids[2] = 1;

        vm.prank(user);
        vault.cancelWithdrawal(ids);

        assertEq(vault.balanceOf(user), shares0 + shares1, "duplicate id must not inflate share refund");
    }

    // ── internal helper ───────────────────────────────────────────────────────

    function _mintUsers() internal {
        usdc.mint(alice, AMOUNT);
        usdc.mint(bob, AMOUNT);
    }
}
