package worker

import (
	"context"
	"fmt"
	"math/big"
	"time"

	"github.com/ethereum/go-ethereum/common"
	"go.uber.org/zap"

	"hydro/offchain/internal/blockchain/cosmos"
	"hydro/offchain/internal/blockchain/evm"
	"hydro/offchain/internal/config"
	"hydro/offchain/internal/models"
)

// Executor handles state transitions for deposit processes
type Executor struct {
	manager *WorkerManager
	logger  *zap.Logger
}

// NewExecutor creates a new process executor
func NewExecutor(manager *WorkerManager) *Executor {
	return &Executor{
		manager: manager,
		logger:  manager.logger.Named("executor"),
	}
}

// Run starts the executor loop
func (e *Executor) Run(ctx context.Context) {
	e.logger.Info("Executor started")

	for {
		select {
		case <-ctx.Done():
			e.logger.Info("Executor stopping")
			return
		case proc, ok := <-e.manager.monitor.readyProcesses:
			if !ok {
				e.logger.Info("Process channel closed, executor stopping")
				return
			}
			e.handleProcess(ctx, proc)
		}
	}
}

// handleProcess processes a single process based on its current status
func (e *Executor) handleProcess(ctx context.Context, proc *models.Process) {
	e.logger.Info("Handling process",
		zap.String("process_id", proc.ProcessID),
		zap.String("status", string(proc.Status)))

	var err error

	switch proc.Status {
	case models.ProcessStatusPendingFunds:
		err = e.executeBridge(ctx, proc)

	case models.ProcessStatusTransferInProgress:
		err = e.executeDeposit(ctx, proc)

	default:
		e.logger.Warn("Unexpected process status",
			zap.String("process_id", proc.ProcessID),
			zap.String("status", string(proc.Status)))
		return
	}

	if err != nil {
		e.handleError(ctx, proc, err)
	}
}

// executeBridge handles PENDING_FUNDS -> TRANSFER_IN_PROGRESS transition
func (e *Executor) executeBridge(ctx context.Context, proc *models.Process) error {
	e.logger.Info("Executing bridge",
		zap.String("process_id", proc.ProcessID),
		zap.String("chain_id", proc.ChainID))

	chainID := proc.ChainID
	forwarder, ok := e.manager.forwarders[chainID]
	if !ok {
		return fmt.Errorf("no forwarder for chain %s", chainID)
	}

	client, ok := e.manager.evmClients[chainID]
	if !ok {
		return fmt.Errorf("no EVM client for chain %s", chainID)
	}

	chainCfg, ok := e.manager.GetChainConfig(chainID)
	if !ok {
		return fmt.Errorf("no config for chain %s", chainID)
	}

	forwarderAddr := common.HexToAddress(proc.ForwarderAddress)

	// 1. Check if forwarder is deployed
	deployed, err := client.IsContractDeployed(ctx, forwarderAddr)
	if err != nil {
		return fmt.Errorf("failed to check forwarder deployment: %w", err)
	}

	if !deployed {
		if err := e.deployForwarder(ctx, proc, chainID, chainCfg); err != nil {
			return err
		}
	}

	// 2. Check if proxy is deployed on Neutron
	proxyDeployed, err := e.manager.proxy.IsProxyDeployed(ctx, proc.ProxyAddress)
	if err != nil {
		e.logger.Warn("Failed to check proxy deployment, assuming not deployed", zap.Error(err))
		proxyDeployed = false
	}

	if !proxyDeployed {
		if err := e.deployProxy(ctx, proc); err != nil {
			return err
		}
	}

	// 3. Calculate fees and bridge amount
	if proc.AmountUSDC == nil {
		return fmt.Errorf("process has no amount set")
	}
	amount := *proc.AmountUSDC

	feeCalc, err := e.manager.feeService.CalculateBridgeFee(chainID, amount)
	if err != nil {
		return fmt.Errorf("failed to calculate fees: %w", err)
	}

	// Smart relay fee estimate (CCTP relayer fee)
	smartRelayFee := int64(100000) // 0.1 USDC as relay fee estimate

	transferAmount := amount - feeCalc.BridgeFeeUSDC - smartRelayFee
	if transferAmount <= 0 {
		return fmt.Errorf("insufficient funds after fees: amount=%d, fee=%d, relay=%d",
			amount, feeCalc.BridgeFeeUSDC, smartRelayFee)
	}

	// 4. Call bridge on forwarder
	bridgeParams := evm.BridgeParams{
		TransferAmount:          big.NewInt(transferAmount),
		SmartRelayFeeAmount:     big.NewInt(smartRelayFee),
		OperationalFeeRecipient: common.HexToAddress(e.manager.cfg.Operator.FeeRecipient),
	}

	e.logger.Info("Calling bridge",
		zap.String("process_id", proc.ProcessID),
		zap.Int64("transfer_amount", transferAmount),
		zap.Int64("relay_fee", smartRelayFee),
		zap.Int64("operational_fee", feeCalc.BridgeFeeUSDC))

	bridgeCtx, cancel := context.WithTimeout(ctx, BridgeTimeout)
	defer cancel()

	receipt, err := forwarder.BridgeAndWait(bridgeCtx, forwarderAddr, bridgeParams, BridgeTimeout)
	if err != nil {
		return fmt.Errorf("bridge failed: %w", err)
	}

	// 5. Update status to TRANSFER_IN_PROGRESS
	txHash := receipt.TxHash.Hex()
	if err := e.manager.db.UpdateProcessBridgeTx(ctx, proc.ID, models.ProcessStatusTransferInProgress, txHash); err != nil {
		return fmt.Errorf("failed to update process status: %w", err)
	}

	e.logger.Info("Bridge initiated successfully",
		zap.String("process_id", proc.ProcessID),
		zap.String("tx_hash", txHash),
		zap.Int64("transfer_amount", transferAmount))

	return nil
}

