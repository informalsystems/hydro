package service

import (
	"context"
	"fmt"

	"go.uber.org/zap"

	"hydro/offchain/internal/config"
	"hydro/offchain/internal/database"
	"hydro/offchain/internal/models"
)

// ProcessService handles process lifecycle management
type ProcessService struct {
	db     *database.DB
	cfg    *config.Config
	logger *zap.Logger
}

// NewProcessService creates a new process service
func NewProcessService(db *database.DB, cfg *config.Config, logger *zap.Logger) *ProcessService {
	return &ProcessService{
		db:     db,
		cfg:    cfg,
		logger: logger,
	}
}

// CreateProcess creates a new process for a deposit operation
func (s *ProcessService) CreateProcess(
	ctx context.Context,
	userEmail string,
	chainID string,
	forwarderAddress string,
	proxyAddress string,
) (*models.Process, error) {
	// Generate process ID
	processID, err := s.db.GenerateProcessID(ctx, userEmail, chainID)
	if err != nil {
		return nil, fmt.Errorf("failed to generate process ID: %w", err)
	}

	process := &models.Process{
		ProcessID:        processID,
		UserEmail:        userEmail,
		ChainID:          chainID,
		ForwarderAddress: forwarderAddress,
		ProxyAddress:     proxyAddress,
		Status:           models.ProcessStatusPendingFunds,
		RetryCount:       0,
	}

	if err := s.db.CreateProcess(ctx, process); err != nil {
		return nil, fmt.Errorf("failed to create process: %w", err)
	}

	s.logger.Info("Process created",
		zap.String("process_id", processID),
		zap.String("user_email", userEmail),
		zap.String("chain_id", chainID))

	return process, nil
}

// GetActiveProcess gets an active (non-terminal) process for a user/chain if one exists
func (s *ProcessService) GetActiveProcess(
	ctx context.Context,
	userEmail string,
	chainID string,
) (*models.Process, error) {
	// Get all processes for this user
	processes, err := s.db.GetProcessesByUser(ctx, userEmail, 100, 0)
	if err != nil {
		return nil, fmt.Errorf("failed to get user processes: %w", err)
	}

	// Look for an active process on this chain
	for i := range processes {
		if processes[i].ChainID == chainID &&
			processes[i].Status != models.ProcessStatusDepositDone &&
			processes[i].Status != models.ProcessStatusFailed {
			return &processes[i], nil
		}
	}

	return nil, nil
}

// GetOrCreateActiveProcess gets an active (non-terminal) process for a user/chain
// or creates a new one if none exists
func (s *ProcessService) GetOrCreateActiveProcess(
	ctx context.Context,
	userEmail string,
	chainID string,
	forwarderAddress string,
	proxyAddress string,
) (*models.Process, error) {
	// Check for existing active process
	process, err := s.GetActiveProcess(ctx, userEmail, chainID)
	if err != nil {
		return nil, err
	}

	if process != nil {
		return process, nil
	}

	// No active process, create new one
	return s.CreateProcess(ctx, userEmail, chainID, forwarderAddress, proxyAddress)
}

// GetProcessByID retrieves a process by its database ID
func (s *ProcessService) GetProcessByID(ctx context.Context, id int64) (*models.Process, error) {
	return s.db.GetProcess(ctx, id)
}

// GetProcessByProcessID retrieves a process by its process_id string
func (s *ProcessService) GetProcessByProcessID(ctx context.Context, processID string) (*models.Process, error) {
	return s.db.GetProcessByProcessID(ctx, processID)
}

// GetProcessesByStatus retrieves all processes with a given status
func (s *ProcessService) GetProcessesByStatus(ctx context.Context, status models.ProcessStatus) ([]models.Process, error) {
	return s.db.GetProcessesByStatus(ctx, status)
}

// UpdateStatus updates the status of a process
func (s *ProcessService) UpdateStatus(ctx context.Context, id int64, status models.ProcessStatus) error {
	if err := s.db.UpdateProcessStatus(ctx, id, status); err != nil {
		return fmt.Errorf("failed to update process status: %w", err)
	}

	s.logger.Debug("Process status updated",
		zap.Int64("id", id),
		zap.String("status", string(status)))

	return nil
}

// RecordBridgeTx records the bridge transaction hash and updates status
func (s *ProcessService) RecordBridgeTx(ctx context.Context, id int64, txHash string) error {
	if err := s.db.UpdateProcessBridgeTx(ctx, id, models.ProcessStatusTransferInProgress, txHash); err != nil {
		return fmt.Errorf("failed to record bridge tx: %w", err)
	}

	s.logger.Info("Bridge transaction recorded",
		zap.Int64("id", id),
		zap.String("tx_hash", txHash))

	return nil
}

// RecordDepositTx records the deposit transaction hash and updates status
func (s *ProcessService) RecordDepositTx(ctx context.Context, id int64, txHash string) error {
	if err := s.db.UpdateProcessDepositTx(ctx, id, models.ProcessStatusDepositDone, txHash); err != nil {
		return fmt.Errorf("failed to record deposit tx: %w", err)
	}

	s.logger.Info("Deposit transaction recorded",
		zap.Int64("id", id),
		zap.String("tx_hash", txHash))

	return nil
}

// RecordError records an error and increments the retry count
func (s *ProcessService) RecordError(ctx context.Context, id int64, errorMsg string) error {
	if err := s.db.UpdateProcessError(ctx, id, errorMsg); err != nil {
		return fmt.Errorf("failed to record error: %w", err)
	}

	s.logger.Warn("Process error recorded",
		zap.Int64("id", id),
		zap.String("error", errorMsg))

	return nil
}

// MarkFailed marks a process as permanently failed
func (s *ProcessService) MarkFailed(ctx context.Context, id int64, reason string) error {
	// Record the error first
	if err := s.db.UpdateProcessError(ctx, id, reason); err != nil {
		return fmt.Errorf("failed to record error: %w", err)
	}

	// Update status to FAILED
	if err := s.db.UpdateProcessStatus(ctx, id, models.ProcessStatusFailed); err != nil {
		return fmt.Errorf("failed to mark as failed: %w", err)
	}

	s.logger.Error("Process marked as failed",
		zap.Int64("id", id),
		zap.String("reason", reason))

	return nil
}

// UpdateAmount updates the USDC amount for a process
func (s *ProcessService) UpdateAmount(ctx context.Context, id int64, amountUSDC int64) error {
	if err := s.db.UpdateProcessAmount(ctx, id, amountUSDC); err != nil {
		return fmt.Errorf("failed to update amount: %w", err)
	}

	s.logger.Debug("Process amount updated",
		zap.Int64("id", id),
		zap.Int64("amount_usdc", amountUSDC))

	return nil
}
