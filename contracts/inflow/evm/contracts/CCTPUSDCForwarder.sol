// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

import "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import "@openzeppelin/contracts/utils/Strings.sol";

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
    uint256 public constant MAX_BPS = 10_000; // 100% = 10000 basis points

    // Address of Skip's contract used to initiate CCTP transfer
    address public immutable cctpContract;
    // Destination domain ID accorindg to CCTP protocol: https://developers.circle.com/cctp/v1/supported-domains
    uint32 public immutable destinationDomain;
    // Address of the USDC ERC-20 token to bridge
    address public immutable tokenToBridge;
    // Recipient Forwarding Account address on the Noble blockchain encoded as bytes32
    bytes32 public immutable recipient;
    // Skip's relayer address on the Noble blockchain authorized to execute the minting
    bytes32 public immutable destinationCaller;
    // Address of the operator that can only initiate bridging transactions
    address public immutable operator;
    // Address of the admin that can pause the contract and safeguard tokens in case of emergency
    address public immutable admin;
    // Operational fee in basis points (bps) charged on each bridging transaction in order to cover operational costs
    uint256 public immutable operationalFeeBps;
    // Minimum operational fee amount charged on each bridging transaction to cover operational costs
    uint256 public immutable minOperationalFee;
    // Indicates whether the contract is paused or not
    bool public paused;

    using Strings for uint256;

    event Paused(address caller);

    event SafeguardTokens(address caller, address receiver, address token, uint256 transferAmount);

    event BridgingRequested(
        address caller,
        address token,
        uint256 transferAmount,
        uint256 smartRelayFeeAmount,
        uint256 operationalFeeAmount,
        address operationalFeeRecipient,
        uint32 destinationDomain,
        bytes32 recipient,
        bytes32 destinationCaller
    );

    constructor(
        address _cctpContract,
        uint32 _destinationDomain,
        address _tokenToBridge,
        bytes32 _recipient,
        bytes32 _destinationCaller,
        address _operator,
        address _admin,
        uint256 _operationalFeeBps,
        uint256 _minOperationalFee
    ) {
        require(_cctpContract != address(0), "cctpContract address is required");
        require(_tokenToBridge != address(0), "tokenToBridge address is required");
        require(_operator != address(0), "operator address is required");
        require(_admin != address(0), "admin address is required");
        require(_recipient != bytes32(0), "recipient is required");
        require(_destinationCaller != bytes32(0), "destinationCaller is required");
        require(_operationalFeeBps <= MAX_BPS, "operationalFeeBps exceeds maximum allowed value");

        cctpContract = _cctpContract;
        operator = _operator;
        admin = _admin;
        destinationDomain = _destinationDomain;
        tokenToBridge = _tokenToBridge;
        recipient = _recipient;
        destinationCaller = _destinationCaller;
        operationalFeeBps = _operationalFeeBps;
        minOperationalFee = _minOperationalFee;
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

    // In case of emergency, admin can pause the contract
    function pause() external onlyAdmin {
        paused = true;
        emit Paused(msg.sender);
    }

    // In case of emergency, admin can safeguard the tokens by sending them to a specified receiver address
    function safeguardTokens(address receiver, address token, uint256 amount) external onlyAdmin {
        require(receiver != address(0), "receiver is required");
        require(token != address(0), "token is required");
        require(amount > 0, "amount must be greater than zero");

        IERC20 erc20Token = IERC20(token);
        uint256 availableBalance = erc20Token.balanceOf(address(this));
        require(availableBalance >= amount, "insufficient balance");

        bool success = erc20Token.transfer(receiver, amount);
        if (!success) {
            revert(string(abi.encodePacked("failed to transfer tokens to receiver: ", Strings.toHexString(receiver))));
        }
        emit SafeguardTokens(msg.sender, receiver, token, amount);
    }

    // Initiates bridging of USDC tokens to the destination chain via CCTP protocol.
    // In addition to fees required by CCTP protocol (smartRelayFeeAmount), an operational fee is deducted
    // from the transfer amount and sent to the operationalFeeRecipient, if contract is configured in such way.
    function bridge(uint256 transferAmount, uint256 smartRelayFeeAmount, address operationalFeeRecipient)
        external
        onlyOperator
    {
        require(!paused, "contract is paused");
        require(transferAmount > 0, "transferAmount must be greater than zero");
        require(smartRelayFeeAmount > 0, "smartRelayFeeAmount must be greater than zero");

        IERC20 erc20TokenToBridge = IERC20(tokenToBridge);
        uint256 contractBalance = erc20TokenToBridge.balanceOf(address(this));
        require(
            contractBalance >= transferAmount + smartRelayFeeAmount,
            string(
                abi.encodePacked(
                    "insufficient balance; requested: ",
                    (transferAmount + smartRelayFeeAmount).toString(),
                    ", available: ",
                    contractBalance.toString()
                )
            )
        );

        uint256 operationalFee = (transferAmount * operationalFeeBps) / MAX_BPS;
        if (operationalFee < minOperationalFee) {
            operationalFee = minOperationalFee;
        }
        require(
            transferAmount > operationalFee,
            string(
                abi.encodePacked(
                    "insufficient transfer amount to cover operational fee; transferAmount: ",
                    transferAmount.toString(),
                    ", operationalFee: ",
                    operationalFee.toString()
                )
            )
        );
        transferAmount -= operationalFee;

        if (operationalFee > 0) {
            require(operationalFeeRecipient != address(0), "operationalFeeRecipient address is required");

            bool feeTransferSuccess = erc20TokenToBridge.transfer(operationalFeeRecipient, operationalFee);
            if (!feeTransferSuccess) {
                revert(
                    string(
                        abi.encodePacked(
                            "failed to transfer operational fee amount: ",
                            operationalFee.toString(),
                            " to recipient: ",
                            Strings.toHexString(operationalFeeRecipient)
                        )
                    )
                );
            }
        }

        uint256 approvalAmount = transferAmount + smartRelayFeeAmount;
        bool success = erc20TokenToBridge.approve(cctpContract, approvalAmount);
        if (!success) {
            revert(
                string(
                    abi.encodePacked("failed to approve cctpContract as spender of amount: ", approvalAmount.toString())
                )
            );
        }

        ICCTPBridge(cctpContract)
            .requestCCTPTransferWithCaller(
                transferAmount, destinationDomain, recipient, tokenToBridge, smartRelayFeeAmount, destinationCaller
            );
        emit BridgingRequested(
            msg.sender,
            tokenToBridge,
            transferAmount,
            smartRelayFeeAmount,
            operationalFee,
            operationalFeeRecipient,
            destinationDomain,
            recipient,
            destinationCaller
        );
    }
}