// deployForwarder deploys the forwarder contract using CREATE2
func (e *Executor) deployForwarder(ctx context.Context, proc *models.Process, chainID string, chainCfg *config.ChainConfig) error {
	e.logger.Info("Deploying forwarder contract",
		zap.String("process_id", proc.ProcessID),
		zap.String("address", proc.ForwarderAddress))

	forwarder := e.manager.forwarders[chainID]

	constructorParams, err := evm.CreateConstructorParamsForUser(
		chainCfg,
		&e.manager.cfg.Neutron,
		&e.manager.cfg.Operator,
		proc.UserEmail,
		cosmos.ComputeNobleForwardingAddressForProxy,
		cosmos.ConvertToBytes32,
	)
	if err != nil {
		return fmt.Errorf("failed to create constructor params: %w", err)
	}

	deployCtx, cancel := context.WithTimeout(ctx, DeploymentTimeout)
	defer cancel()

	_, txHash, err := forwarder.DeployForwarderCREATE2AndWait(
		deployCtx,
		proc.UserEmail,
		chainID,
		constructorParams,
		DeploymentTimeout,
	)
	if err != nil {
		return fmt.Errorf("failed to deploy forwarder: %w", err)
	}

	// Mark contract as deployed in DB
	contract, _ := e.manager.db.GetContract(ctx, proc.UserEmail, chainID, models.ContractTypeForwarder)
	if contract != nil {
		if err := e.manager.db.UpdateContractDeployed(ctx, contract.ID, txHash.Hex()); err != nil {
			e.logger.Warn("Failed to update contract deployed status", zap.Error(err))
		}
	}

	e.logger.Info("Forwarder deployed",
		zap.String("process_id", proc.ProcessID),
		zap.String("tx_hash", txHash.Hex()))

	return nil
}

