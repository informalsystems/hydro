package cosmos

import (
	"context"
	"encoding/json"
	"fmt"
	"time"

	"cosmossdk.io/math"
	sdk "github.com/cosmos/cosmos-sdk/types"
	"go.uber.org/zap"

	"hydro/offchain/internal/config"
)

// Proxy provides methods to interact with the Inflow Proxy contract on Neutron
type Proxy struct {
	client       *Client
	cfg          *config.NeutronConfig
	logger       *zap.Logger
	codeChecksum []byte // Cached code checksum for instantiate2 address computation
}

// NewProxy creates a new Proxy instance
func NewProxy(client *Client, cfg *config.NeutronConfig, logger *zap.Logger) *Proxy {
	return &Proxy{
		client: client,
		cfg:    cfg,
		logger: logger,
	}
}

// Initialize fetches and caches the code checksum from the chain.
// This must be called before using address computation methods.
func (p *Proxy) Initialize(ctx context.Context) error {
	if p.cfg.ProxyCodeID == 0 {
		return fmt.Errorf("proxy code ID not configured")
	}

	codeInfo, err := p.client.GetCodeInfo(ctx, p.cfg.ProxyCodeID)
	if err != nil {
		return fmt.Errorf("failed to get code info: %w", err)
	}

	p.codeChecksum = codeInfo.Checksum
	p.logger.Info("Proxy initialized with code checksum",
		zap.Uint64("code_id", p.cfg.ProxyCodeID),
		zap.Int("checksum_len", len(p.codeChecksum)))

	return nil
}

// GetCodeChecksum returns the cached code checksum
func (p *Proxy) GetCodeChecksum() []byte {
	return p.codeChecksum
}

// ComputeProxyAddressForUser computes the deterministic proxy address for a user
func (p *Proxy) ComputeProxyAddressForUser(userEmail string) (string, error) {
	if len(p.codeChecksum) == 0 {
		return "", fmt.Errorf("proxy not initialized - call Initialize() first")
	}

	salt := GenerateProxySalt(userEmail)
	return ComputeProxyAddress(
		p.codeChecksum,
		p.client.OperatorAddress().String(),
		salt[:],
		nil, // No msg for FixMsg=false
	)
}

// ==================== Message Types ====================

// InstantiateMsg is the message to instantiate the proxy contract
type ProxyInstantiateMsg struct {
	Admins         []string `json:"admins"`
	ControlCenters []string `json:"control_centers"`
}

// ExecuteMsg types for proxy contract
type ForwardToInflowMsg struct {
	ForwardToInflow struct{} `json:"forward_to_inflow"`
}

type WithdrawReceiptTokensMsg struct {
	WithdrawReceiptTokens struct {
		Address string  `json:"address"`
		Coin    CoinMsg `json:"coin"`
	} `json:"withdraw_receipt_tokens"`
}

type WithdrawFundsMsg struct {
	WithdrawFunds struct {
		Address string  `json:"address"`
		Coin    CoinMsg `json:"coin"`
	} `json:"withdraw_funds"`
}

type CoinMsg struct {
	Denom  string `json:"denom"`
	Amount string `json:"amount"`
}

// QueryMsg types
type ConfigQueryMsg struct {
	Config struct{} `json:"config"`
}

type StateQueryMsg struct {
	State struct{} `json:"state"`
}

// Response types
type ProxyConfigResponse struct {
	Config ProxyConfig `json:"config"`
}

type ProxyConfig struct {
	Admins         []string `json:"admins"`
	ControlCenters []string `json:"control_centers"`
}

type ProxyStateResponse struct {
	State ProxyState `json:"state"`
}

type ProxyState struct {
	TotalDeposited []CoinBalance `json:"total_deposited"`
}

type CoinBalance struct {
	Denom  string `json:"denom"`
	Amount string `json:"amount"`
}

// ==================== Instantiation ====================

// InstantiateProxy instantiates a new proxy contract using instantiate2
// Returns the transaction hash and contract address
func (p *Proxy) InstantiateProxy(ctx context.Context, userEmail string) (string, string, error) {
	p.logger.Info("Instantiating proxy contract",
		zap.String("user_email", userEmail),
		zap.Uint64("code_id", p.cfg.ProxyCodeID))

	// Generate deterministic salt from user email
	salt := GenerateProxySalt(userEmail)

	// Create instantiate message
	instantiateMsg := ProxyInstantiateMsg{
		Admins:         p.cfg.Admins,
		ControlCenters: p.cfg.ControlCenters,
	}

	// Create label
	label := fmt.Sprintf("inflow-proxy-%s", userEmail)

	// Instantiate contract with deterministic address
	txHash, contractAddr, err := p.client.InstantiateContract2(
		ctx,
		p.cfg.ProxyCodeID,
		label,
		instantiateMsg,
		salt[:],
		nil, // No funds
	)
	if err != nil {
		return "", "", fmt.Errorf("failed to instantiate proxy: %w", err)
	}

	p.logger.Info("Proxy contract instantiated",
		zap.String("contract_address", contractAddr),
		zap.String("tx_hash", txHash),
		zap.String("user_email", userEmail))

	return txHash, contractAddr, nil
}

// ==================== Execute Functions ====================

// ForwardToInflow calls the ForwardToInflow function on the proxy contract
// This forwards all USDC held by the proxy to the Inflow vault
func (p *Proxy) ForwardToInflow(ctx context.Context, proxyAddress string) (string, error) {
	p.logger.Info("Calling ForwardToInflow on proxy",
		zap.String("proxy_address", proxyAddress))

	// Create execute message
	executeMsg := ForwardToInflowMsg{}

	txHash, err := p.client.ExecuteContract(ctx, proxyAddress, executeMsg, nil)
	if err != nil {
		return "", fmt.Errorf("failed to execute ForwardToInflow: %w", err)
	}

	p.logger.Info("ForwardToInflow transaction sent",
		zap.String("tx_hash", txHash),
		zap.String("proxy_address", proxyAddress))

	return txHash, nil
}

