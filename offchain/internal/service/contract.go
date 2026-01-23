package service

import (
	"context"
	"encoding/hex"
	"fmt"
	"strings"

	"github.com/ethereum/go-ethereum/common"
	"go.uber.org/zap"

	"hydro/offchain/internal/blockchain/cosmos"
	"hydro/offchain/internal/blockchain/evm"
	"hydro/offchain/internal/config"
	"hydro/offchain/internal/database"
	"hydro/offchain/internal/models"
)

// ContractService handles contract address computation and management
type ContractService struct {
	db     *database.DB
	cfg    *config.Config
	logger *zap.Logger
}

// NewContractService creates a new contract service
func NewContractService(db *database.DB, cfg *config.Config, logger *zap.Logger) *ContractService {
	return &ContractService{
		db:     db,
		cfg:    cfg,
		logger: logger,
	}
}

// GetOrCreateContractAddresses gets or creates contract addresses for a user on specified chains
// This is idempotent - it will return existing addresses if already computed
func (s *ContractService) GetOrCreateContractAddresses(ctx context.Context, email string, chainIDs []string) (map[string]ContractAddresses, error) {
	s.logger.Info("Getting/creating contract addresses",
		zap.String("email", email),
		zap.Strings("chain_ids", chainIDs))

	// Ensure user exists
	if err := s.db.CreateUser(ctx, email); err != nil {
		return nil, fmt.Errorf("failed to create user: %w", err)
	}

	// Result map: chainID -> ContractAddresses
	result := make(map[string]ContractAddresses)

	// Compute proxy address (shared across all chains)
	proxyAddress, err := s.getOrCreateProxyAddress(ctx, email)
	if err != nil {
		return nil, fmt.Errorf("failed to get/create proxy address: %w", err)
	}

	// Compute forwarder addresses for each chain
	for _, chainID := range chainIDs {
		forwarderAddress, err := s.getOrCreateForwarderAddress(ctx, email, chainID)
		if err != nil {
			s.logger.Error("Failed to get/create forwarder address",
				zap.String("email", email),
				zap.String("chain_id", chainID),
				zap.Error(err))
			return nil, fmt.Errorf("failed to get/create forwarder address for chain %s: %w", chainID, err)
		}

		result[chainID] = ContractAddresses{
			Forwarder: forwarderAddress,
			Proxy:     proxyAddress,
		}
	}

	return result, nil
}

// ContractAddresses holds forwarder and proxy addresses
type ContractAddresses struct {
	Forwarder string
	Proxy     string
}

// getOrCreateForwarderAddress gets or creates a forwarder address for a user on a specific chain
func (s *ContractService) getOrCreateForwarderAddress(ctx context.Context, email, chainID string) (string, error) {
	// Check if contract already exists in DB
	existingContract, err := s.db.GetContract(ctx, email, chainID, models.ContractTypeForwarder)
	if err != nil {
		return "", fmt.Errorf("failed to query contract: %w", err)
	}
	if existingContract != nil {
		return existingContract.Address, nil
	}

	// Get chain config
	chainCfg, ok := s.cfg.Chains[chainID]
	if !ok {
		return "", fmt.Errorf("chain %s not configured", chainID)
	}

	// Parse operator address
	operatorAddress := common.HexToAddress(chainCfg.OperatorAddress)

	// Parse forwarder bytecode (remove 0x prefix if present)
	bytecodeHex := strings.TrimPrefix(chainCfg.ForwarderBytecode, "0x")
	initCode, err := hex.DecodeString(bytecodeHex)
	if err != nil {
		return "", fmt.Errorf("failed to decode forwarder bytecode: %w", err)
	}

	// Compute CREATE2 address
	forwarderAddress, err := evm.ComputeForwarderAddress(operatorAddress, email, chainID, initCode)
	if err != nil {
		return "", fmt.Errorf("failed to compute forwarder address: %w", err)
	}

	// Store in database
	contract := &models.Contract{
		UserEmail:    email,
		ChainID:      chainID,
		ContractType: models.ContractTypeForwarder,
		Address:      forwarderAddress.Hex(),
		Deployed:     false,
	}
	if err := s.db.CreateContract(ctx, contract); err != nil {
		return "", fmt.Errorf("failed to store contract: %w", err)
	}

	s.logger.Info("Computed new forwarder address",
		zap.String("email", email),
		zap.String("chain_id", chainID),
		zap.String("address", forwarderAddress.Hex()))

	return forwarderAddress.Hex(), nil
}

