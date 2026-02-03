package cosmos

import (
	"context"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strings"
	"time"

	"cosmossdk.io/math"
	"github.com/cosmos/cosmos-sdk/client"
	"github.com/cosmos/cosmos-sdk/codec"
	codectypes "github.com/cosmos/cosmos-sdk/codec/types"
	cryptocodec "github.com/cosmos/cosmos-sdk/crypto/codec"
	"github.com/cosmos/cosmos-sdk/crypto/hd"
	"github.com/cosmos/cosmos-sdk/crypto/keyring"
	cryptotypes "github.com/cosmos/cosmos-sdk/crypto/types"
	sdk "github.com/cosmos/cosmos-sdk/types"
	"github.com/cosmos/cosmos-sdk/types/tx/signing"
	authsigning "github.com/cosmos/cosmos-sdk/x/auth/signing"
	authtx "github.com/cosmos/cosmos-sdk/x/auth/tx"
	authtypes "github.com/cosmos/cosmos-sdk/x/auth/types"
	banktypes "github.com/cosmos/cosmos-sdk/x/bank/types"
	rpchttp "github.com/cometbft/cometbft/rpc/client/http"
	"go.uber.org/zap"

	wasmtypes "github.com/CosmWasm/wasmd/x/wasm/types"

	"hydro/offchain/internal/config"
)

const (
	// Neutron chain configuration
	NeutronBech32Prefix = "neutron"
	NeutronCoinType     = 118
	DefaultGasLimit     = 500000
	DefaultGasAdjust    = 1.5
	DefaultFeeDenom     = "untrn"
	DefaultGasPrice     = 0.025
)

// Client wraps Cosmos SDK client functionality for interacting with Neutron
type Client struct {
	rpcClient    *rpchttp.HTTP
	restEndpoint string
	cdc          codec.Codec
	interfaceReg codectypes.InterfaceRegistry
	txConfig     client.TxConfig
	keyring      keyring.Keyring
	operatorAddr sdk.AccAddress
	pubKey       cryptotypes.PubKey
	chainID      string
	cfg          *config.NeutronConfig
	logger       *zap.Logger
}

// NewClient creates a new Cosmos client for Neutron
func NewClient(cfg *config.NeutronConfig, operatorMnemonic string, logger *zap.Logger) (*Client, error) {
	// Set Bech32 prefix for Neutron
	sdkConfig := sdk.GetConfig()
	sdkConfig.SetBech32PrefixForAccount(NeutronBech32Prefix, NeutronBech32Prefix+"pub")
	sdkConfig.SetBech32PrefixForValidator(NeutronBech32Prefix+"valoper", NeutronBech32Prefix+"valoperpub")
	sdkConfig.SetBech32PrefixForConsensusNode(NeutronBech32Prefix+"valcons", NeutronBech32Prefix+"valconspub")

	// Create RPC client
	rpcClient, err := rpchttp.New(cfg.RPCEndpoint, "/websocket")
	if err != nil {
		return nil, fmt.Errorf("failed to create RPC client: %w", err)
	}

	// Get chain ID from RPC
	status, err := rpcClient.Status(context.Background())
	if err != nil {
		return nil, fmt.Errorf("failed to get chain status: %w", err)
	}
	chainID := status.NodeInfo.Network

	// Create codec and interface registry
	interfaceRegistry := codectypes.NewInterfaceRegistry()
	cryptocodec.RegisterInterfaces(interfaceRegistry)
	authtypes.RegisterInterfaces(interfaceRegistry)
	banktypes.RegisterInterfaces(interfaceRegistry)
	wasmtypes.RegisterInterfaces(interfaceRegistry)
	cdc := codec.NewProtoCodec(interfaceRegistry)

	// Create tx config
	txConfig := authtx.NewTxConfig(cdc, authtx.DefaultSignModes)

	// Create in-memory keyring
	kr := keyring.NewInMemory(cdc)

	// Derive key from mnemonic
	hdPath := hd.CreateHDPath(NeutronCoinType, 0, 0).String()
	record, err := kr.NewAccount("operator", operatorMnemonic, "", hdPath, hd.Secp256k1)
	if err != nil {
		return nil, fmt.Errorf("failed to create key from mnemonic: %w", err)
	}

	pubKey, err := record.GetPubKey()
	if err != nil {
		return nil, fmt.Errorf("failed to get public key: %w", err)
	}
	operatorAddr := sdk.AccAddress(pubKey.Address())

	// Use configured REST endpoint, or derive from RPC endpoint as fallback
	restEndpoint := cfg.RESTEndpoint
	if restEndpoint == "" {
		// Fallback: derive from RPC endpoint (works for standard Cosmos port layouts)
		restEndpoint = strings.Replace(cfg.RPCEndpoint, ":26657", ":1317", 1)
	}

	logger.Info("Cosmos client initialized",
		zap.String("chain_id", chainID),
		zap.String("rpc_endpoint", cfg.RPCEndpoint),
		zap.String("operator_address", operatorAddr.String()))

	return &Client{
		rpcClient:    rpcClient,
		restEndpoint: restEndpoint,
		cdc:          cdc,
		interfaceReg: interfaceRegistry,
		txConfig:     txConfig,
		keyring:      kr,
		operatorAddr: operatorAddr,
		pubKey:       pubKey,
		chainID:      chainID,
		cfg:          cfg,
		logger:       logger,
	}, nil
}

