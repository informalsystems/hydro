// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {ERC4626} from "@openzeppelin/contracts/token/ERC20/extensions/ERC4626.sol";
import {ERC20} from "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {SafeERC20} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import {ReentrancyGuard} from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import {Math} from "@openzeppelin/contracts/utils/math/Math.sol";
import {InflowAdapterLib} from "./InflowAdapterLib.sol";
import {InflowWithdrawalQueueLib} from "./InflowWithdrawalQueueLib.sol";

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
    using InflowAdapterLib for InflowAdapterLib.AdapterStorage;
    using InflowWithdrawalQueueLib for InflowWithdrawalQueueLib.QueueStorage;

    // ERRORS

    error Unauthorized();
    error InvalidFeeRate();
    error FeeRecipientNotSet();
    error NoSharesIssued();
    error ZeroAmount();
    error ZeroAddress();
    error DepositCapReached();
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
    // WithdrawalFunded, WithdrawalClaimed, WithdrawalCancelled are emitted by
    // InflowWithdrawalQueueLib (DELEGATECALL context = vault address). They are re-declared
    // here so they appear in the vault's ABI and clients can decode them without the library ABI.
    event WithdrawalFunded(uint256 indexed id);
    event WithdrawalClaimed(uint256 indexed id, address indexed receiver, uint256 assets);
    /// @notice The shares re-minted to the owner after a batch cancellation are reported by
    /// the ERC-20 Transfer event from _mint.
    event WithdrawalCancelled(uint256 indexed id, address indexed owner);

    event WithdrawForDeployment(address indexed caller, uint256 requested, uint256 withdrawn);
    event DepositFromDeployment(address indexed caller, uint256 amount);

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

    // Adapter registry
    InflowAdapterLib.AdapterStorage internal _adapterStorage;

    // Withdrawal queue
    InflowWithdrawalQueueLib.QueueStorage internal _queueStorage;

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
        uint256 adapterPositions = _adapterStorage.queryUntrackedPositions(asset());
        uint256 pendingWithdrawals = _queueStorage.info.totalWithdrawalAmount;
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
        deployedAmount = _adapterStorage.allocateToAdapters(assets, asset(), deployedAmount);
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
        uint256 fundedReserve = _queueStorage.info.totalWithdrawalAmount
            - _queueStorage.info.nonFundedWithdrawalAmount;
        uint256 vaultBalance = IERC20(asset()).balanceOf(address(this));
        uint256 freeBalance = vaultBalance > fundedReserve ? vaultBalance - fundedReserve : 0;

        uint256 remainingNeeded = assets > freeBalance ? assets - freeBalance : 0;
        (bytes32[] memory keys, uint256[] memory amounts) =
            _adapterStorage.calculateAllocation(remainingNeeded, false, asset());

        uint256 totalFromAdapters;
        for (uint256 i = 0; i < amounts.length; i++) {
            totalFromAdapters += amounts[i];
        }

        if (freeBalance + totalFromAdapters >= assets) {
            // Immediate fulfilment
            deployedAmount = _adapterStorage.executeAdapterWithdrawals(keys, amounts, deployedAmount);
            SafeERC20.safeTransfer(IERC20(asset()), receiver, assets);
            emit Withdraw(caller, receiver, owner, assets, shares);
        } else {
            // Queued withdrawal
            uint256 id = _queueStorage.enqueue(owner, receiver, shares, assets, maxWithdrawalsPerUser);
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
        uint256 reserved = _queueStorage.info.totalWithdrawalAmount;
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
    function fulfillPendingWithdrawals(uint256 limit) external nonReentrant {
        _queueStorage.fulfill(limit, asset());
    }

    /// @notice Permissionless. Transfers assets to each funded withdrawal's receiver.
    /// Only processes IDs at or below lastFundedWithdrawalId that are marked isFunded.
    function claimUnbondedWithdrawals(uint256[] calldata ids) external nonReentrant {
        _queueStorage.claim(ids, asset());
    }

    /// @notice Cancels unfunded withdrawal requests owned by msg.sender and re-mints shares.
    /// IDs below the funded watermark (lastFundedWithdrawalId) are skipped
    /// to prevent cancellation races with fulfillPendingWithdrawals.
    ///
    /// Reverts if the cancellation would push totalAssets() above depositCap.
    function cancelWithdrawal(uint256[] calldata ids) external nonReentrant {
        (uint256 totalAmount, uint256 totalSharesBurnedToRemove) = _queueStorage.cancel(ids);
        if (totalAmount == 0) return;

        // Deposit cap check: cancellation restores assets to totalAssets().
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
        bool tracked
    ) external onlyWhitelisted {
        if (addr == address(0)) revert ZeroAddress();
        _adapterStorage.registerAdapter(name, addr, automated, tracked);
        emit AdapterRegistered(name, addr, automated, tracked);
    }

    /// @notice Whitelisted only. Removes an adapter. Does not withdraw any funds first.
    function unregisterAdapter(string calldata name) external onlyWhitelisted {
        address addr = _adapterStorage.unregisterAdapter(name);
        emit AdapterUnregistered(name, addr);
    }

    /// @notice Whitelisted only. Switches allocation mode between Automated and Manual.
    function setAdapterAllocationMode(string calldata name, bool automated) external onlyWhitelisted {
        _adapterStorage.setAdapterAllocationMode(name, automated);
        emit AdapterAllocationModeUpdated(name, automated);
    }

    /// @notice Whitelisted only. Switches deployment tracking between Tracked and Untracked.
    function setAdapterDeploymentTracking(string calldata name, bool tracked) external onlyWhitelisted {
        _adapterStorage.setAdapterDeploymentTracking(name, tracked);
        emit AdapterDeploymentTrackingUpdated(name, tracked);
    }

    /// @notice Whitelisted only. Manually withdraws `amount` from a specific adapter back
    /// to the vault. Updates deployedAmount if the adapter is tracked.
    function withdrawFromAdapter(string calldata name, uint256 amount)
        external
        onlyWhitelisted
        nonReentrant
    {
        deployedAmount = _adapterStorage.withdrawFromAdapter(name, amount, deployedAmount);
    }

    /// @notice Whitelisted only. Manually deposits `amount` from the vault into a specific
    /// adapter. Updates deployedAmount if the adapter is tracked.
    function depositToAdapter(string calldata name, uint256 amount)
        external
        onlyWhitelisted
        nonReentrant
    {
        deployedAmount = _adapterStorage.depositToAdapter(name, amount, asset(), deployedAmount);
    }

    /// @notice Whitelisted only. Moves `amount` from one adapter to another in a single
    /// call. Updates deployedAmount if the tracking modes of the two adapters differ.
    function moveAdapterFunds(
        string calldata fromName,
        string calldata toName,
        uint256 amount
    ) external onlyWhitelisted nonReentrant {
        deployedAmount = _adapterStorage.moveAdapterFunds(fromName, toName, amount, asset(), deployedAmount);
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
        uint256 reserved = _queueStorage.info.totalWithdrawalAmount;
        return vaultBalance > reserved ? vaultBalance - reserved : 0;
    }

    /// @notice Returns all queued withdrawal IDs for `user` (including funded ones pending claim).
    function getUserWithdrawalIds(address user) external view returns (uint256[] memory) {
        return _queueStorage.userIds[user];
    }

    /// @notice Returns info for all registered adapters in registration order.
    function getAdapters() external view returns (InflowAdapterLib.AdapterInfo[] memory) {
        return _adapterStorage.getAdapters();
    }

    /// @notice Returns info for a specific adapter by name.
    function getAdapterByName(string calldata name)
        external view returns (InflowAdapterLib.AdapterInfo memory)
    {
        return _adapterStorage.getAdapterByName(name);
    }

    // WITHDRAWAL QUEUE STATE GETTERS

    function withdrawalRequest(uint256 id)
        external view returns (InflowWithdrawalQueueLib.WithdrawalEntry memory)
    {
        return _queueStorage.requests[id];
    }

    function withdrawalQueueInfo()
        external view returns (InflowWithdrawalQueueLib.WithdrawalQueueInfo memory)
    {
        return _queueStorage.info;
    }

    function nextWithdrawalId() external view returns (uint256) {
        return _queueStorage.nextId;
    }

    function lastFundedWithdrawalId() external view returns (uint256) {
        return _queueStorage.lastFundedId;
    }

    function anyWithdrawalFunded() external view returns (bool) {
        return _queueStorage.anyFunded;
    }
}
