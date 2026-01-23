package service

import (
	"fmt"

	"go.uber.org/zap"

	"hydro/offchain/internal/config"
)

// FeeService handles fee calculations
type FeeService struct {
	cfg    *config.Config
	logger *zap.Logger
}

// NewFeeService creates a new fee service
func NewFeeService(cfg *config.Config, logger *zap.Logger) *FeeService {
	return &FeeService{
		cfg:    cfg,
		logger: logger,
	}
}

// FeeCalculation holds calculated fee information
type FeeCalculation struct {
	BridgeFeeUSDC  int64 // Bridge operational fee in base units (6 decimals)
	MinDepositUSDC int64 // Minimum deposit amount in base units (6 decimals)
}

// CalculateBridgeFee calculates the bridge operational fee for a given amount
// The fee is calculated as: max(amount * operationalFeeBps / 10000, minOperationalFee)
//
// Parameters:
//   - chainID: The EVM chain ID (e.g., "1" for Ethereum, "8453" for Base)
//   - amountUSDC: The amount to bridge in base units (6 decimals, e.g., 1000000 = 1 USDC)
//
// Returns:
//   - FeeCalculation with the calculated fee and minimum deposit amount
func (s *FeeService) CalculateBridgeFee(chainID string, amountUSDC int64) (*FeeCalculation, error) {
	// Get chain config
	chainCfg, ok := s.cfg.Chains[chainID]
	if !ok {
		return nil, fmt.Errorf("chain %s not configured", chainID)
	}

	// Calculate operational fee: amount * bps / 10000
	// Example: 100 USDC * 50 bps / 10000 = 0.5 USDC
	fee := (amountUSDC * int64(chainCfg.OperationalFeeBps)) / 10000

	// Apply minimum fee if calculated fee is below minimum
	if fee < chainCfg.MinOperationalFee {
		fee = chainCfg.MinOperationalFee
	}

	s.logger.Debug("Calculated bridge fee",
		zap.String("chain_id", chainID),
		zap.Int64("amount_usdc", amountUSDC),
		zap.Int64("fee_usdc", fee),
		zap.Int64("min_deposit_usdc", chainCfg.MinDepositAmount))

	return &FeeCalculation{
		BridgeFeeUSDC:  fee,
		MinDepositUSDC: chainCfg.MinDepositAmount,
	}, nil
}

// GetMinDepositAmount returns the minimum deposit amount for a chain
func (s *FeeService) GetMinDepositAmount(chainID string) (int64, error) {
	chainCfg, ok := s.cfg.Chains[chainID]
	if !ok {
		return 0, fmt.Errorf("chain %s not configured", chainID)
	}
	return chainCfg.MinDepositAmount, nil
}

// ValidateAmount checks if an amount meets the minimum deposit requirement
func (s *FeeService) ValidateAmount(chainID string, amountUSDC int64) error {
	chainCfg, ok := s.cfg.Chains[chainID]
	if !ok {
		return fmt.Errorf("chain %s not configured", chainID)
	}

	if amountUSDC < chainCfg.MinDepositAmount {
		return fmt.Errorf("amount %d is below minimum deposit %d for chain %s",
			amountUSDC, chainCfg.MinDepositAmount, chainID)
	}

	return nil
}

// CalculateNetAmount calculates the net amount after deducting fees
// This is useful to know how much will actually be deposited into the vault
func (s *FeeService) CalculateNetAmount(chainID string, grossAmountUSDC int64) (int64, error) {
	feeCalc, err := s.CalculateBridgeFee(chainID, grossAmountUSDC)
	if err != nil {
		return 0, err
	}

	netAmount := grossAmountUSDC - feeCalc.BridgeFeeUSDC

	if netAmount <= 0 {
		return 0, fmt.Errorf("net amount would be zero or negative after fees")
	}

	return netAmount, nil
}
