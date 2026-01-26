package evm

import (
	"context"
	"crypto/ecdsa"
	"fmt"
	"math/big"
	"strings"
	"time"

	"github.com/ethereum/go-ethereum"
	"github.com/ethereum/go-ethereum/accounts/abi/bind"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/crypto"
	"github.com/ethereum/go-ethereum/ethclient"
	"go.uber.org/zap"

	"hydro/offchain/internal/config"
)

// Client wraps Ethereum client functionality for interacting with EVM chains
type Client struct {
	ethClient   *ethclient.Client
	chainConfig *config.ChainConfig
	privateKey  *ecdsa.PrivateKey
	fromAddress common.Address
	logger      *zap.Logger
}

// NewClient creates a new EVM client for the specified chain
func NewClient(chainCfg *config.ChainConfig, operatorPrivateKey string, logger *zap.Logger) (*Client, error) {
	// Connect to RPC endpoint
	ethClient, err := ethclient.Dial(chainCfg.RPCEndpoint)
	if err != nil {
		return nil, fmt.Errorf("failed to connect to RPC endpoint %s: %w", chainCfg.RPCEndpoint, err)
	}

	// Parse private key (remove 0x prefix if present)
	privateKeyHex := strings.TrimPrefix(operatorPrivateKey, "0x")
	privateKey, err := crypto.HexToECDSA(privateKeyHex)
	if err != nil {
		return nil, fmt.Errorf("failed to parse private key: %w", err)
	}

	// Get public key and address
	publicKey := privateKey.Public()
	publicKeyECDSA, ok := publicKey.(*ecdsa.PublicKey)
	if !ok {
		return nil, fmt.Errorf("failed to cast public key to ECDSA")
	}
	fromAddress := crypto.PubkeyToAddress(*publicKeyECDSA)

	logger.Info("EVM client initialized",
		zap.String("chain_id", chainCfg.ChainID),
		zap.String("chain_name", chainCfg.Name),
		zap.String("operator_address", fromAddress.Hex()))

	return &Client{
		ethClient:   ethClient,
		chainConfig: chainCfg,
		privateKey:  privateKey,
		fromAddress: fromAddress,
		logger:      logger,
	}, nil
}

// Close closes the underlying RPC connection
func (c *Client) Close() {
	c.ethClient.Close()
}

// ChainID returns the chain ID
func (c *Client) ChainID() string {
	return c.chainConfig.ChainID
}

// OperatorAddress returns the operator's address
func (c *Client) OperatorAddress() common.Address {
	return c.fromAddress
}

// GetUSDCBalance returns the USDC balance of an address
func (c *Client) GetUSDCBalance(ctx context.Context, address common.Address) (*big.Int, error) {
	usdcAddress := common.HexToAddress(c.chainConfig.USDCContractAddress)

	// ERC20 balanceOf(address) selector: 0x70a08231
	data := append(
		common.Hex2Bytes("70a08231"),
		common.LeftPadBytes(address.Bytes(), 32)...,
	)

	result, err := c.ethClient.CallContract(ctx, ethereum.CallMsg{
		To:   &usdcAddress,
		Data: data,
	}, nil)
	if err != nil {
		return nil, fmt.Errorf("failed to call balanceOf: %w", err)
	}

	if len(result) < 32 {
		return nil, fmt.Errorf("invalid balance response length: %d", len(result))
	}

	balance := new(big.Int).SetBytes(result)
	return balance, nil
}

// GetETHBalance returns the ETH balance of an address
func (c *Client) GetETHBalance(ctx context.Context, address common.Address) (*big.Int, error) {
	return c.ethClient.BalanceAt(ctx, address, nil)
}

// GetNonce returns the current nonce for the operator address
func (c *Client) GetNonce(ctx context.Context) (uint64, error) {
	return c.ethClient.PendingNonceAt(ctx, c.fromAddress)
}

// GetGasPrice returns the suggested gas price
func (c *Client) GetGasPrice(ctx context.Context) (*big.Int, error) {
	return c.ethClient.SuggestGasPrice(ctx)
}

// GetChainID returns the chain ID from the network
func (c *Client) GetChainIDFromNetwork(ctx context.Context) (*big.Int, error) {
	return c.ethClient.ChainID(ctx)
}

// EstimateGas estimates gas for a transaction
func (c *Client) EstimateGas(ctx context.Context, msg ethereum.CallMsg) (uint64, error) {
	return c.ethClient.EstimateGas(ctx, msg)
}

// SendTransaction sends a signed transaction
func (c *Client) SendTransaction(ctx context.Context, tx *types.Transaction) error {
	return c.ethClient.SendTransaction(ctx, tx)
}

