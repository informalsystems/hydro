// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import "@openzeppelin/contracts/token/ERC20/IERC20.sol";

interface ICCTPBridge {
	function requestCCTPTransferWithCaller(
		uint256 transferAmount,
		uint32 destinationDomain,
		bytes32 mintRecipient,
		address burnToken,
		uint256 feeAmount,
		bytes32 destinationCaller
	) external;
}

contract CCTPUSDCForwarder {
	address public immutable cctpContract;
	uint32 public immutable destinationDomain;
	address public immutable tokenToBridge;
	bytes32 public immutable recipient;
	bytes32 public immutable destinationCaller;
	address public immutable operator;
	address public immutable admin;
	bool public paused;

	constructor(
		address _cctpContract,
		uint32 _destinationDomain,
		address _tokenToBridge,
		bytes32 _recipient,
		bytes32 _destinationCaller,
		address _operator,
		address _admin
	) {
		require(_cctpContract != address(0), "cctpContract address is required");
		require(_tokenToBridge != address(0), "tokenToBridge address is required");
		require(_operator != address(0), "operator address is required");
		require(_admin != address(0), "admin address is required");

		cctpContract = _cctpContract;
		operator = _operator;
		admin = _admin;
		destinationDomain = _destinationDomain;
		tokenToBridge = _tokenToBridge;
		recipient = _recipient;
		destinationCaller = _destinationCaller;
		paused = false;
	}

	modifier onlyOperator() {
        require(msg.sender == operator, "only operator can call this function");
        _;
    }

	modifier onlyAdmin() {
		require(msg.sender == admin, "only admin can call this function");
		_;
	}

	modifier onlyOperatorOrAdmin() {
		require(msg.sender == operator || msg.sender == admin, "only operator or admin can call this function");
		_;
	}

	// In case of emergency, admin can pause the contract
	function pause() external onlyAdmin {
		paused = true;
	}

	// Forwards approve to tokenToBridge so that the specified spender can spend a given number of tokens.
	// Used by the operator to authorize the cctpContract to spend tokens on behalf of the contract,
	// or to give allowance to another address in order to charge the bridging fees.
	// It can also be used by admin to manage approvals in case of emergency (i.e. when the contract is paused).
	function approve(address spender, uint256 amount) external onlyOperatorOrAdmin returns (bool) {
		// Operator is blocked when contract is paused; admin can execute the function at any time
		if (msg.sender == operator) {
			require(!paused, "contract is paused");
		}

		return IERC20(tokenToBridge).approve(spender, amount);
	}

	// Calls cctpContract.requestCCTPTransferWithCaller to bridge tokens to the destination chain.
	function bridge(uint256 transferAmount, uint256 feeAmount) external onlyOperator {
		require(!paused, "contract is paused");
		ICCTPBridge(cctpContract).requestCCTPTransferWithCaller(
			transferAmount,
			destinationDomain,
			recipient,
			tokenToBridge,
			feeAmount,
			destinationCaller
		);
	}
}
