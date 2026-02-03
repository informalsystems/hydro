package evm

import (
	"context"
	"encoding/hex"
	"fmt"
	"math/big"
	"strings"
	"time"

	"github.com/ethereum/go-ethereum"
	"github.com/ethereum/go-ethereum/accounts/abi"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
	"go.uber.org/zap"

	"hydro/offchain/internal/config"
)

// ForwarderABI is the ABI for CCTPUSDCForwarder contract
const ForwarderABI = `[
	{
		"inputs": [
			{"internalType": "address", "name": "_cctpContract", "type": "address"},
			{"internalType": "uint32", "name": "_destinationDomain", "type": "uint32"},
			{"internalType": "address", "name": "_tokenToBridge", "type": "address"},
			{"internalType": "bytes32", "name": "_recipient", "type": "bytes32"},
			{"internalType": "bytes32", "name": "_destinationCaller", "type": "bytes32"},
			{"internalType": "address", "name": "_operator", "type": "address"},
			{"internalType": "address", "name": "_admin", "type": "address"},
			{"internalType": "uint256", "name": "_operationalFeeBps", "type": "uint256"},
			{"internalType": "uint256", "name": "_minOperationalFee", "type": "uint256"}
		],
		"stateMutability": "nonpayable",
		"type": "constructor"
	},
	{
		"inputs": [
			{"internalType": "uint256", "name": "transferAmount", "type": "uint256"},
			{"internalType": "uint256", "name": "smartRelayFeeAmount", "type": "uint256"},
			{"internalType": "address", "name": "operationalFeeRecipient", "type": "address"}
		],
		"name": "bridge",
		"outputs": [],
		"stateMutability": "nonpayable",
		"type": "function"
	},
	{
		"inputs": [],
		"name": "pause",
		"outputs": [],
		"stateMutability": "nonpayable",
		"type": "function"
	},
	{
		"inputs": [
			{"internalType": "address", "name": "receiver", "type": "address"},
			{"internalType": "address", "name": "token", "type": "address"},
			{"internalType": "uint256", "name": "amount", "type": "uint256"}
		],
		"name": "safeguardTokens",
		"outputs": [],
		"stateMutability": "nonpayable",
		"type": "function"
	},
	{
		"inputs": [],
		"name": "paused",
		"outputs": [{"internalType": "bool", "name": "", "type": "bool"}],
		"stateMutability": "view",
		"type": "function"
	},
	{
		"inputs": [],
		"name": "operator",
		"outputs": [{"internalType": "address", "name": "", "type": "address"}],
		"stateMutability": "view",
		"type": "function"
	},
	{
		"inputs": [],
		"name": "admin",
		"outputs": [{"internalType": "address", "name": "", "type": "address"}],
		"stateMutability": "view",
		"type": "function"
	},
	{
		"inputs": [],
		"name": "tokenToBridge",
		"outputs": [{"internalType": "address", "name": "", "type": "address"}],
		"stateMutability": "view",
		"type": "function"
	},
	{
		"inputs": [],
		"name": "operationalFeeBps",
		"outputs": [{"internalType": "uint256", "name": "", "type": "uint256"}],
		"stateMutability": "view",
		"type": "function"
	},
	{
		"inputs": [],
		"name": "minOperationalFee",
		"outputs": [{"internalType": "uint256", "name": "", "type": "uint256"}],
		"stateMutability": "view",
		"type": "function"
	},
	{
		"anonymous": false,
		"inputs": [
			{"indexed": false, "internalType": "address", "name": "caller", "type": "address"},
			{"indexed": false, "internalType": "address", "name": "token", "type": "address"},
			{"indexed": false, "internalType": "uint256", "name": "transferAmount", "type": "uint256"},
			{"indexed": false, "internalType": "uint256", "name": "smartRelayFeeAmount", "type": "uint256"},
			{"indexed": false, "internalType": "uint256", "name": "operationalFeeAmount", "type": "uint256"},
			{"indexed": false, "internalType": "address", "name": "operationalFeeRecipient", "type": "address"},
			{"indexed": false, "internalType": "uint32", "name": "destinationDomain", "type": "uint32"},
			{"indexed": false, "internalType": "bytes32", "name": "recipient", "type": "bytes32"},
			{"indexed": false, "internalType": "bytes32", "name": "destinationCaller", "type": "bytes32"}
		],
		"name": "BridgingRequested",
		"type": "event"
	}
]`

// Forwarder provides methods to interact with the CCTPUSDCForwarder contract
type Forwarder struct {
	client      *Client
	chainConfig *config.ChainConfig
	abi         abi.ABI
	logger      *zap.Logger
}