// deployProxy deploys the proxy contract on Neutron
func (e *Executor) deployProxy(ctx context.Context, proc *models.Process) error {
	e.logger.Info("Deploying proxy contract",
		zap.String("process_id", proc.ProcessID),
		zap.String("address", proc.ProxyAddress))

	txHash, contractAddr, err := e.manager.proxy.InstantiateProxy(ctx, proc.UserEmail)
	if err != nil {
		return fmt.Errorf("failed to deploy proxy: %w", err)
	}

	// Verify address matches
	if contractAddr != proc.ProxyAddress {
		return fmt.Errorf("proxy address mismatch: expected %s, got %s", proc.ProxyAddress, contractAddr)
	}

	// Mark contract as deployed in DB
	contract, _ := e.manager.db.GetContract(ctx, proc.UserEmail, "neutron", models.ContractTypeProxy)
	if contract != nil {
		if err := e.manager.db.UpdateContractDeployed(ctx, contract.ID, txHash); err != nil {
			e.logger.Warn("Failed to update contract deployed status", zap.Error(err))
		}
	}

	e.logger.Info("Proxy deployed",
		zap.String("process_id", proc.ProcessID),
		zap.String("tx_hash", txHash))

	return nil
}

// executeDeposit handles TRANSFER_IN_PROGRESS -> DEPOSIT_DONE transition
func (e *Executor) executeDeposit(ctx context.Context, proc *models.Process) error {
	e.logger.Info("Executing deposit",
		zap.String("process_id", proc.ProcessID))

	// Update status to DEPOSIT_IN_PROGRESS
	if err := e.manager.db.UpdateProcessStatus(ctx, proc.ID, models.ProcessStatusDepositInProgress); err != nil {
		return fmt.Errorf("failed to update status: %w", err)
	}

	// Call ForwardToInflow on proxy
	depositCtx, cancel := context.WithTimeout(ctx, DepositTimeout)
	defer cancel()

	txHash, err := e.manager.proxy.ForwardToInflowAndWait(depositCtx, proc.ProxyAddress, DepositTimeout)
	if err != nil {
		return fmt.Errorf("ForwardToInflow failed: %w", err)
	}

	// Update with deposit tx hash and mark as done
	if err := e.manager.db.UpdateProcessDepositTx(ctx, proc.ID, models.ProcessStatusDepositDone, txHash); err != nil {
		return fmt.Errorf("failed to update deposit tx: %w", err)
	}

	e.logger.Info("Deposit completed successfully",
		zap.String("process_id", proc.ProcessID),
		zap.String("tx_hash", txHash))

	return nil
}

// handleError handles execution errors with retry logic
func (e *Executor) handleError(ctx context.Context, proc *models.Process, execErr error) {
	e.logger.Error("Execution failed",
		zap.String("process_id", proc.ProcessID),
		zap.Int("retry_count", proc.RetryCount),
		zap.Error(execErr))

	// Record error and increment retry count
	if err := e.manager.db.UpdateProcessError(ctx, proc.ID, execErr.Error()); err != nil {
		e.logger.Error("Failed to update error", zap.Error(err))
	}

	// Get updated retry count
	updatedProc, err := e.manager.db.GetProcess(ctx, proc.ID)
	if err != nil {
		e.logger.Error("Failed to get updated process", zap.Error(err))
		return
	}

	// Check if max retries exceeded
	if updatedProc.RetryCount >= MaxRetries {
		e.logger.Error("Max retries exceeded, marking as failed",
			zap.String("process_id", proc.ProcessID),
			zap.Int("retry_count", updatedProc.RetryCount))

		if err := e.manager.db.UpdateProcessStatus(ctx, proc.ID, models.ProcessStatusFailed); err != nil {
			e.logger.Error("Failed to mark as failed", zap.Error(err))
		}
		return
	}

	// Calculate backoff delay for logging
	delay := BaseRetryDelay * time.Duration(1<<uint(updatedProc.RetryCount))
	e.logger.Info("Scheduled for retry",
		zap.String("process_id", proc.ProcessID),
		zap.Duration("backoff_delay", delay),
		zap.Int("attempt", updatedProc.RetryCount))

	// Process will be picked up again on next poll cycle
}