// Close closes the RPC client connection
func (c *Client) Close() error {
	return c.rpcClient.Stop()
}

// OperatorAddress returns the operator's address
func (c *Client) OperatorAddress() sdk.AccAddress {
	return c.operatorAddr
}

// ChainID returns the chain ID
func (c *Client) ChainID() string {
	return c.chainID
}

// GetBalance returns the balance of a specific denom for an address via REST API
func (c *Client) GetBalance(ctx context.Context, address string, denom string) (sdk.Coin, error) {
	url := fmt.Sprintf("%s/cosmos/bank/v1beta1/balances/%s/by_denom?denom=%s",
		c.restEndpoint, address, denom)

	resp, err := http.Get(url)
	if err != nil {
		return sdk.Coin{}, fmt.Errorf("failed to query balance: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		body, _ := io.ReadAll(resp.Body)
		return sdk.Coin{}, fmt.Errorf("balance query failed: %s", string(body))
	}

	var result struct {
		Balance struct {
			Denom  string `json:"denom"`
			Amount string `json:"amount"`
		} `json:"balance"`
	}

	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return sdk.Coin{}, fmt.Errorf("failed to decode balance response: %w", err)
	}

	amount, ok := math.NewIntFromString(result.Balance.Amount)
	if !ok {
		return sdk.NewCoin(denom, math.ZeroInt()), nil
	}

	return sdk.NewCoin(result.Balance.Denom, amount), nil
}

// GetUSDCBalance returns the USDC balance for an address on Neutron
func (c *Client) GetUSDCBalance(ctx context.Context, address string) (math.Int, error) {
	// USDC from Noble via IBC
	usdcDenom := "ibc/B559A80D62249C8AA07A380E2A2BEA6E5CA9A6F079C912C3A9E9B494105E4F81"

	balance, err := c.GetBalance(ctx, address, usdcDenom)
	if err != nil {
		return math.ZeroInt(), err
	}

	return balance.Amount, nil
}

// GetAccountInfo returns account number and sequence for transaction signing
func (c *Client) GetAccountInfo(ctx context.Context, address string) (uint64, uint64, error) {
	url := fmt.Sprintf("%s/cosmos/auth/v1beta1/accounts/%s", c.restEndpoint, address)

	resp, err := http.Get(url)
	if err != nil {
		return 0, 0, fmt.Errorf("failed to query account: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		body, _ := io.ReadAll(resp.Body)
		return 0, 0, fmt.Errorf("account query failed: %s", string(body))
	}

	var result struct {
		Account struct {
			AccountNumber string `json:"account_number"`
			Sequence      string `json:"sequence"`
		} `json:"account"`
	}

	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return 0, 0, fmt.Errorf("failed to decode account response: %w", err)
	}

	var accountNum, sequence uint64
	fmt.Sscanf(result.Account.AccountNumber, "%d", &accountNum)
	fmt.Sscanf(result.Account.Sequence, "%d", &sequence)

	return accountNum, sequence, nil
}

// QueryContract queries a CosmWasm contract via REST API
func (c *Client) QueryContract(ctx context.Context, contractAddr string, queryMsg interface{}) ([]byte, error) {
	queryMsgBytes, err := json.Marshal(queryMsg)
	if err != nil {
		return nil, fmt.Errorf("failed to marshal query message: %w", err)
	}

	queryBase64 := base64.StdEncoding.EncodeToString(queryMsgBytes)

	url := fmt.Sprintf("%s/cosmwasm/wasm/v1/contract/%s/smart/%s",
		c.restEndpoint, contractAddr, queryBase64)

	resp, err := http.Get(url)
	if err != nil {
		return nil, fmt.Errorf("failed to query contract: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		body, _ := io.ReadAll(resp.Body)
		return nil, fmt.Errorf("contract query failed: %s", string(body))
	}

	var result struct {
		Data json.RawMessage `json:"data"`
	}

	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return nil, fmt.Errorf("failed to decode contract response: %w", err)
	}

	return result.Data, nil
}