// NewForwarder creates a new Forwarder instance
func NewForwarder(client *Client, chainConfig *config.ChainConfig, logger *zap.Logger) (*Forwarder, error) {
	parsedABI, err := abi.JSON(strings.NewReader(ForwarderABI))
	if err != nil {
		return nil, fmt.Errorf("failed to parse forwarder ABI: %w", err)
	}

	return &Forwarder{
		client:      client,
		chainConfig: chainConfig,
		abi:         parsedABI,
		logger:      logger,
	}, nil
}

// BridgeParams holds parameters for the bridge function call
type BridgeParams struct {
	TransferAmount            *big.Int       // Amount to transfer (before operational fee deduction)
	SmartRelayFeeAmount       *big.Int       // Fee for CCTP relayer
	OperationalFeeRecipient   common.Address // Where operational fees are sent
}

// Bridge calls the bridge() function on the forwarder contract
func (f *Forwarder) Bridge(ctx context.Context, forwarderAddress common.Address, params BridgeParams) (common.Hash, error) {
	f.logger.Info("Calling bridge on forwarder",
		zap.String("forwarder", forwarderAddress.Hex()),
		zap.String("transfer_amount", params.TransferAmount.String()),
		zap.String("relay_fee", params.SmartRelayFeeAmount.String()),
		zap.String("fee_recipient", params.OperationalFeeRecipient.Hex()))

	// Encode function call
	data, err := f.abi.Pack("bridge",
		params.TransferAmount,
		params.SmartRelayFeeAmount,
		params.OperationalFeeRecipient,
	)
	if err != nil {
		return common.Hash{}, fmt.Errorf("failed to pack bridge call: %w", err)
	}

	// Send transaction
	txHash, err := f.client.SignAndSendTransaction(ctx, forwarderAddress, data, big.NewInt(0))
	if err != nil {
		return common.Hash{}, fmt.Errorf("failed to send bridge transaction: %w", err)
	}

	f.logger.Info("Bridge transaction sent",
		zap.String("tx_hash", txHash.Hex()),
		zap.String("forwarder", forwarderAddress.Hex()))

	return txHash, nil
}

// BridgeAndWait calls bridge() and waits for the transaction to be mined
func (f *Forwarder) BridgeAndWait(ctx context.Context, forwarderAddress common.Address, params BridgeParams, timeout time.Duration) (*types.Receipt, error) {
	txHash, err := f.Bridge(ctx, forwarderAddress, params)
	if err != nil {
		return nil, err
	}

	receipt, err := f.client.WaitForTransaction(ctx, txHash, timeout)
	if err != nil {
		return nil, fmt.Errorf("bridge transaction failed: %w", err)
	}

	f.logger.Info("Bridge transaction confirmed",
		zap.String("tx_hash", txHash.Hex()),
		zap.Uint64("gas_used", receipt.GasUsed),
		zap.Uint64("block_number", receipt.BlockNumber.Uint64()))

	return receipt, nil
}

// IsPaused checks if the forwarder contract is paused
func (f *Forwarder) IsPaused(ctx context.Context, forwarderAddress common.Address) (bool, error) {
	data, err := f.abi.Pack("paused")
	if err != nil {
		return false, fmt.Errorf("failed to pack paused call: %w", err)
	}

	result, err := f.client.ethClient.CallContract(ctx, ethereum.CallMsg{
		To:   &forwarderAddress,
		Data: data,
	}, nil)
	if err != nil {
		return false, fmt.Errorf("failed to call paused: %w", err)
	}

	var paused bool
	err = f.abi.UnpackIntoInterface(&paused, "paused", result)
	if err != nil {
		return false, fmt.Errorf("failed to unpack paused result: %w", err)
	}

	return paused, nil
}

// GetOperator returns the operator address of the forwarder
func (f *Forwarder) GetOperator(ctx context.Context, forwarderAddress common.Address) (common.Address, error) {
	data, err := f.abi.Pack("operator")
	if err != nil {
		return common.Address{}, fmt.Errorf("failed to pack operator call: %w", err)
	}

	result, err := f.client.ethClient.CallContract(ctx, ethereum.CallMsg{
		To:   &forwarderAddress,
		Data: data,
	}, nil)
	if err != nil {
		return common.Address{}, fmt.Errorf("failed to call operator: %w", err)
	}

	var operator common.Address
	err = f.abi.UnpackIntoInterface(&operator, "operator", result)
	if err != nil {
		return common.Address{}, fmt.Errorf("failed to unpack operator result: %w", err)
	}

	return operator, nil
}