// WaitForTransaction waits for a transaction to be mined
func (c *Client) WaitForTransaction(ctx context.Context, txHash common.Hash, timeout time.Duration) (*types.Receipt, error) {
	ctx, cancel := context.WithTimeout(ctx, timeout)
	defer cancel()

	ticker := time.NewTicker(2 * time.Second)
	defer ticker.Stop()

	for {
		select {
		case <-ctx.Done():
			return nil, fmt.Errorf("timeout waiting for transaction %s", txHash.Hex())
		case <-ticker.C:
			receipt, err := c.ethClient.TransactionReceipt(ctx, txHash)
			if err == nil && receipt != nil {
				if receipt.Status == 0 {
					return receipt, fmt.Errorf("transaction failed: %s", txHash.Hex())
				}
				return receipt, nil
			}
			// Transaction not yet mined, continue waiting
		}
	}
}

// GetTransactionReceipt gets the receipt for a transaction
func (c *Client) GetTransactionReceipt(ctx context.Context, txHash common.Hash) (*types.Receipt, error) {
	return c.ethClient.TransactionReceipt(ctx, txHash)
}

// IsContractDeployed checks if a contract exists at the given address
func (c *Client) IsContractDeployed(ctx context.Context, address common.Address) (bool, error) {
	code, err := c.ethClient.CodeAt(ctx, address, nil)
	if err != nil {
		return false, fmt.Errorf("failed to get code at address: %w", err)
	}
	return len(code) > 0, nil
}

// CreateTransactOpts creates transaction options for sending transactions
func (c *Client) CreateTransactOpts(ctx context.Context) (*bind.TransactOpts, error) {
	chainID, err := c.ethClient.ChainID(ctx)
	if err != nil {
		return nil, fmt.Errorf("failed to get chain ID: %w", err)
	}

	auth, err := bind.NewKeyedTransactorWithChainID(c.privateKey, chainID)
	if err != nil {
		return nil, fmt.Errorf("failed to create transactor: %w", err)
	}

	nonce, err := c.ethClient.PendingNonceAt(ctx, c.fromAddress)
	if err != nil {
		return nil, fmt.Errorf("failed to get nonce: %w", err)
	}

	gasPrice, err := c.ethClient.SuggestGasPrice(ctx)
	if err != nil {
		return nil, fmt.Errorf("failed to suggest gas price: %w", err)
	}

	auth.Nonce = big.NewInt(int64(nonce))
	auth.GasPrice = gasPrice
	auth.Context = ctx

	return auth, nil
}

// SignAndSendTransaction creates, signs, and sends a transaction
func (c *Client) SignAndSendTransaction(
	ctx context.Context,
	to common.Address,
	data []byte,
	value *big.Int,
) (common.Hash, error) {
	chainID, err := c.ethClient.ChainID(ctx)
	if err != nil {
		return common.Hash{}, fmt.Errorf("failed to get chain ID: %w", err)
	}

	nonce, err := c.ethClient.PendingNonceAt(ctx, c.fromAddress)
	if err != nil {
		return common.Hash{}, fmt.Errorf("failed to get nonce: %w", err)
	}

	gasPrice, err := c.ethClient.SuggestGasPrice(ctx)
	if err != nil {
		return common.Hash{}, fmt.Errorf("failed to suggest gas price: %w", err)
	}

	// Estimate gas
	gasLimit, err := c.ethClient.EstimateGas(ctx, ethereum.CallMsg{
		From:  c.fromAddress,
		To:    &to,
		Data:  data,
		Value: value,
	})
	if err != nil {
		return common.Hash{}, fmt.Errorf("failed to estimate gas: %w", err)
	}

	// Add 20% buffer
	gasLimit = gasLimit * 120 / 100

	// Create transaction
	tx := types.NewTransaction(nonce, to, value, gasLimit, gasPrice, data)

	// Sign transaction
	signedTx, err := types.SignTx(tx, types.NewEIP155Signer(chainID), c.privateKey)
	if err != nil {
		return common.Hash{}, fmt.Errorf("failed to sign transaction: %w", err)
	}

	// Send transaction
	if err := c.ethClient.SendTransaction(ctx, signedTx); err != nil {
		return common.Hash{}, fmt.Errorf("failed to send transaction: %w", err)
	}

	c.logger.Info("Transaction sent",
		zap.String("tx_hash", signedTx.Hash().Hex()),
		zap.String("to", to.Hex()),
		zap.Uint64("nonce", nonce),
		zap.Uint64("gas_limit", gasLimit))

	return signedTx.Hash(), nil
}