// ExecuteContract executes a CosmWasm contract message
func (c *Client) ExecuteContract(
	ctx context.Context,
	contractAddr string,
	executeMsg interface{},
	funds sdk.Coins,
) (string, error) {
	executeMsgBytes, err := json.Marshal(executeMsg)
	if err != nil {
		return "", fmt.Errorf("failed to marshal execute message: %w", err)
	}

	msg := &wasmtypes.MsgExecuteContract{
		Sender:   c.operatorAddr.String(),
		Contract: contractAddr,
		Msg:      executeMsgBytes,
		Funds:    funds,
	}

	return c.SignAndBroadcast(ctx, msg)
}

// InstantiateContract2 instantiates a contract using instantiate2 (deterministic address)
func (c *Client) InstantiateContract2(
	ctx context.Context,
	codeID uint64,
	label string,
	instantiateMsg interface{},
	salt []byte,
	funds sdk.Coins,
) (string, string, error) {
	instantiateMsgBytes, err := json.Marshal(instantiateMsg)
	if err != nil {
		return "", "", fmt.Errorf("failed to marshal instantiate message: %w", err)
	}

	msg := &wasmtypes.MsgInstantiateContract2{
		Sender: c.operatorAddr.String(),
		Admin:  c.operatorAddr.String(),
		CodeID: codeID,
		Label:  label,
		Msg:    instantiateMsgBytes,
		Funds:  funds,
		Salt:   salt,
		FixMsg: false,
	}

	txHash, err := c.SignAndBroadcast(ctx, msg)
	if err != nil {
		return "", "", err
	}

	// Wait for transaction to be included
	if err := c.WaitForTx(ctx, txHash, 30*time.Second); err != nil {
		return txHash, "", err
	}

	// Get contract address from transaction result
	contractAddr, err := c.GetContractAddressFromTx(ctx, txHash)
	if err != nil {
		return txHash, "", err
	}

	return txHash, contractAddr, nil
}

// SignAndBroadcast signs and broadcasts a transaction using cosmos-sdk tx builder
func (c *Client) SignAndBroadcast(ctx context.Context, msgs ...sdk.Msg) (string, error) {
	// Get account info
	accountNum, sequence, err := c.GetAccountInfo(ctx, c.operatorAddr.String())
	if err != nil {
		return "", fmt.Errorf("failed to get account info: %w", err)
	}

	// Build the transaction
	txBuilder := c.txConfig.NewTxBuilder()

	// Set messages
	if err := txBuilder.SetMsgs(msgs...); err != nil {
		return "", fmt.Errorf("failed to set messages: %w", err)
	}

	// Set gas limit and fees
	txBuilder.SetGasLimit(DefaultGasLimit)
	feeAmount := int64(float64(DefaultGasLimit) * DefaultGasPrice)
	txBuilder.SetFeeAmount(sdk.NewCoins(sdk.NewCoin(DefaultFeeDenom, math.NewInt(feeAmount))))

	// Set empty memo
	txBuilder.SetMemo("")

	// Create signature
	sigV2 := signing.SignatureV2{
		PubKey: c.pubKey,
		Data: &signing.SingleSignatureData{
			SignMode:  signing.SignMode_SIGN_MODE_DIRECT,
			Signature: nil,
		},
		Sequence: sequence,
	}

	// Set signature placeholder to get proper sign bytes
	if err := txBuilder.SetSignatures(sigV2); err != nil {
		return "", fmt.Errorf("failed to set signature placeholder: %w", err)
	}

	// Create signer data
	signerData := authsigning.SignerData{
		ChainID:       c.chainID,
		AccountNumber: accountNum,
		Sequence:      sequence,
	}

	// Get sign bytes using the tx config's sign mode handler
	signBytes, err := authsigning.GetSignBytesAdapter(
		ctx,
		c.txConfig.SignModeHandler(),
		signing.SignMode_SIGN_MODE_DIRECT,
		signerData,
		txBuilder.GetTx(),
	)
	if err != nil {
		return "", fmt.Errorf("failed to get sign bytes: %w", err)
	}

	// Sign the transaction
	sigBytes, _, err := c.keyring.Sign("operator", signBytes, signing.SignMode_SIGN_MODE_DIRECT)
	if err != nil {
		return "", fmt.Errorf("failed to sign transaction: %w", err)
	}

	// Set the actual signature
	sigV2.Data = &signing.SingleSignatureData{
		SignMode:  signing.SignMode_SIGN_MODE_DIRECT,
		Signature: sigBytes,
	}

	if err := txBuilder.SetSignatures(sigV2); err != nil {
		return "", fmt.Errorf("failed to set final signature: %w", err)
	}

	// Encode the transaction
	txBytes, err := c.txConfig.TxEncoder()(txBuilder.GetTx())
	if err != nil {
		return "", fmt.Errorf("failed to encode transaction: %w", err)
	}

	// Broadcast via RPC (sync mode)
	resp, err := c.rpcClient.BroadcastTxSync(ctx, txBytes)
	if err != nil {
		return "", fmt.Errorf("failed to broadcast transaction: %w", err)
	}

	if resp.Code != 0 {
		return "", fmt.Errorf("transaction failed with code %d: %s", resp.Code, resp.Log)
	}

	txHash := strings.ToUpper(hex.EncodeToString(resp.Hash))
	c.logger.Info("Transaction broadcast successfully",
		zap.String("tx_hash", txHash),
		zap.Uint64("account_number", accountNum),
		zap.Uint64("sequence", sequence))

	return txHash, nil
}