// ForwardToInflowAndWait calls ForwardToInflow and waits for the transaction
func (p *Proxy) ForwardToInflowAndWait(ctx context.Context, proxyAddress string, timeout time.Duration) (string, error) {
	txHash, err := p.ForwardToInflow(ctx, proxyAddress)
	if err != nil {
		return "", err
	}

	if err := p.client.WaitForTx(ctx, txHash, timeout); err != nil {
		return txHash, fmt.Errorf("ForwardToInflow transaction failed: %w", err)
	}

	p.logger.Info("ForwardToInflow transaction confirmed",
		zap.String("tx_hash", txHash),
		zap.String("proxy_address", proxyAddress))

	return txHash, nil
}

// WithdrawReceiptTokens withdraws receipt tokens from the proxy
func (p *Proxy) WithdrawReceiptTokens(ctx context.Context, proxyAddress string, recipientAddress string, coin sdk.Coin) (string, error) {
	p.logger.Info("Withdrawing receipt tokens from proxy",
		zap.String("proxy_address", proxyAddress),
		zap.String("recipient", recipientAddress),
		zap.String("coin", coin.String()))

	executeMsg := WithdrawReceiptTokensMsg{}
	executeMsg.WithdrawReceiptTokens.Address = recipientAddress
	executeMsg.WithdrawReceiptTokens.Coin = CoinMsg{
		Denom:  coin.Denom,
		Amount: coin.Amount.String(),
	}

	txHash, err := p.client.ExecuteContract(ctx, proxyAddress, executeMsg, nil)
	if err != nil {
		return "", fmt.Errorf("failed to execute WithdrawReceiptTokens: %w", err)
	}

	p.logger.Info("WithdrawReceiptTokens transaction sent",
		zap.String("tx_hash", txHash))

	return txHash, nil
}

// WithdrawFunds withdraws funds from the proxy
func (p *Proxy) WithdrawFunds(ctx context.Context, proxyAddress string, recipientAddress string, coin sdk.Coin) (string, error) {
	p.logger.Info("Withdrawing funds from proxy",
		zap.String("proxy_address", proxyAddress),
		zap.String("recipient", recipientAddress),
		zap.String("coin", coin.String()))

	executeMsg := WithdrawFundsMsg{}
	executeMsg.WithdrawFunds.Address = recipientAddress
	executeMsg.WithdrawFunds.Coin = CoinMsg{
		Denom:  coin.Denom,
		Amount: coin.Amount.String(),
	}

	txHash, err := p.client.ExecuteContract(ctx, proxyAddress, executeMsg, nil)
	if err != nil {
		return "", fmt.Errorf("failed to execute WithdrawFunds: %w", err)
	}

	p.logger.Info("WithdrawFunds transaction sent",
		zap.String("tx_hash", txHash))

	return txHash, nil
}

// ==================== Query Functions ====================

// GetConfig queries the proxy contract configuration
func (p *Proxy) GetConfig(ctx context.Context, proxyAddress string) (*ProxyConfig, error) {
	queryMsg := ConfigQueryMsg{}

	resultBytes, err := p.client.QueryContract(ctx, proxyAddress, queryMsg)
	if err != nil {
		return nil, fmt.Errorf("failed to query config: %w", err)
	}

	var response ProxyConfigResponse
	if err := json.Unmarshal(resultBytes, &response); err != nil {
		return nil, fmt.Errorf("failed to unmarshal config response: %w", err)
	}

	return &response.Config, nil
}

// GetState queries the proxy contract state
func (p *Proxy) GetState(ctx context.Context, proxyAddress string) (*ProxyState, error) {
	queryMsg := StateQueryMsg{}

	resultBytes, err := p.client.QueryContract(ctx, proxyAddress, queryMsg)
	if err != nil {
		return nil, fmt.Errorf("failed to query state: %w", err)
	}

	var response ProxyStateResponse
	if err := json.Unmarshal(resultBytes, &response); err != nil {
		return nil, fmt.Errorf("failed to unmarshal state response: %w", err)
	}

	return &response.State, nil
}

// GetProxyUSDCBalance returns the USDC balance of the proxy contract
func (p *Proxy) GetProxyUSDCBalance(ctx context.Context, proxyAddress string) (math.Int, error) {
	return p.client.GetUSDCBalance(ctx, proxyAddress)
}

// IsProxyDeployed checks if a proxy contract is deployed at the given address
func (p *Proxy) IsProxyDeployed(ctx context.Context, proxyAddress string) (bool, error) {
	// Try to query the config - if it succeeds, the contract is deployed
	_, err := p.GetConfig(ctx, proxyAddress)
	if err != nil {
		// Check if error is because contract doesn't exist
		// vs some other query error
		return false, nil
	}
	return true, nil
}

// VerifyProxyAddress verifies that a proxy address matches the expected instantiate2 address
func (p *Proxy) VerifyProxyAddress(expectedAddress string, userEmail string) (bool, error) {
	if len(p.codeChecksum) == 0 {
		return false, fmt.Errorf("proxy not initialized - call Initialize() first")
	}

	salt := GenerateProxySalt(userEmail)
	return VerifyProxyAddress(
		expectedAddress,
		p.codeChecksum,
		p.client.OperatorAddress().String(),
		salt[:],
		nil, // No msg for FixMsg=false
	)
}
