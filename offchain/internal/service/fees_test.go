package service

import (
	"testing"

	"go.uber.org/zap"

	"hydro/offchain/internal/config"
)

func TestFeeService_CalculateBridgeFee(t *testing.T) {
	logger := zap.NewNop()

	tests := []struct {
		name           string
		chainID        string
		amountUSDC     int64
		cfg            *config.Config
		expectedFee    int64
		expectedMinDep int64
		expectError    bool
	}{
		{
			name:       "fee based on percentage (50 bps = 0.5%) - min fee applies",
			chainID:    "1",
			amountUSDC: 100_000_000, // 100 USDC
			cfg: &config.Config{
				Chains: map[string]config.ChainConfig{
					"1": {
						OperationalFeeBps: 50,          // 0.5%
						MinOperationalFee: 1_000_000,   // 1 USDC
						MinDepositAmount:  10_000_000,  // 10 USDC
					},
				},
			},
			expectedFee:    1_000_000, // 100 * 0.5% = 0.5 USDC, but min is 1 USDC
			expectedMinDep: 10_000_000,
			expectError:    false,
		},
		{
			name:       "fee falls back to minimum",
			chainID:    "1",
			amountUSDC: 10_000_000, // 10 USDC
			cfg: &config.Config{
				Chains: map[string]config.ChainConfig{
					"1": {
						OperationalFeeBps: 50,          // 0.5%
						MinOperationalFee: 1_000_000,   // 1 USDC
						MinDepositAmount:  10_000_000,  // 10 USDC
					},
				},
			},
			expectedFee:    1_000_000, // 10 * 0.5% = 0.05 USDC, but min is 1 USDC
			expectedMinDep: 10_000_000,
			expectError:    false,
		},
		{
			name:       "large amount",
			chainID:    "8453",
			amountUSDC: 1_000_000_000, // 1000 USDC
			cfg: &config.Config{
				Chains: map[string]config.ChainConfig{
					"8453": {
						OperationalFeeBps: 100,         // 1%
						MinOperationalFee: 500_000,     // 0.5 USDC
						MinDepositAmount:  50_000_000,  // 50 USDC
					},
				},
			},
			expectedFee:    10_000_000, // 1000 * 1% = 10 USDC
			expectedMinDep: 50_000_000,
			expectError:    false,
		},
		{
			name:       "zero bps still uses minimum fee",
			chainID:    "1",
			amountUSDC: 100_000_000, // 100 USDC
			cfg: &config.Config{
				Chains: map[string]config.ChainConfig{
					"1": {
						OperationalFeeBps: 0,           // 0%
						MinOperationalFee: 2_000_000,   // 2 USDC
						MinDepositAmount:  10_000_000,  // 10 USDC
					},
				},
			},
			expectedFee:    2_000_000, // 0% = 0, but min is 2 USDC
			expectedMinDep: 10_000_000,
			expectError:    false,
		},
		{
			name:       "unconfigured chain returns error",
			chainID:    "999",
			amountUSDC: 100_000_000,
			cfg: &config.Config{
				Chains: map[string]config.ChainConfig{
					"1": {
						OperationalFeeBps: 50,
						MinOperationalFee: 1_000_000,
						MinDepositAmount:  10_000_000,
					},
				},
			},
			expectError: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			svc := NewFeeService(tt.cfg, logger)

			result, err := svc.CalculateBridgeFee(tt.chainID, tt.amountUSDC)

			if tt.expectError {
				if err == nil {
					t.Error("expected error but got nil")
				}
				return
			}

			if err != nil {
				t.Fatalf("unexpected error: %v", err)
			}

			if result.BridgeFeeUSDC != tt.expectedFee {
				t.Errorf("expected fee %d, got %d", tt.expectedFee, result.BridgeFeeUSDC)
			}

			if result.MinDepositUSDC != tt.expectedMinDep {
				t.Errorf("expected min deposit %d, got %d", tt.expectedMinDep, result.MinDepositUSDC)
			}
		})
	}
}