// SignAndBroadcastWithGas signs and broadcasts a transaction with custom gas limit
func (c *Client) SignAndBroadcastWithGas(ctx context.Context, gasLimit uint64, msgs ...sdk.Msg) (string, error) {
	// Get account info
	accountNum, sequence, err := c.GetAccountInfo(ctx, c.operatorAddr.String())
	if err != nil {
		return "", fmt.Errorf("failed to get account info: %w", err)
	}

	// Build the transaction
	txBuilder := c.txConfig.NewTxBuilder()

	if err := txBuilder.SetMsgs(msgs...); err != nil {
		return "", fmt.Errorf("failed to set messages: %w", err)
	}

	txBuilder.SetGasLimit(gasLimit)
	feeAmount := int64(float64(gasLimit) * DefaultGasPrice)
	txBuilder.SetFeeAmount(sdk.NewCoins(sdk.NewCoin(DefaultFeeDenom, math.NewInt(feeAmount))))
	txBuilder.SetMemo("")

	// Create and set signature
	sigV2 := signing.SignatureV2{
		PubKey: c.pubKey,
		Data: &signing.SingleSignatureData{
			SignMode:  signing.SignMode_SIGN_MODE_DIRECT,
			Signature: nil,
		},
		Sequence: sequence,
	}

	if err := txBuilder.SetSignatures(sigV2); err != nil {
		return "", fmt.Errorf("failed to set signature placeholder: %w", err)
	}

	signerData := authsigning.SignerData{
		ChainID:       c.chainID,
		AccountNumber: accountNum,
		Sequence:      sequence,
	}

	signBytes, err := authsigning.GetSignBytesAdapter(
		ctx,
		c.txConfig.SignModeHandler(),
		signing.SignMode_SIGN_MODE_DIRECT,
		signerData,
		txBuilder.GetTx(),
	)
	if err != nil {
		return "", fmt.Errorf("failed to get sign bytes: %w", err)
	}

	sigBytes, _, err := c.keyring.Sign("operator", signBytes, signing.SignMode_SIGN_MODE_DIRECT)
	if err != nil {
		return "", fmt.Errorf("failed to sign transaction: %w", err)
	}

	sigV2.Data = &signing.SingleSignatureData{
		SignMode:  signing.SignMode_SIGN_MODE_DIRECT,
		Signature: sigBytes,
	}

	if err := txBuilder.SetSignatures(sigV2); err != nil {
		return "", fmt.Errorf("failed to set final signature: %w", err)
	}

	txBytes, err := c.txConfig.TxEncoder()(txBuilder.GetTx())
	if err != nil {
		return "", fmt.Errorf("failed to encode transaction: %w", err)
	}

	resp, err := c.rpcClient.BroadcastTxSync(ctx, txBytes)
	if err != nil {
		return "", fmt.Errorf("failed to broadcast transaction: %w", err)
	}

	if resp.Code != 0 {
		return "", fmt.Errorf("transaction failed with code %d: %s", resp.Code, resp.Log)
	}

	txHash := strings.ToUpper(hex.EncodeToString(resp.Hash))
	c.logger.Info("Transaction broadcast successfully",
		zap.String("tx_hash", txHash),
		zap.Uint64("gas_limit", gasLimit))

	return txHash, nil
}

