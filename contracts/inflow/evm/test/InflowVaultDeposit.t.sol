// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {InflowVaultBase, InflowVault, MockAdapterWithAsset} from "./InflowVaultBase.t.sol";

/// @notice Tests for deposit(), mint(), and related ERC-4626 view functions.
/// Corresponds to: deposit_withdrawal_for_deployment_test,
/// deposit_mints_shares_for_on_behalf_recipient,
/// dry_run_deposit_query_test, reporting_balance_queries_test
/// (vault/testing.rs).
contract InflowVaultDepositTest is InflowVaultBase {
    // ── 1:1 initial deposit ───────────────────────────────────────────────────

    function test_deposit_1to1_empty_vault() public {
        uint256 amount = 1_000e6;
        uint256 shares = _deposit(user, amount);

        assertEq(shares, amount, "shares == assets at 1:1");
        assertEq(vault.balanceOf(user), amount);
        assertEq(vault.totalSupply(), amount);
        assertEq(asset.balanceOf(address(vault)), amount);
    }

    function test_deposit_updates_total_assets() public {
        _deposit(user, 500e6);
        assertEq(vault.totalAssets(), 500e6);
        _deposit(alice, 300e6);
        assertEq(vault.totalAssets(), 800e6);
    }

    function test_deposit_for_different_receiver() public {
        uint256 amount = 200e6;
        _depositTo(user, alice, amount);

        assertEq(vault.balanceOf(user), 0, "user should not hold shares");
        assertEq(vault.balanceOf(alice), amount, "alice should hold shares");
    }

    // ── mint() ────────────────────────────────────────────────────────────────

    function test_mint_by_share_amount() public {
        uint256 sharesToMint = 500e6;
        uint256 assetsNeeded = vault.previewMint(sharesToMint);
        asset.mint(user, assetsNeeded);
        vm.prank(user);
        asset.approve(address(vault), assetsNeeded);

        vm.prank(user);
        uint256 assetsUsed = vault.mint(sharesToMint, user);

        assertEq(assetsUsed, assetsNeeded);
        assertEq(vault.balanceOf(user), sharesToMint);
        assertEq(vault.totalSupply(), sharesToMint);
    }

    // ── deposit at non-unit share price ───────────────────────────────────────

    function test_deposit_at_non_unit_share_price() public {
        // Alice deposits 100k; admin reports 120k deployed (20% yield).
        uint256 aliceDeposit = 100_000e6;
        _deposit(alice, aliceDeposit);

        vm.prank(admin);
        vault.withdrawForDeployment(aliceDeposit);

        vm.prank(admin);
        vault.submitDeployedAmount(120_000e6); // price = 1.2

        // Bob deposits 60k at price 1.2 -> should receive 50k shares.
        uint256 bobDeposit = 60_000e6;
        uint256 bobShares = _deposit(bob, bobDeposit);

        assertEq(bobShares, 50_000e6, "bob should receive 50k shares at 1.2:1 price");
    }

    // ── previewDeposit / previewMint ──────────────────────────────────────────

    function test_preview_deposit_empty_vault() public view {
        assertEq(vault.previewDeposit(1_000e6), 1_000e6, "1:1 on empty vault");
    }

    function test_preview_deposit_with_yield() public {
        _deposit(alice, 100_000e6);
        vm.prank(admin);
        vault.withdrawForDeployment(100_000e6);
        vm.prank(admin);
        vault.submitDeployedAmount(110_000e6); // 10% yield -> price 1.1

        uint256 expected = vault.previewDeposit(11_000e6);
        assertEq(expected, 10_000e6, "11k assets at 1.1 price = 10k shares");
    }

    function test_preview_mint_empty_vault() public view {
        assertEq(vault.previewMint(500e6), 500e6, "1:1 on empty vault");
    }

    // ── maxDeposit / maxMint ──────────────────────────────────────────────────

    function test_max_deposit() public {
        assertEq(vault.maxDeposit(user), DEPOSIT_CAP);
        _deposit(alice, 100_000e6);
        assertEq(vault.maxDeposit(user), DEPOSIT_CAP - 100_000e6);
    }

    function test_max_deposit_zero_when_cap_reached() public {
        vm.prank(admin);
        vault.updateDepositCap(100e6);

        _deposit(alice, 100e6);
        assertEq(vault.maxDeposit(user), 0, "cap reached");
    }

    function test_max_mint() public view {
        uint256 room = vault.maxDeposit(address(0));
        uint256 maxShares = vault.maxMint(user);
        // In an empty vault (price 1:1) maxMint == maxDeposit.
        assertEq(maxShares, room);
    }

    function test_max_mint_zero_when_cap_reached() public {
        vm.prank(admin);
        vault.updateDepositCap(100e6);
        _deposit(alice, 100e6);
        assertEq(vault.maxMint(user), 0);
    }

    // ── deposit cap enforcement ───────────────────────────────────────────────

    function test_deposit_fails_if_cap_exceeded() public {
        vm.prank(admin);
        vault.updateDepositCap(500e6);

        asset.mint(user, 501e6);
        vm.prank(user);
        asset.approve(address(vault), 501e6);

        vm.prank(user);
        vm.expectRevert();
        vault.deposit(501e6, user);
    }

    function test_deposit_exactly_at_cap_succeeds() public {
        vm.prank(admin);
        vault.updateDepositCap(500e6);

        _deposit(user, 500e6);
        assertEq(vault.totalAssets(), 500e6);
    }

    // ── multiple users ────────────────────────────────────────────────────────

    function test_multiple_users_deposit() public {
        _deposit(alice, 100_000e6);
        _deposit(bob, 200_000e6);

        assertEq(vault.totalSupply(), 300_000e6);
        assertEq(vault.totalAssets(), 300_000e6);
        assertEq(vault.balanceOf(alice), 100_000e6);
        assertEq(vault.balanceOf(bob), 200_000e6);
    }

    // ── zero amount ───────────────────────────────────────────────────────────

    function test_deposit_zero_amount_reverts() public {
        vm.prank(user);
        vm.expectRevert(InflowVault.ZeroAmount.selector);
        vault.deposit(0, user);
    }

    // ── adapter allocation on deposit ─────────────────────────────────────────

    function test_deposit_allocates_to_automated_adapter() public {
        MockAdapterWithAsset adapter = new MockAdapterWithAsset(address(asset));
        adapter.setDepositCap(address(vault), 50_000e6);
        adapter.setWithdrawCap(address(vault), 50_000e6);
        _registerAdapter("adapter1", adapter, true, false);

        _deposit(user, 100_000e6);

        // Adapter should hold 50k (its capacity), vault keeps the remaining 50k.
        assertEq(asset.balanceOf(address(adapter)), 50_000e6, "adapter holds 50k");
        assertEq(asset.balanceOf(address(vault)), 50_000e6, "vault holds 50k");
    }

    function test_deposit_keeps_funds_when_no_adapters() public {
        _deposit(user, 100_000e6);
        assertEq(asset.balanceOf(address(vault)), 100_000e6);
    }

    function test_deposit_skips_manual_adapter() public {
        MockAdapterWithAsset adapter = new MockAdapterWithAsset(address(asset));
        adapter.setDepositCap(address(vault), 100_000e6);
        _registerAdapter("manual1", adapter, false, false); // automated = false

        _deposit(user, 100_000e6);

        assertEq(asset.balanceOf(address(adapter)), 0, "manual adapter untouched");
        assertEq(asset.balanceOf(address(vault)), 100_000e6, "all funds stay in vault");
    }
}