func TestFeeService_GetMinDepositAmount(t *testing.T) {
	logger := zap.NewNop()
	cfg := &config.Config{
		Chains: map[string]config.ChainConfig{
			"1": {
				MinDepositAmount: 10_000_000, // 10 USDC
			},
			"8453": {
				MinDepositAmount: 50_000_000, // 50 USDC
			},
		},
	}

	svc := NewFeeService(cfg, logger)

	tests := []struct {
		name        string
		chainID     string
		expected    int64
		expectError bool
	}{
		{
			name:     "ethereum",
			chainID:  "1",
			expected: 10_000_000,
		},
		{
			name:     "base",
			chainID:  "8453",
			expected: 50_000_000,
		},
		{
			name:        "unknown chain",
			chainID:     "999",
			expectError: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result, err := svc.GetMinDepositAmount(tt.chainID)

			if tt.expectError {
				if err == nil {
					t.Error("expected error but got nil")
				}
				return
			}

			if err != nil {
				t.Fatalf("unexpected error: %v", err)
			}

			if result != tt.expected {
				t.Errorf("expected %d, got %d", tt.expected, result)
			}
		})
	}
}

func TestFeeService_ValidateAmount(t *testing.T) {
	logger := zap.NewNop()
	cfg := &config.Config{
		Chains: map[string]config.ChainConfig{
			"1": {
				MinDepositAmount: 10_000_000, // 10 USDC
			},
		},
	}

	svc := NewFeeService(cfg, logger)

	tests := []struct {
		name        string
		chainID     string
		amount      int64
		expectError bool
	}{
		{
			name:        "valid amount - equal to minimum",
			chainID:     "1",
			amount:      10_000_000,
			expectError: false,
		},
		{
			name:        "valid amount - above minimum",
			chainID:     "1",
			amount:      100_000_000,
			expectError: false,
		},
		{
			name:        "invalid amount - below minimum",
			chainID:     "1",
			amount:      5_000_000,
			expectError: true,
		},
		{
			name:        "invalid chain",
			chainID:     "999",
			amount:      100_000_000,
			expectError: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			err := svc.ValidateAmount(tt.chainID, tt.amount)

			if tt.expectError && err == nil {
				t.Error("expected error but got nil")
			}
			if !tt.expectError && err != nil {
				t.Errorf("unexpected error: %v", err)
			}
		})
	}
}

func TestFeeService_CalculateNetAmount(t *testing.T) {
	logger := zap.NewNop()
	cfg := &config.Config{
		Chains: map[string]config.ChainConfig{
			"1": {
				OperationalFeeBps: 50,          // 0.5%
				MinOperationalFee: 1_000_000,   // 1 USDC
				MinDepositAmount:  10_000_000,  // 10 USDC
			},
		},
	}

	svc := NewFeeService(cfg, logger)

	tests := []struct {
		name        string
		chainID     string
		grossAmount int64
		expectedNet int64
		expectError bool
	}{
		{
			name:        "normal amount - min fee applies",
			chainID:     "1",
			grossAmount: 100_000_000, // 100 USDC
			expectedNet: 99_000_000,  // 100 - 1 (min fee) = 99 USDC
			expectError: false,
		},
		{
			name:        "small amount uses min fee",
			chainID:     "1",
			grossAmount: 10_000_000, // 10 USDC
			expectedNet: 9_000_000,  // 10 - 1 (min fee) = 9 USDC
			expectError: false,
		},
		{
			name:        "amount too small - net would be zero or negative",
			chainID:     "1",
			grossAmount: 500_000, // 0.5 USDC - less than min fee
			expectError: true,
		},
		{
			name:        "unknown chain",
			chainID:     "999",
			grossAmount: 100_000_000,
			expectError: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result, err := svc.CalculateNetAmount(tt.chainID, tt.grossAmount)

			if tt.expectError {
				if err == nil {
					t.Error("expected error but got nil")
				}
				return
			}

			if err != nil {
				t.Fatalf("unexpected error: %v", err)
			}

			if result != tt.expectedNet {
				t.Errorf("expected net %d, got %d", tt.expectedNet, result)
			}
		})
	}
}