// WaitForTx waits for a transaction to be included in a block
func (c *Client) WaitForTx(ctx context.Context, txHash string, timeout time.Duration) error {
	ctx, cancel := context.WithTimeout(ctx, timeout)
	defer cancel()

	ticker := time.NewTicker(2 * time.Second)
	defer ticker.Stop()

	hashBytes, err := hex.DecodeString(txHash)
	if err != nil {
		return fmt.Errorf("invalid tx hash: %w", err)
	}

	for {
		select {
		case <-ctx.Done():
			return fmt.Errorf("timeout waiting for transaction %s", txHash)
		case <-ticker.C:
			result, err := c.rpcClient.Tx(ctx, hashBytes, false)
			if err != nil {
				continue // Transaction not found yet
			}

			if result.TxResult.Code != 0 {
				return fmt.Errorf("transaction failed with code %d: %s", result.TxResult.Code, result.TxResult.Log)
			}

			c.logger.Info("Transaction confirmed",
				zap.String("tx_hash", txHash),
				zap.Int64("height", result.Height))

			return nil
		}
	}
}

// GetContractAddressFromTx extracts the contract address from a transaction result
func (c *Client) GetContractAddressFromTx(ctx context.Context, txHash string) (string, error) {
	hashBytes, err := hex.DecodeString(txHash)
	if err != nil {
		return "", fmt.Errorf("invalid tx hash: %w", err)
	}

	result, err := c.rpcClient.Tx(ctx, hashBytes, false)
	if err != nil {
		return "", fmt.Errorf("failed to get transaction: %w", err)
	}

	// Look for instantiate event with contract address
	for _, event := range result.TxResult.Events {
		if event.Type == "instantiate" {
			for _, attr := range event.Attributes {
				if string(attr.Key) == "_contract_address" {
					return string(attr.Value), nil
				}
			}
		}
	}

	return "", fmt.Errorf("contract address not found in transaction events")
}

// GetTxStatus returns the status of a transaction
func (c *Client) GetTxStatus(ctx context.Context, txHash string) (bool, error) {
	hashBytes, err := hex.DecodeString(txHash)
	if err != nil {
		return false, fmt.Errorf("invalid tx hash: %w", err)
	}

	result, err := c.rpcClient.Tx(ctx, hashBytes, false)
	if err != nil {
		return false, nil // Not found yet
	}

	return result.TxResult.Code == 0, nil
}

// CodeInfo contains information about a stored wasm code
type CodeInfo struct {
	CodeID   uint64
	Creator  string
	Checksum []byte // SHA256 hash of the wasm bytecode
}

// GetCodeInfo queries code information including the checksum from the chain
func (c *Client) GetCodeInfo(ctx context.Context, codeID uint64) (*CodeInfo, error) {
	url := fmt.Sprintf("%s/cosmwasm/wasm/v1/code/%d", c.restEndpoint, codeID)

	resp, err := http.Get(url)
	if err != nil {
		return nil, fmt.Errorf("failed to query code info: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		body, _ := io.ReadAll(resp.Body)
		return nil, fmt.Errorf("code info query failed: %s", string(body))
	}

	var result struct {
		CodeInfo struct {
			CodeID   string `json:"code_id"`
			Creator  string `json:"creator"`
			Checksum string `json:"data_hash"` // Base64 or hex encoded checksum
		} `json:"code_info"`
	}

	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return nil, fmt.Errorf("failed to decode code info response: %w", err)
	}

	// Decode checksum (try hex first, then base64)
	var checksum []byte
	checksum, err = hex.DecodeString(result.CodeInfo.Checksum)
	if err != nil {
		// Try base64
		checksum, err = base64.StdEncoding.DecodeString(result.CodeInfo.Checksum)
		if err != nil {
			return nil, fmt.Errorf("failed to decode checksum: %w", err)
		}
	}

	var parsedCodeID uint64
	fmt.Sscanf(result.CodeInfo.CodeID, "%d", &parsedCodeID)

	return &CodeInfo{
		CodeID:   parsedCodeID,
		Creator:  result.CodeInfo.Creator,
		Checksum: checksum,
	}, nil
}
