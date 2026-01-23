package api

import "hydro/offchain/internal/models"

// ==================== Contract Addresses ====================

// GetContractAddressesRequest represents request to get contract addresses
type GetContractAddressesRequest struct {
	Email    string   `json:"email"`
	ChainIDs []string `json:"chain_ids"`
}

// ChainContracts holds contract addresses for a specific chain
type ChainContracts struct {
	Forwarder string `json:"forwarder"`
	Proxy     string `json:"proxy"`
}

// GetContractAddressesResponse represents response with contract addresses
type GetContractAddressesResponse struct {
	Email     string                    `json:"email"`
	Contracts map[string]ChainContracts `json:"contracts"` // key: chain_id
}

// ==================== Fee Calculation ====================

// CalculateFeeRequest represents request to calculate bridge fee
type CalculateFeeRequest struct {
	ChainID    string `json:"chain_id"`
	AmountUSDC string `json:"amount_usdc"` // in base units (6 decimals)
}

// CalculateFeeResponse represents response with calculated fees
type CalculateFeeResponse struct {
	BridgeFeeUSDC  string `json:"bridge_fee_usdc"`  // in base units (6 decimals)
	MinDepositUSDC string `json:"min_deposit_usdc"` // in base units (6 decimals)
}

// ==================== Process Status ====================

// TxHashes holds transaction hashes for a process
type TxHashes struct {
	Bridge  *string `json:"bridge"`
	Deposit *string `json:"deposit"`
}

// GetProcessStatusResponse represents response with process status
type GetProcessStatusResponse struct {
	ProcessID string              `json:"process_id"`
	Status    models.ProcessStatus `json:"status"`
	AmountUSDC *string             `json:"amount_usdc"` // in base units (6 decimals)
	TxHashes  TxHashes            `json:"tx_hashes"`
	Error     *string             `json:"error,omitempty"`
}

// ==================== User Processes ====================

// ProcessSummary represents a summary of a process
type ProcessSummary struct {
	ProcessID        string              `json:"process_id"`
	ChainID          string              `json:"chain_id"`
	ForwarderAddress string              `json:"forwarder_address"`
	ProxyAddress     string              `json:"proxy_address"`
	Status           models.ProcessStatus `json:"status"`
	AmountUSDC       *string             `json:"amount_usdc"` // in base units (6 decimals)
	TxHashes         TxHashes            `json:"tx_hashes"`
	Error            *string             `json:"error,omitempty"`
}

// GetUserProcessesResponse represents response with user's processes
type GetUserProcessesResponse struct {
	Processes []ProcessSummary `json:"processes"`
}

// ==================== Error Response ====================

// ErrorResponse represents an API error response
type ErrorResponse struct {
	Error   string `json:"error"`
	Message string `json:"message,omitempty"`
}

// ==================== Health Check ====================

// HealthResponse represents health check response
type HealthResponse struct {
	Status  string `json:"status"`
	Version string `json:"version,omitempty"`
}