// getOrCreateProxyAddress gets or creates a proxy address for a user
func (s *ContractService) getOrCreateProxyAddress(ctx context.Context, email string) (string, error) {
	// Proxy is stored with chainID as empty string since it's shared across all chains
	// We use "neutron" as a sentinel value to distinguish it
	proxyChainID := "neutron"

	// Check if contract already exists in DB
	existingContract, err := s.db.GetContract(ctx, email, proxyChainID, models.ContractTypeProxy)
	if err != nil {
		return "", fmt.Errorf("failed to query contract: %w", err)
	}
	if existingContract != nil {
		return existingContract.Address, nil
	}

	// Compute instantiate2 address
	proxyAddress, err := cosmos.ComputeProxyAddress(
		s.cfg.Neutron.ProxyCodeID,
		s.cfg.Operator.NeutronAddress,
		email,
	)
	if err != nil {
		return "", fmt.Errorf("failed to compute proxy address: %w", err)
	}

	// Store in database
	contract := &models.Contract{
		UserEmail:    email,
		ChainID:      proxyChainID,
		ContractType: models.ContractTypeProxy,
		Address:      proxyAddress,
		Deployed:     false,
	}
	if err := s.db.CreateContract(ctx, contract); err != nil {
		return "", fmt.Errorf("failed to store contract: %w", err)
	}

	s.logger.Info("Computed new proxy address",
		zap.String("email", email),
		zap.String("address", proxyAddress))

	return proxyAddress, nil
}

// GetForwarderAddress gets the forwarder address for a user on a specific chain
func (s *ContractService) GetForwarderAddress(ctx context.Context, email, chainID string) (string, error) {
	contract, err := s.db.GetContract(ctx, email, chainID, models.ContractTypeForwarder)
	if err != nil {
		return "", fmt.Errorf("failed to query contract: %w", err)
	}
	if contract == nil {
		return "", fmt.Errorf("forwarder contract not found for user %s on chain %s", email, chainID)
	}
	return contract.Address, nil
}

// GetProxyAddress gets the proxy address for a user
func (s *ContractService) GetProxyAddress(ctx context.Context, email string) (string, error) {
	contract, err := s.db.GetContract(ctx, email, "neutron", models.ContractTypeProxy)
	if err != nil {
		return "", fmt.Errorf("failed to query contract: %w", err)
	}
	if contract == nil {
		return "", fmt.Errorf("proxy contract not found for user %s", email)
	}
	return contract.Address, nil
}

// IsForwarderDeployed checks if a forwarder contract has been deployed
func (s *ContractService) IsForwarderDeployed(ctx context.Context, email, chainID string) (bool, error) {
	contract, err := s.db.GetContract(ctx, email, chainID, models.ContractTypeForwarder)
	if err != nil {
		return false, fmt.Errorf("failed to query contract: %w", err)
	}
	if contract == nil {
		return false, nil
	}
	return contract.Deployed, nil
}

// IsProxyDeployed checks if a proxy contract has been deployed
func (s *ContractService) IsProxyDeployed(ctx context.Context, email string) (bool, error) {
	contract, err := s.db.GetContract(ctx, email, "neutron", models.ContractTypeProxy)
	if err != nil {
		return false, fmt.Errorf("failed to query contract: %w", err)
	}
	if contract == nil {
		return false, nil
	}
	return contract.Deployed, nil
}