// ForwarderConstructorParams holds the constructor parameters for deploying a forwarder
type ForwarderConstructorParams struct {
	CCTPContract        common.Address // Skip's CCTP contract address
	DestinationDomain   uint32         // CCTP destination domain (Noble = 4)
	TokenToBridge       common.Address // USDC address on this chain
	Recipient           [32]byte       // Noble forwarding account (bytes32)
	DestinationCaller   [32]byte       // Skip relayer address (bytes32)
	Operator            common.Address // Operator address
	Admin               common.Address // Admin address
	OperationalFeeBps   *big.Int       // Fee in basis points
	MinOperationalFee   *big.Int       // Minimum fee
}

// CreateConstructorParamsForUser creates complete constructor params including the recipient
// (Noble forwarding account) for a specific user.
//
// Parameters:
//   - chainCfg: EVM chain configuration
//   - neutronCfg: Neutron configuration (for Noble channel)
//   - operatorCfg: Operator configuration
//   - userEmail: User's email (for computing proxy and forwarding addresses)
//   - codeChecksum: SHA256 checksum of the proxy contract wasm bytecode (32 bytes)
//   - computeNobleForwardingAddr: Function to compute Noble forwarding address
//   - convertToBytes32: Function to convert bech32 address to bytes32
//
// Returns ForwarderConstructorParams with all fields populated including Recipient.
func CreateConstructorParamsForUser(
	chainCfg *config.ChainConfig,
	neutronCfg *config.NeutronConfig,
	operatorCfg *config.OperatorConfig,
	userEmail string,
	codeChecksum []byte,
	computeNobleForwardingAddr func(codeChecksum []byte, operatorAddr, email, channel string) (string, error),
	convertToBytes32 func(nobleAddr string) ([32]byte, error),
) (ForwarderConstructorParams, error) {
	// Parse destination caller (bytes32 hex string)
	destCallerHex := strings.TrimPrefix(chainCfg.DestinationCaller, "0x")
	if len(destCallerHex) != 64 {
		return ForwarderConstructorParams{}, fmt.Errorf("invalid destination caller length: %d (expected 64)", len(destCallerHex))
	}
	destCallerBytes, err := hex.DecodeString(destCallerHex)
	if err != nil {
		return ForwarderConstructorParams{}, fmt.Errorf("failed to decode destination caller: %w", err)
	}
	var destCaller [32]byte
	copy(destCaller[:], destCallerBytes)

	// Compute Noble forwarding address for this user
	nobleForwardingAddr, err := computeNobleForwardingAddr(
		codeChecksum,
		operatorCfg.NeutronAddress,
		userEmail,
		neutronCfg.NobleChannel,
	)
	if err != nil {
		return ForwarderConstructorParams{}, fmt.Errorf("failed to compute Noble forwarding address: %w", err)
	}

	// Convert to bytes32 for EVM
	recipient, err := convertToBytes32(nobleForwardingAddr)
	if err != nil {
		return ForwarderConstructorParams{}, fmt.Errorf("failed to convert Noble address to bytes32: %w", err)
	}

	return ForwarderConstructorParams{
		CCTPContract:        common.HexToAddress(chainCfg.CCTPContractAddress),
		DestinationDomain:   chainCfg.DestinationDomain,
		TokenToBridge:       common.HexToAddress(chainCfg.USDCContractAddress),
		Recipient:           recipient,
		DestinationCaller:   destCaller,
		Operator:            common.HexToAddress(chainCfg.OperatorAddress),
		Admin:               common.HexToAddress(operatorCfg.AdminAddress),
		OperationalFeeBps:   big.NewInt(int64(chainCfg.OperationalFeeBps)),
		MinOperationalFee:   big.NewInt(chainCfg.MinOperationalFee),
	}, nil
}

