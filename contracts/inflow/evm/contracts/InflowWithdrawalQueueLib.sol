// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {SafeERC20} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";

/// @title InflowWithdrawalQueueLib
/// @notice Manages the two-phase withdrawal queue for InflowVault. All external functions
/// execute via DELEGATECALL in the vault's storage context, so token transfers and event
/// emissions are attributed to the vault.
library InflowWithdrawalQueueLib {

    // STRUCTS

    struct WithdrawalEntry {
        uint256 id;
        uint256 initiatedAt;   // block.timestamp at queue time
        address owner;         // shares owner — validated on cancellation
        address receiver;      // receives assets on claim
        uint256 sharesBurned;
        uint256 amountToReceive;
        bool isFunded;
    }

    struct WithdrawalQueueInfo {
        uint256 totalSharesBurned;
        uint256 totalWithdrawalAmount;
        uint256 nonFundedWithdrawalAmount;
    }

    /// @dev All withdrawal-queue state packed into one struct so it can be passed as a
    /// single storage pointer to library functions.
    struct QueueStorage {
        mapping(uint256 => WithdrawalEntry) requests;
        mapping(address => uint256[]) userIds;
        uint256 nextId;
        uint256 lastFundedId;
        bool anyFunded;
        WithdrawalQueueInfo info;
    }

    // ERRORS

    error MaxWithdrawalsReached();
    error NothingFundedYet();

    // EVENTS — declared here for emission; vault re-declares these for ABI discoverability.
    // Topic hashes are identical (same signature), so clients using the vault ABI decode correctly.

    event WithdrawalFunded(uint256 indexed id);
    event WithdrawalClaimed(uint256 indexed id, address indexed receiver, uint256 assets);
    /// @notice Emitted once per cancelled withdrawal ID. The shares re-minted to the owner
    /// after a batch cancellation are reported by the ERC-20 Transfer event from _mint.
    event WithdrawalCancelled(uint256 indexed id, address indexed owner);

    // EXTERNAL FUNCTIONS

    /// @dev Adds a new unfunded entry to the queue. Returns the assigned ID so the vault
    /// can emit WithdrawalQueued.
    function enqueue(
        QueueStorage storage q,
        address owner,
        address receiver,
        uint256 shares,
        uint256 assets,
        uint256 maxPerUser
    ) external returns (uint256 id) {
        id = q.nextId++;
        q.requests[id] = WithdrawalEntry({
            id: id,
            initiatedAt: block.timestamp,
            owner: owner,
            receiver: receiver,
            sharesBurned: shares,
            amountToReceive: assets,
            isFunded: false
        });
        q.userIds[owner].push(id);
        if (q.userIds[owner].length > maxPerUser) revert MaxWithdrawalsReached();

        q.info.totalSharesBurned += shares;
        q.info.totalWithdrawalAmount += assets;
        q.info.nonFundedWithdrawalAmount += assets;
    }

    /// @dev Scans the queue FIFO, marking entries as funded when the vault's free balance
    /// covers them. Processes at most `limit` entries.
    function fulfill(
        QueueStorage storage q,
        uint256 limit,
        address assetAddr
    ) external {
        if (limit == 0) return;

        uint256 start = q.anyFunded ? q.lastFundedId + 1 : 0;
        uint256 end = q.nextId;
        if (start >= end) return;

        uint256 fundedReserve = q.info.totalWithdrawalAmount - q.info.nonFundedWithdrawalAmount;
        uint256 vaultBalance = IERC20(assetAddr).balanceOf(address(this));
        uint256 freeBalance = vaultBalance > fundedReserve ? vaultBalance - fundedReserve : 0;

        uint256 processed;
        uint256 highestFunded;
        bool anyNewFunded;
        uint256 totalAmountFunded;

        for (uint256 id = start; id < end && processed < limit; id++) {
            WithdrawalEntry storage entry = q.requests[id];
            if (entry.initiatedAt == 0) continue;             // cancelled entry
            if (entry.isFunded) continue;                     // already funded (should not occur past start)
            if (entry.amountToReceive > freeBalance) break;   // FIFO: stop at first unaffordable

            entry.isFunded = true;
            freeBalance -= entry.amountToReceive;
            totalAmountFunded += entry.amountToReceive;
            highestFunded = id;
            anyNewFunded = true;
            processed++;
            emit WithdrawalFunded(id);
        }

        if (anyNewFunded) {
            q.info.nonFundedWithdrawalAmount -= totalAmountFunded;
            q.lastFundedId = highestFunded;
            q.anyFunded = true;
        }
    }

    /// @dev Transfers assets to each funded withdrawal's receiver.
    /// Only processes IDs at or below lastFundedId that are marked isFunded.
    function claim(
        QueueStorage storage q,
        uint256[] calldata ids,
        address assetAddr
    ) external {
        if (!q.anyFunded) revert NothingFundedYet();

        for (uint256 i = 0; i < ids.length; i++) {
            uint256 id = ids[i];
            if (id > q.lastFundedId) continue;

            WithdrawalEntry storage entry = q.requests[id];
            // Covers unfunded entries and duplicate IDs (entry deleted on first processing).
            if (!entry.isFunded) continue;

            uint256 amount = entry.amountToReceive;
            address receiver = entry.receiver;
            address owner = entry.owner;
            uint256 shares = entry.sharesBurned;

            q.info.totalSharesBurned -= shares;
            q.info.totalWithdrawalAmount -= amount;

            delete q.requests[id];
            _removeFromUserIds(q, owner, id);

            SafeERC20.safeTransfer(IERC20(assetAddr), receiver, amount);
            emit WithdrawalClaimed(id, receiver, amount);
        }
    }

    /// @dev Cancels unfunded withdrawal requests owned by msg.sender and updates queue totals.
    /// IDs below the funded watermark are skipped to prevent races with fulfill().
    /// Returns (totalAmountRestored, totalSharesBurnedToRemove) so the vault can perform
    /// the deposit-cap check and re-mint shares.
    function cancel(
        QueueStorage storage q,
        uint256[] calldata ids
    ) external returns (uint256 totalAmount, uint256 totalSharesBurnedToRemove) {
        uint256 lowestCancelable = q.anyFunded ? q.lastFundedId + 1 : 0;

        for (uint256 i = 0; i < ids.length; i++) {
            uint256 id = ids[i];
            if (id < lowestCancelable) continue;

            WithdrawalEntry storage entry = q.requests[id];
            if (entry.initiatedAt == 0) continue;    // doesn't exist or already cancelled
            if (entry.owner != msg.sender) continue; // not owned by caller
            if (entry.isFunded) continue;            // already funded, cannot cancel

            totalAmount += entry.amountToReceive;
            totalSharesBurnedToRemove += entry.sharesBurned;

            delete q.requests[id];
            _removeFromUserIds(q, msg.sender, id);
            emit WithdrawalCancelled(id, msg.sender);
        }

        if (totalAmount > 0) {
            q.info.totalSharesBurned -= totalSharesBurnedToRemove;
            q.info.totalWithdrawalAmount -= totalAmount;
            q.info.nonFundedWithdrawalAmount -= totalAmount;
        }
    }

    // INTERNAL HELPERS

    function _removeFromUserIds(QueueStorage storage q, address user, uint256 id) private {
        uint256[] storage ids = q.userIds[user];
        uint256 n = ids.length;
        for (uint256 i = 0; i < n; i++) {
            if (ids[i] == id) {
                ids[i] = ids[n - 1];
                ids.pop();
                return;
            }
        }
    }
}
