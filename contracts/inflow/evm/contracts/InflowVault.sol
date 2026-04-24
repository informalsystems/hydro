// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {ERC4626} from "@openzeppelin/contracts/token/ERC20/extensions/ERC4626.sol";
import {ERC20} from "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {SafeERC20} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import {ReentrancyGuard} from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import {Math} from "@openzeppelin/contracts/utils/math/Math.sol";
import {IAdapter} from "./IAdapter.sol";

/// @title InflowVault
/// @notice ERC-4626 tokenised vault with adapter-based deployment, a two-phase withdrawal
/// queue, and an embedded high-water-mark performance fee system.
///
/// Design highlights
/// -----------------
/// * Vault accepts deposits of a single asset.
/// * totalAssets() = vault balance + untracked adapter positions + deployedAmount
///                 - pending withdrawal reserves.
/// * All-or-nothing withdrawal: if the vault cannot immediately cover a redemption,
///   shares are burned instantly and the claim is queued (FIFO).
/// * Fee accrual is based on a high-water-mark model.
/// * Adapters can be Automated (included in deposit/withdraw flows) or Manual (explicit
///   calls only), and Tracked (counted in deployedAmount) or Untracked (queried directly).
contract InflowVault is ERC4626, ReentrancyGuard {
    using Math for uint256;

    // ERRORS

    error Unauthorized();
    error InvalidFeeRate();
    error FeeRecipientNotSet();
    error NoSharesIssued();
    error AdapterAlreadyExists(string name);
    error AdapterNotFound(string name);
    error ZeroAmount();
    error ZeroAddress();
    error DepositCapReached();
    error MaxWithdrawalsReached();
    error NothingFundedYet();
    error NotWhitelisted();
    error AlreadyWhitelisted();
    error WhitelistCannotBeEmpty();

    // EVENTS

    event WhitelistAdded(address indexed addr);
    event WhitelistRemoved(address indexed addr);
    event DepositCapUpdated(uint256 newCap);
    event MaxWithdrawalsPerUserUpdated(uint256 newMax);

    event AdapterRegistered(string name, address indexed addr, bool automated, bool tracked);
    event AdapterUnregistered(string name, address indexed addr);
    event AdapterAllocationModeUpdated(string name, bool automated);
    event AdapterDeploymentTrackingUpdated(string name, bool tracked);

    event DeployedAmountSubmitted(address indexed caller, uint256 newAmount);
    event FeeConfigUpdated(uint256 feeRate, address feeRecipient);
    event FeesAccrued(
        address indexed recipient,
        uint256 sharesMinted,
        uint256 sharePrice,
        uint256 feeAssets
    );

    /// @notice Emitted when a withdrawal cannot be fulfilled immediately and is queued.
    event WithdrawalQueued(
        uint256 indexed id,
        address indexed owner,
        address indexed receiver,
        uint256 shares,
        uint256 assets
    );
    event WithdrawalFunded(uint256 indexed id);
    event WithdrawalClaimed(uint256 indexed id, address indexed receiver, uint256 assets);
    /// @notice Emitted once per cancelled withdrawal ID. The shares re-minted to the owner
    /// after a batch cancellation are reported by the ERC-20 Transfer event from _mint.
    event WithdrawalCancelled(uint256 indexed id, address indexed owner);

    event WithdrawForDeployment(address indexed caller, uint256 requested, uint256 withdrawn);
    event DepositFromDeployment(address indexed caller, uint256 amount);

    // TYPES

    struct AdapterInfo {
        address addr;
        bool automated;     // true = included in automated deposit/withdraw allocation
        bool tracked;       // true = position is counted in deployedAmount (not queried)
        string name;
        string description;
    }

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

    // CONSTANTS

    /// @dev 1e18 fixed-point unit used for fee rate and share price arithmetic.
    uint256 public constant WAD = 1e18;

    // STATE

    // Access control
    mapping(address => bool) public whitelist;
    uint256 private _whitelistCount;

    // Vault config
    uint256 public depositCap;
    uint256 public maxWithdrawalsPerUser;

    // Deployed amount represents funds that left the vault for external deployment (principal + yield).
    uint256 public deployedAmount;

    // Fee system
    uint256 public feeRate;            // WAD: 0 = disabled, 1e18 = 100 %
    address public feeRecipient;
    uint256 public highWaterMarkPrice; // WAD share price during the last fee accrual

    // Adapters - keyed by keccak256(name) for O(1) lookup; array of keys used for enumeration.
    mapping(bytes32 => AdapterInfo) public adapters;
    bytes32[] private _adapterKeys;

    // Withdrawal queue
    mapping(uint256 => WithdrawalEntry) public withdrawalRequests;
    mapping(address => uint256[]) private _userWithdrawalIds;

    uint256 public nextWithdrawalId;
    uint256 public lastFundedWithdrawalId;
    bool public anyWithdrawalFunded;

    WithdrawalQueueInfo public withdrawalQueueInfo;

    // MODIFIERS

    modifier onlyWhitelisted() {
        if (!whitelist[msg.sender]) revert Unauthorized();
        _;
    }

    // CONSTRUCTOR

    /// @param asset_                 ERC-20 token accepted as deposit.
    /// @param name_                  Vault share token name.
    /// @param symbol_                Vault share token symbol.
    /// @param depositCap_            Maximum total assets the vault will hold (WAD units).
    /// @param maxWithdrawalsPerUser_ Maximum concurrent queued withdrawals per address.
    /// @param initialWhitelist       At least one address must be provided.
    /// @param feeRate_               Performance fee rate in WAD (0 = disabled, 1e18 = 100%).
    /// @param feeRecipient_          Recipient of fee shares; required when feeRate_ > 0.
    constructor(
        IERC20 asset_,
        string memory name_,
        string memory symbol_,
        uint256 depositCap_,
        uint256 maxWithdrawalsPerUser_,
        address[] memory initialWhitelist,
        uint256 feeRate_,
        address feeRecipient_
    ) ERC4626(asset_) ERC20(name_, symbol_) {
        if (initialWhitelist.length == 0) revert WhitelistCannotBeEmpty();
        if (feeRate_ > WAD) revert InvalidFeeRate();
        if (feeRate_ > 0 && feeRecipient_ == address(0)) revert FeeRecipientNotSet();

        depositCap = depositCap_;
        maxWithdrawalsPerUser = maxWithdrawalsPerUser_;
        feeRate = feeRate_;
        feeRecipient = feeRecipient_;
        highWaterMarkPrice = WAD;

        for (uint256 i = 0; i < initialWhitelist.length; i++) {
            address addr = initialWhitelist[i];
            if (addr == address(0)) revert ZeroAddress();
            if (!whitelist[addr]) {
                whitelist[addr] = true;
                _whitelistCount++;
                emit WhitelistAdded(addr);
            }
        }
    }

    // ERC-4626 OVERRIDES

    /// @notice Total assets backing outstanding shares.
    ///
    /// Formula:
    ///   totalAssets = vaultBalance
    ///               + untrackedAdapterPositions   (queried; tracked adapters positions are already in deployedAmount)
    ///               + deployedAmount
    ///               - totalWithdrawalAmount        (funds already committed to pending withdrawers)
    ///
    /// Subtracting pending withdrawal reserves prevents them from being double-counted
    /// as backing for remaining shares after the corresponding shares have been burned.
    function totalAssets() public view override returns (uint256) {
        uint256 balance = IERC20(asset()).balanceOf(address(this));
        uint256 adapterPositions = _queryUntrackedAdapterPositions();
        uint256 pendingWithdrawals = withdrawalQueueInfo.totalWithdrawalAmount;
        uint256 gross = balance + adapterPositions + deployedAmount;
        return gross > pendingWithdrawals ? gross - pendingWithdrawals : 0;
    }

    /// @notice Returns 0 when the deposit cap is already reached, otherwise the remaining room.
    function maxDeposit(address) public view override returns (uint256) {
        uint256 assets = totalAssets();
        return assets >= depositCap ? 0 : depositCap - assets;
    }

    /// @notice Returns 0 when the deposit cap is already reached, otherwise the share equivalent
    /// of the remaining deposit room.
    function maxMint(address) public view override returns (uint256) {
        uint256 remaining = maxDeposit(address(0));
        if (remaining == 0) return 0;
        return _convertToShares(remaining, Math.Rounding.Floor);
    }

    // Expose nonReentrant on the public ERC-4626 entry points so adapter callbacks
    // cannot re-enter any vault state-changing function.

    function deposit(uint256 assets, address receiver)
        public
        override
        nonReentrant
        returns (uint256)
    {
        return super.deposit(assets, receiver);
    }

    function mint(uint256 shares, address receiver)
        public
        override
        nonReentrant
        returns (uint256)
    {
        return super.mint(shares, receiver);
    }

    /// @notice Redeem shares for assets.
    /// @dev If the vault cannot immediately fulfil the redemption, shares are burned and
    /// the claim is queued. Listen for {WithdrawalQueued} to distinguish this case.
    /// Returns 0 assets when queued.
    function redeem(uint256 shares, address receiver, address owner)
        public
        override
        nonReentrant
        returns (uint256)
    {
        return super.redeem(shares, receiver, owner);
    }

    /// @notice Withdraw a specific asset amount by burning the corresponding shares.
    /// @dev See {redeem} for the queued-withdrawal caveat.
    function withdraw(uint256 assets, address receiver, address owner)
        public
        override
        nonReentrant
        returns (uint256)
    {
        return super.withdraw(assets, receiver, owner);
    }

    /// @dev Deposit/mint hook: pull tokens then allocate to automated adapters.
    function _deposit(address caller, address receiver, uint256 assets, uint256 shares)
        internal
        override
    {
        if (assets == 0) revert ZeroAmount();
        SafeERC20.safeTransferFrom(IERC20(asset()), caller, address(this), assets);
        _allocateToAdapters(assets);
        _mint(receiver, shares);
        emit Deposit(caller, receiver, assets, shares);
    }

    /// @dev Withdraw/redeem hook: all-or-nothing logic.
    /// Burns shares immediately in both paths. Fulfils immediately when possible, otherwise queues.
    function _withdraw(
        address caller,
        address receiver,
        address owner,
        uint256 assets,
        uint256 shares
    ) internal override {
        if (assets == 0) revert ZeroAmount();

        if (caller != owner) {
            _spendAllowance(owner, caller, shares);
        }
        _burn(owner, shares);

        // Free balance = vault balance minus the reserve already allocated to funded-but-unclaimed withdrawals.
        uint256 fundedReserve = withdrawalQueueInfo.totalWithdrawalAmount
            - withdrawalQueueInfo.nonFundedWithdrawalAmount;
        uint256 vaultBalance = IERC20(asset()).balanceOf(address(this));
        uint256 freeBalance = vaultBalance > fundedReserve ? vaultBalance - fundedReserve : 0;

        uint256 remainingNeeded = assets > freeBalance ? assets - freeBalance : 0;
        (bytes32[] memory keys, uint256[] memory amounts) = _calculateAllocation(remainingNeeded, false);

        uint256 totalFromAdapters;
        for (uint256 i = 0; i < amounts.length; i++) {
            totalFromAdapters += amounts[i];
        }

        if (freeBalance + totalFromAdapters >= assets) {
            // Immediate fulfilment
            uint256 trackedAmount;
            for (uint256 i = 0; i < keys.length; i++) {
                AdapterInfo storage a = adapters[keys[i]];
                IAdapter(a.addr).withdraw(amounts[i]);
                if (a.tracked) trackedAmount += amounts[i];
            }
            if (trackedAmount > 0) {
                deployedAmount = deployedAmount > trackedAmount
                    ? deployedAmount - trackedAmount
                    : 0;
            }
            SafeERC20.safeTransfer(IERC20(asset()), receiver, assets);
            emit Withdraw(caller, receiver, owner, assets, shares);
        } else {
            // Queued withdrawal
            uint256 id = nextWithdrawalId++;
            withdrawalRequests[id] = WithdrawalEntry({
                id: id,
                initiatedAt: block.timestamp,
                owner: owner,
                receiver: receiver,
                sharesBurned: shares,
                amountToReceive: assets,
                isFunded: false
            });
            _userWithdrawalIds[owner].push(id);
            if (_userWithdrawalIds[owner].length > maxWithdrawalsPerUser)
                revert MaxWithdrawalsReached();

            withdrawalQueueInfo.totalSharesBurned += shares;
            withdrawalQueueInfo.totalWithdrawalAmount += assets;
            withdrawalQueueInfo.nonFundedWithdrawalAmount += assets;

            emit WithdrawalQueued(id, owner, receiver, shares, assets);
        }
    }

    // FEE LOGIC

    /// @notice Permissionless. Mints fee shares to feeRecipient when the current share price
    /// exceeds highWaterMarkPrice.
    ///
    /// When fees are disabled (feeRate == 0) the high-water mark is still advanced so that
    /// re-enabling fees does not retroactively charge for gains made during the fee-free period.
    ///
    /// Formula (all arithmetic in WAD):
    ///   sharePrice  = totalAssets * WAD / totalSupply
    ///   totalYield  = (sharePrice − HWM) * totalSupply / WAD
    ///   feeAssets   = totalYield * feeRate / WAD
    ///   sharesToMint = feeAssets * WAD / sharePrice
    ///
    /// If sharesToMint rounds to 0 (dust), HWM is NOT updated so dust accumulates over
    /// multiple calls until enough yield has built up to mint at least one share.
    function accrueFees() public {
        uint256 supply = totalSupply();
        if (supply == 0) {
            if (feeRate > 0) revert NoSharesIssued();
            return;
        }

        uint256 assets = totalAssets();
        uint256 currentSharePrice = assets.mulDiv(WAD, supply, Math.Rounding.Floor);

        if (feeRate == 0) {
            // Fees disabled — advance HWM to prevent backdating when fees are re-enabled.
            if (currentSharePrice > highWaterMarkPrice) {
                highWaterMarkPrice = currentSharePrice;
            }
            return;
        }

        if (currentSharePrice <= highWaterMarkPrice) return;

        uint256 yieldPerShare = currentSharePrice - highWaterMarkPrice;
        uint256 totalYield = yieldPerShare.mulDiv(supply, WAD, Math.Rounding.Floor);
        uint256 feeAssets = totalYield.mulDiv(feeRate, WAD, Math.Rounding.Floor);
        uint256 sharesToMint = feeAssets.mulDiv(WAD, currentSharePrice, Math.Rounding.Floor);

        if (sharesToMint == 0) return; // dust; do not update HWM

        highWaterMarkPrice = currentSharePrice;
        _mint(feeRecipient, sharesToMint);
        emit FeesAccrued(feeRecipient, sharesToMint, currentSharePrice, feeAssets);
    }

    /// @notice Whitelisted only. Overwrites deployedAmount entirely with `amount`.
    /// Accrues fees first so they are calculated at the old share price before the pool
    /// value changes.
    function submitDeployedAmount(uint256 amount) external onlyWhitelisted {
        accrueFees();
        deployedAmount = amount;
        emit DeployedAmountSubmitted(msg.sender, amount);
    }

    /// @notice Whitelisted only. Updates fee rate and/or recipient.
    /// When transitioning from 0 to non-zero rate, highWaterMarkPrice is advanced to
    /// max(existing HWM, current share price) to avoid charging fees on yield that
    /// accrued while fees were disabled.
    function updateFeeConfig(uint256 newFeeRate, address newRecipient) external onlyWhitelisted {
        if (newFeeRate > WAD) revert InvalidFeeRate();

        // Validate recipient: required when enabling fees (existing or new rate > 0)
        address effectiveRecipient = (newRecipient != address(0)) ? newRecipient : feeRecipient;
        if (newFeeRate > 0 && effectiveRecipient == address(0)) revert FeeRecipientNotSet();

        // When enabling fees, advance HWM to current price (if higher) to avoid
        // backdating charges for yield that occurred while fees were disabled.
        if (feeRate == 0 && newFeeRate > 0) {
            uint256 supply = totalSupply();
            uint256 currentSharePrice = supply == 0
                ? WAD
                : totalAssets().mulDiv(WAD, supply, Math.Rounding.Floor);
            if (currentSharePrice > highWaterMarkPrice) {
                highWaterMarkPrice = currentSharePrice;
            }
        }

        feeRate = newFeeRate;
        if (newRecipient != address(0)) feeRecipient = newRecipient;

        emit FeeConfigUpdated(feeRate, feeRecipient);
    }

    // DEPLOYMENT FLOWS

    /// @notice Whitelisted only. Transfers idle vault funds to the caller for external
    /// deployment. Increases deployedAmount to track the outflow. The actual transferred
    /// amount is capped at available (balance minus pending withdrawal reserves).
    function withdrawForDeployment(uint256 amount) external onlyWhitelisted nonReentrant {
        if (amount == 0) revert ZeroAmount();
        uint256 requested = amount;
        uint256 vaultBalance = IERC20(asset()).balanceOf(address(this));
        uint256 reserved = withdrawalQueueInfo.totalWithdrawalAmount;
        uint256 available = vaultBalance > reserved ? vaultBalance - reserved : 0;
        if (available == 0) revert ZeroAmount();
        if (amount > available) amount = available;

        deployedAmount += amount;
        SafeERC20.safeTransfer(IERC20(asset()), msg.sender, amount);
        emit WithdrawForDeployment(msg.sender, requested, amount);
    }

    /// @notice Whitelisted only. Accepts funds returned from external deployment.
    /// Caller must approve this contract to pull `amount` before calling.
    /// Decreases deployedAmount accordingly.
    function depositFromDeployment(uint256 amount) external onlyWhitelisted nonReentrant {
        if (amount == 0) revert ZeroAmount();
        SafeERC20.safeTransferFrom(IERC20(asset()), msg.sender, address(this), amount);
        deployedAmount = deployedAmount > amount ? deployedAmount - amount : 0;
        emit DepositFromDeployment(msg.sender, amount);
    }

    // WITHDRAWAL QUEUE

    /// @notice Permissionless. Scans the withdrawal queue FIFO, marking entries as funded
    /// when the vault's free balance covers them. Processes at most `limit` entries.
    /// Free balance excludes funds already reserved for previously funded-but-unclaimed entries.
    function fulfillPendingWithdrawals(uint256 limit) external nonReentrant {
        if (limit == 0) return;

        uint256 start = anyWithdrawalFunded ? lastFundedWithdrawalId + 1 : 0;
        uint256 end = nextWithdrawalId;
        if (start >= end) return;

        uint256 fundedReserve = withdrawalQueueInfo.totalWithdrawalAmount
            - withdrawalQueueInfo.nonFundedWithdrawalAmount;
        uint256 vaultBalance = IERC20(asset()).balanceOf(address(this));
        uint256 freeBalance = vaultBalance > fundedReserve ? vaultBalance - fundedReserve : 0;

        uint256 processed;
        uint256 highestFunded;
        bool anyFunded;
        uint256 totalAmountFunded;

        for (uint256 id = start; id < end && processed < limit; id++) {
            WithdrawalEntry storage entry = withdrawalRequests[id];
            if (entry.initiatedAt == 0) continue;             // entry doesn't exist since it was cancelled
            if (entry.isFunded) continue;                     // already funded (should not occur past start)
            if (entry.amountToReceive > freeBalance) break;   // FIFO: stop at first unaffordable

            entry.isFunded = true;
            freeBalance -= entry.amountToReceive;
            totalAmountFunded += entry.amountToReceive;
            highestFunded = id;
            anyFunded = true;
            processed++;
            emit WithdrawalFunded(id);
        }

        if (anyFunded) {
            withdrawalQueueInfo.nonFundedWithdrawalAmount -= totalAmountFunded;
            lastFundedWithdrawalId = highestFunded;
            anyWithdrawalFunded = true;
        }
    }

    /// @notice Permissionless. Transfers assets to each funded withdrawal's receiver.
    /// Only processes IDs at or below lastFundedWithdrawalId that are marked isFunded.
    function claimUnbondedWithdrawals(uint256[] calldata ids) external nonReentrant {
        if (!anyWithdrawalFunded) revert NothingFundedYet();

        for (uint256 i = 0; i < ids.length; i++) {
            uint256 id = ids[i];
            if (id > lastFundedWithdrawalId) continue;

            WithdrawalEntry storage entry = withdrawalRequests[id];

            // Checks that the withdrawal entry has been previously funded.
            // Also covers the case when duplicate IDs are passed- !entry.isFunded will be
            // true since the request is deleted while processing the first occurrence.
            if (!entry.isFunded) continue;

            uint256 amount = entry.amountToReceive;
            address receiver = entry.receiver;
            address owner = entry.owner;
            uint256 shares = entry.sharesBurned;

            withdrawalQueueInfo.totalSharesBurned -= shares;
            withdrawalQueueInfo.totalWithdrawalAmount -= amount;

            delete withdrawalRequests[id];
            _removeFromUserWithdrawalIds(owner, id);

            SafeERC20.safeTransfer(IERC20(asset()), receiver, amount);
            emit WithdrawalClaimed(id, receiver, amount);
        }
    }

    /// @notice Cancels unfunded withdrawal requests owned by msg.sender and re-mints shares.
    /// IDs below the funded watermark (lastFundedWithdrawalId) are skipped
    /// to prevent cancellation races with fulfillPendingWithdrawals.
    ///
    /// Reverts if the cancellation would push totalAssets() above depositCap.
    function cancelWithdrawal(uint256[] calldata ids) external nonReentrant {
        uint256 lowestCancelable = anyWithdrawalFunded ? lastFundedWithdrawalId + 1 : 0;

        uint256 totalAmount;
        uint256 totalSharesBurnedToRemove;

        for (uint256 i = 0; i < ids.length; i++) {
            uint256 id = ids[i];
            if (id < lowestCancelable) continue;

            WithdrawalEntry storage entry = withdrawalRequests[id];
            if (entry.initiatedAt == 0) continue;   // entry didn't exist or it was already canceled
            if (entry.owner != msg.sender) continue; // not owned by caller
            if (entry.isFunded) continue;            // already funded, cannot cancel

            totalAmount += entry.amountToReceive;
            totalSharesBurnedToRemove += entry.sharesBurned;

            delete withdrawalRequests[id];
            _removeFromUserWithdrawalIds(msg.sender, id);
            emit WithdrawalCancelled(id, msg.sender);
        }

        if (totalAmount == 0) return;

        // Update queue totals — restores the cancelled amount to totalAssets().
        withdrawalQueueInfo.totalSharesBurned -= totalSharesBurnedToRemove;
        withdrawalQueueInfo.totalWithdrawalAmount -= totalAmount;
        withdrawalQueueInfo.nonFundedWithdrawalAmount -= totalAmount;

        // Deposit cap check: cancellation increases effective pool value.
        if (totalAssets() > depositCap) revert DepositCapReached();

        // Re-mint shares.
        uint256 sharesToMint = _convertToShares(totalAmount, Math.Rounding.Floor);
        if (sharesToMint > totalSharesBurnedToRemove) sharesToMint = totalSharesBurnedToRemove;
        _mint(msg.sender, sharesToMint);
    }

    // ADAPTER MANAGEMENT

    /// @notice Whitelisted only. Registers a new adapter.
    function registerAdapter(
        string calldata name,
        address addr,
        bool automated,
        bool tracked,
        string calldata description
    ) external onlyWhitelisted {
        if (addr == address(0)) revert ZeroAddress();
        bytes32 key = keccak256(bytes(name));
        if (adapters[key].addr != address(0)) revert AdapterAlreadyExists(name);

        adapters[key] = AdapterInfo({
            addr: addr,
            automated: automated,
            tracked: tracked,
            name: name,
            description: description
        });
        _adapterKeys.push(key);
        emit AdapterRegistered(name, addr, automated, tracked);
    }

    /// @notice Whitelisted only. Removes an adapter. Does not withdraw any funds first.
    function unregisterAdapter(string calldata name) external onlyWhitelisted {
        bytes32 key = keccak256(bytes(name));
        AdapterInfo storage a = adapters[key];
        if (a.addr == address(0)) revert AdapterNotFound(name);
        address addr = a.addr;
        delete adapters[key];
        _removeAdapterKey(key);
        emit AdapterUnregistered(name, addr);
    }

    /// @notice Whitelisted only. Switches allocation mode between Automated and Manual.
    function setAdapterAllocationMode(string calldata name, bool automated) external onlyWhitelisted {
        bytes32 key = keccak256(bytes(name));
        if (adapters[key].addr == address(0)) revert AdapterNotFound(name);
        adapters[key].automated = automated;
        emit AdapterAllocationModeUpdated(name, automated);
    }

    /// @notice Whitelisted only. Switches deployment tracking between Tracked and Untracked.
    function setAdapterDeploymentTracking(string calldata name, bool tracked) external onlyWhitelisted {
        bytes32 key = keccak256(bytes(name));
        if (adapters[key].addr == address(0)) revert AdapterNotFound(name);
        adapters[key].tracked = tracked;
        emit AdapterDeploymentTrackingUpdated(name, tracked);
    }

    /// @notice Whitelisted only. Manually withdraws `amount` from a specific adapter back
    /// to the vault. Updates deployedAmount if the adapter is tracked.
    function withdrawFromAdapter(string calldata name, uint256 amount)
        external
        onlyWhitelisted
        nonReentrant
    {
        bytes32 key = keccak256(bytes(name));
        AdapterInfo storage a = adapters[key];
        if (a.addr == address(0)) revert AdapterNotFound(name);

        IAdapter(a.addr).withdraw(amount);
        if (a.tracked) {
            deployedAmount = deployedAmount > amount ? deployedAmount - amount : 0;
        }
    }

    /// @notice Whitelisted only. Manually deposits `amount` from the vault into a specific
    /// adapter. Updates deployedAmount if the adapter is tracked.
    function depositToAdapter(string calldata name, uint256 amount)
        external
        onlyWhitelisted
        nonReentrant
    {
        bytes32 key = keccak256(bytes(name));
        AdapterInfo storage a = adapters[key];
        if (a.addr == address(0)) revert AdapterNotFound(name);

        SafeERC20.safeTransfer(IERC20(asset()), a.addr, amount);
        IAdapter(a.addr).deposit(amount);
        if (a.tracked) {
            deployedAmount += amount;
        }
    }

    /// @notice Whitelisted only. Moves `amount` from one adapter to another in a single
    /// call. Updates deployedAmount if the tracking modes of the two adapters differ.
    function moveAdapterFunds(
        string calldata fromName,
        string calldata toName,
        uint256 amount
    ) external onlyWhitelisted nonReentrant {
        bytes32 fromKey = keccak256(bytes(fromName));
        bytes32 toKey = keccak256(bytes(toName));
        AdapterInfo storage fromAdapter = adapters[fromKey];
        AdapterInfo storage toAdapter = adapters[toKey];
        if (fromAdapter.addr == address(0)) revert AdapterNotFound(fromName);
        if (toAdapter.addr == address(0)) revert AdapterNotFound(toName);

        IAdapter(fromAdapter.addr).withdraw(amount);
        SafeERC20.safeTransfer(IERC20(asset()), toAdapter.addr, amount);
        IAdapter(toAdapter.addr).deposit(amount);

        // Adjust deployedAmount only when tracking modes differ.
        if (fromAdapter.tracked && !toAdapter.tracked) {
            deployedAmount = deployedAmount > amount ? deployedAmount - amount : 0;
        } else if (!fromAdapter.tracked && toAdapter.tracked) {
            deployedAmount += amount;
        }
    }

    // ACCESS CONTROL

    /// @notice Whitelisted only. Adds `addr` to the whitelist.
    function addToWhitelist(address addr) external onlyWhitelisted {
        if (addr == address(0)) revert ZeroAddress();
        if (whitelist[addr]) revert AlreadyWhitelisted();
        whitelist[addr] = true;
        _whitelistCount++;
        emit WhitelistAdded(addr);
    }

    /// @notice Whitelisted only. Removes `addr` from the whitelist. Reverts if it is the
    /// last remaining entry.
    function removeFromWhitelist(address addr) external onlyWhitelisted {
        if (!whitelist[addr]) revert NotWhitelisted();
        if (_whitelistCount <= 1) revert WhitelistCannotBeEmpty();
        delete whitelist[addr];
        _whitelistCount--;
        emit WhitelistRemoved(addr);
    }

    // CONFIG

    function updateDepositCap(uint256 newCap) external onlyWhitelisted {
        depositCap = newCap;
        emit DepositCapUpdated(newCap);
    }

    function updateMaxWithdrawalsPerUser(uint256 newMax) external onlyWhitelisted {
        maxWithdrawalsPerUser = newMax;
        emit MaxWithdrawalsPerUserUpdated(newMax);
    }

    // VIEW FUNCTIONS

    /// @notice Amount available to send for external deployment
    /// (vault balance minus withdrawal reserves).
    function availableForDeployment() external view returns (uint256) {
        uint256 vaultBalance = IERC20(asset()).balanceOf(address(this));
        uint256 reserved = withdrawalQueueInfo.totalWithdrawalAmount;
        return vaultBalance > reserved ? vaultBalance - reserved : 0;
    }

    /// @notice Returns all queued withdrawal IDs for `user` (including funded ones pending claim).
    function getUserWithdrawalIds(address user) external view returns (uint256[] memory) {
        return _userWithdrawalIds[user];
    }

    /// @notice Returns info for all registered adapters in registration order.
    function getAdapters() external view returns (AdapterInfo[] memory result) {
        result = new AdapterInfo[](_adapterKeys.length);
        for (uint256 i = 0; i < _adapterKeys.length; i++) {
            result[i] = adapters[_adapterKeys[i]];
        }
    }

    /// @notice Returns info for a specific adapter by name.
    function getAdapterByName(string calldata name) external view returns (AdapterInfo memory) {
        bytes32 key = keccak256(bytes(name));
        AdapterInfo storage a = adapters[key];
        if (a.addr == address(0)) revert AdapterNotFound(name);
        return a;
    }

    // INTERNAL HELPERS

    /// @dev Greedily allocates `amount` across automated adapters in registration order.
    /// For deposits, queries availableForDeposit; for withdrawals, queries availableForWithdraw.
    /// Adapters not marked automated are skipped. Returns parallel arrays of adapter keys and
    /// allocated amounts. The sum of amounts may be less than `amount` if adapter capacity
    /// is insufficient — remaining funds stay in the vault.
    function _calculateAllocation(uint256 amount, bool isDeposit)
        internal
        view
        returns (bytes32[] memory keys, uint256[] memory amounts)
    {
        if (amount == 0) return (new bytes32[](0), new uint256[](0));

        uint256 n = _adapterKeys.length;
        bytes32[] memory tempKeys = new bytes32[](n);
        uint256[] memory tempAmounts = new uint256[](n);
        uint256 count;
        uint256 remaining = amount;
        address assetAddr = asset();

        for (uint256 i = 0; i < n && remaining > 0; i++) {
            bytes32 key = _adapterKeys[i];
            AdapterInfo storage a = adapters[key];
            if (!a.automated) continue;

            uint256 available = isDeposit
                ? IAdapter(a.addr).availableForDeposit(address(this), assetAddr)
                : IAdapter(a.addr).availableForWithdraw(address(this), assetAddr);

            if (available == 0) continue;

            uint256 alloc = available < remaining ? available : remaining;
            tempKeys[count] = key;
            tempAmounts[count] = alloc;
            count++;
            remaining -= alloc;
        }

        keys = new bytes32[](count);
        amounts = new uint256[](count);
        for (uint256 i = 0; i < count; i++) {
            keys[i] = tempKeys[i];
            amounts[i] = tempAmounts[i];
        }
    }

    /// @dev Sums depositor positions from all untracked adapters.
    /// Tracked adapter positions are already counted in deployedAmount and must not be queried
    /// here to avoid double-counting. Failures on individual adapters are silently ignored.
    function _queryUntrackedAdapterPositions() internal view returns (uint256 total) {
        address assetAddr = asset();
        for (uint256 i = 0; i < _adapterKeys.length; i++) {
            AdapterInfo storage a = adapters[_adapterKeys[i]];
            if (a.tracked) continue;
            try IAdapter(a.addr).depositorPosition(address(this), assetAddr) returns (uint256 pos) {
                total += pos;
            } catch {}
        }
    }

    /// @dev Allocates `amount` to automated adapters and updates deployedAmount for tracked ones.
    /// Called from _deposit after tokens have arrived in the vault.
    function _allocateToAdapters(uint256 amount) internal {
        (bytes32[] memory keys, uint256[] memory amounts) = _calculateAllocation(amount, true);
        address assetAddr = asset();
        uint256 trackedTotal;
        for (uint256 i = 0; i < keys.length; i++) {
            AdapterInfo storage a = adapters[keys[i]];
            SafeERC20.safeTransfer(IERC20(assetAddr), a.addr, amounts[i]);
            IAdapter(a.addr).deposit(amounts[i]);
            if (a.tracked) trackedTotal += amounts[i];
        }
        if (trackedTotal > 0) deployedAmount += trackedTotal;
    }

    /// @dev Removes `id` from `_userWithdrawalIds[user]` using swap-and-pop (O(n) scan).
    function _removeFromUserWithdrawalIds(address user, uint256 id) internal {
        uint256[] storage ids = _userWithdrawalIds[user];
        uint256 n = ids.length;
        for (uint256 i = 0; i < n; i++) {
            if (ids[i] == id) {
                ids[i] = ids[n - 1];
                ids.pop();
                return;
            }
        }
    }

    /// @dev Removes `key` from `_adapterKeys` using swap-and-pop.
    /// This changes the iteration order of remaining adapters.
    function _removeAdapterKey(bytes32 key) internal {
        uint256 n = _adapterKeys.length;
        for (uint256 i = 0; i < n; i++) {
            if (_adapterKeys[i] == key) {
                _adapterKeys[i] = _adapterKeys[n - 1];
                _adapterKeys.pop();
                return;
            }
        }
    }
}