// DeployForwarderCREATE2 deploys a forwarder contract using CREATE2 via Arachnid's factory
// This ensures the deployed address matches the precomputed address from ComputeForwarderAddress
func (f *Forwarder) DeployForwarderCREATE2(ctx context.Context, userEmail string, chainID string, constructorParams ForwarderConstructorParams) (common.Address, common.Hash, error) {
	f.logger.Info("Deploying forwarder via CREATE2",
		zap.String("user_email", userEmail),
		zap.String("chain_id", chainID),
		zap.String("cctp_contract", constructorParams.CCTPContract.Hex()),
		zap.Uint32("dest_domain", constructorParams.DestinationDomain),
		zap.String("token", constructorParams.TokenToBridge.Hex()),
		zap.String("recipient", hex.EncodeToString(constructorParams.Recipient[:])),
		zap.String("dest_caller", hex.EncodeToString(constructorParams.DestinationCaller[:])),
		zap.String("operator", constructorParams.Operator.Hex()),
		zap.String("admin", constructorParams.Admin.Hex()),
		zap.String("fee_bps", constructorParams.OperationalFeeBps.String()),
		zap.String("min_fee", constructorParams.MinOperationalFee.String()))

	// Get bytecode from config
	bytecodeHex := strings.TrimPrefix(f.chainConfig.ForwarderBytecode, "0x")
	bytecode, err := hex.DecodeString(bytecodeHex)
	if err != nil {
		return common.Address{}, common.Hash{}, fmt.Errorf("failed to decode bytecode: %w", err)
	}

	// Encode constructor arguments
	constructorArgs, err := f.abi.Constructor.Inputs.Pack(
		constructorParams.CCTPContract,
		constructorParams.DestinationDomain,
		constructorParams.TokenToBridge,
		constructorParams.Recipient,
		constructorParams.DestinationCaller,
		constructorParams.Operator,
		constructorParams.Admin,
		constructorParams.OperationalFeeBps,
		constructorParams.MinOperationalFee,
	)
	if err != nil {
		return common.Address{}, common.Hash{}, fmt.Errorf("failed to encode constructor args: %w", err)
	}

	// Complete init code = bytecode + constructor args
	initCode := append(bytecode, constructorArgs...)

	f.logger.Debug("CREATE2 deployment data",
		zap.Int("bytecode_len", len(bytecode)),
		zap.Int("constructor_args_len", len(constructorArgs)),
		zap.Int("init_code_len", len(initCode)))

	// Generate salt
	salt := GenerateSalt(userEmail, chainID)

	// Compute expected address before deployment
	expectedAddress, err := ComputeForwarderAddress(userEmail, chainID, initCode)
	if err != nil {
		return common.Address{}, common.Hash{}, fmt.Errorf("failed to compute expected address: %w", err)
	}

	// Arachnid's factory expects: salt (32 bytes) + init code
	deployData := append(salt[:], initCode...)

	// Send transaction to factory
	txHash, err := f.client.SignAndSendTransaction(ctx, ArachnidFactoryAddress, deployData, big.NewInt(0))
	if err != nil {
		return common.Address{}, common.Hash{}, fmt.Errorf("failed to send CREATE2 deployment tx: %w", err)
	}

	f.logger.Info("Forwarder CREATE2 deployment tx sent",
		zap.String("tx_hash", txHash.Hex()),
		zap.String("expected_address", expectedAddress.Hex()))

	return expectedAddress, txHash, nil
}

// DeployForwarderCREATE2AndWait deploys and waits for confirmation
func (f *Forwarder) DeployForwarderCREATE2AndWait(ctx context.Context, userEmail string, chainID string, constructorParams ForwarderConstructorParams, timeout time.Duration) (common.Address, common.Hash, error) {
	expectedAddress, txHash, err := f.DeployForwarderCREATE2(ctx, userEmail, chainID, constructorParams)
	if err != nil {
		return common.Address{}, common.Hash{}, err
	}

	receipt, err := f.client.WaitForTransaction(ctx, txHash, timeout)
	if err != nil {
		return expectedAddress, txHash, fmt.Errorf("CREATE2 deployment failed: %w", err)
	}

	if receipt.Status != 1 {
		return expectedAddress, txHash, fmt.Errorf("CREATE2 deployment reverted")
	}

	// Verify contract is deployed at expected address
	deployed, err := f.IsForwarderDeployed(ctx, expectedAddress)
	if err != nil {
		return expectedAddress, txHash, fmt.Errorf("failed to verify deployment: %w", err)
	}
	if !deployed {
		return expectedAddress, txHash, fmt.Errorf("contract not found at expected address %s", expectedAddress.Hex())
	}

	f.logger.Info("Forwarder CREATE2 deployment confirmed",
		zap.String("address", expectedAddress.Hex()),
		zap.String("tx_hash", txHash.Hex()))

	return expectedAddress, txHash, nil
}

// GetForwarderBalance gets the USDC balance of a forwarder contract
func (f *Forwarder) GetForwarderBalance(ctx context.Context, forwarderAddress common.Address) (*big.Int, error) {
	return f.client.GetUSDCBalance(ctx, forwarderAddress)
}

// IsForwarderDeployed checks if a forwarder contract is deployed at the given address
func (f *Forwarder) IsForwarderDeployed(ctx context.Context, forwarderAddress common.Address) (bool, error) {
	return f.client.IsContractDeployed(ctx, forwarderAddress)
}
