package worker

import (
	"context"
	"time"

	"cosmossdk.io/math"
	"github.com/ethereum/go-ethereum/common"
	"go.uber.org/zap"

	"hydro/offchain/internal/models"
)

// Monitor polls blockchain for balance changes and detects funds arrival
type Monitor struct {
	manager *WorkerManager
	logger  *zap.Logger

	// Channel to send processes ready for execution
	readyProcesses chan *models.Process
}

// NewMonitor creates a new balance monitor
func NewMonitor(manager *WorkerManager) *Monitor {
	return &Monitor{
		manager:        manager,
		logger:         manager.logger.Named("monitor"),
		readyProcesses: make(chan *models.Process, 100),
	}
}

// Run starts the monitor polling loop
func (m *Monitor) Run(ctx context.Context) {
	m.logger.Info("Monitor started",
		zap.Duration("poll_interval", DefaultPollInterval))

	ticker := time.NewTicker(DefaultPollInterval)
	defer ticker.Stop()

	// Initial poll
	m.poll(ctx)

	for {
		select {
		case <-ctx.Done():
			m.logger.Info("Monitor stopping")
			close(m.readyProcesses)
			return
		case <-ticker.C:
			m.poll(ctx)
		}
	}
}

// poll executes one polling cycle
func (m *Monitor) poll(ctx context.Context) {
	pollCtx, cancel := context.WithTimeout(ctx, MonitorTimeout)
	defer cancel()

	m.logger.Debug("Starting poll cycle")

	// First, detect new deposits on forwarder addresses without active processes
	m.detectNewDeposits(pollCtx)

	// Then check processes in each state
	m.checkPendingFunds(pollCtx)
	m.checkTransferInProgress(pollCtx)
	m.checkDepositInProgress(pollCtx)
}

// detectNewDeposits scans all forwarder addresses for new deposits
// and creates Process records when funds are detected
func (m *Monitor) detectNewDeposits(ctx context.Context) {
	// Check each configured chain
	for chainID := range m.manager.cfg.Chains {
		select {
		case <-ctx.Done():
			return
		default:
		}

		m.detectNewDepositsForChain(ctx, chainID)
	}
}

// detectNewDepositsForChain checks all forwarder addresses on a specific chain
func (m *Monitor) detectNewDepositsForChain(ctx context.Context, chainID string) {
	// Get all forwarder contracts for this chain
	contracts, err := m.manager.db.GetAllForwarderContracts(ctx, chainID)
	if err != nil {
		m.logger.Error("Failed to get forwarder contracts",
			zap.String("chain_id", chainID),
			zap.Error(err))
		return
	}

	if len(contracts) == 0 {
		return
	}

	forwarder, ok := m.manager.forwarders[chainID]
	if !ok {
		m.logger.Error("No forwarder client for chain", zap.String("chain_id", chainID))
		return
	}

	m.logger.Debug("Scanning forwarder addresses for new deposits",
		zap.String("chain_id", chainID),
		zap.Int("count", len(contracts)))

	for _, contract := range contracts {
		select {
		case <-ctx.Done():
			return
		default:
		}

		// Check if there's already an active process for this forwarder
		hasActive, err := m.manager.db.HasActiveProcess(ctx, contract.Address)
		if err != nil {
			m.logger.Error("Failed to check active process",
				zap.String("address", contract.Address),
				zap.Error(err))
			continue
		}

		if hasActive {
			// Already has an active process, skip
			continue
		}

		// Check balance on forwarder
		forwarderAddr := common.HexToAddress(contract.Address)
		balance, err := forwarder.GetForwarderBalance(ctx, forwarderAddr)
		if err != nil {
			m.logger.Error("Failed to get forwarder balance",
				zap.String("address", contract.Address),
				zap.Error(err))
			continue
		}

		// If balance > 0, create a new process
		if balance.Sign() > 0 {
			m.logger.Info("New deposit detected, creating process",
				zap.String("user_email", contract.UserEmail),
				zap.String("chain_id", chainID),
				zap.String("forwarder", contract.Address),
				zap.String("balance", balance.String()))

			// Get proxy address for this user
			proxyAddress, err := m.manager.db.GetProxyAddressForUser(ctx, contract.UserEmail)
			if err != nil || proxyAddress == "" {
				m.logger.Error("Failed to get proxy address for user",
					zap.String("user_email", contract.UserEmail),
					zap.Error(err))
				continue
			}

			// Create new process
			process, err := m.manager.processService.CreateProcess(
				ctx,
				contract.UserEmail,
				chainID,
				contract.Address,
				proxyAddress,
			)
			if err != nil {
				m.logger.Error("Failed to create process",
					zap.String("user_email", contract.UserEmail),
					zap.Error(err))
				continue
			}

			m.logger.Info("Process created for new deposit",
				zap.String("process_id", process.ProcessID),
				zap.String("forwarder", contract.Address))
		}
	}
}

// checkPendingFunds monitors forwarder balances for PENDING_FUNDS processes
func (m *Monitor) checkPendingFunds(ctx context.Context) {
	processes, err := m.manager.db.GetProcessesByStatus(ctx, models.ProcessStatusPendingFunds)
	if err != nil {
		m.logger.Error("Failed to get pending funds processes", zap.Error(err))
		return
	}

	if len(processes) == 0 {
		return
	}

	m.logger.Debug("Checking pending funds processes", zap.Int("count", len(processes)))

	// Group by chain for efficient processing
	byChain := make(map[string][]*models.Process)
	for i := range processes {
		chainID := processes[i].ChainID
		byChain[chainID] = append(byChain[chainID], &processes[i])
	}

	// Check each chain
	for chainID, chainProcesses := range byChain {
		forwarder, ok := m.manager.forwarders[chainID]
		if !ok {
			m.logger.Error("No forwarder for chain", zap.String("chain_id", chainID))
			continue
		}

		chainCfg, ok := m.manager.GetChainConfig(chainID)
		if !ok {
			m.logger.Error("No config for chain", zap.String("chain_id", chainID))
			continue
		}

		minDeposit := chainCfg.MinDepositAmount

		for _, proc := range chainProcesses {
			select {
			case <-ctx.Done():
				return
			default:
			}

			// Check balance
			forwarderAddr := common.HexToAddress(proc.ForwarderAddress)
			balance, err := forwarder.GetForwarderBalance(ctx, forwarderAddr)
			if err != nil {
				m.logger.Error("Failed to get forwarder balance",
					zap.String("process_id", proc.ProcessID),
					zap.Error(err))
				continue
			}

			balanceInt64 := balance.Int64()

			// Check if balance meets minimum
			if balanceInt64 >= minDeposit {
				m.logger.Info("Funds detected, triggering execution",
					zap.String("process_id", proc.ProcessID),
					zap.Int64("balance", balanceInt64),
					zap.Int64("min_deposit", minDeposit))

				// Update amount in database
				if err := m.manager.db.UpdateProcessAmount(ctx, proc.ID, balanceInt64); err != nil {
					m.logger.Error("Failed to update process amount", zap.Error(err))
					continue
				}
				proc.AmountUSDC = &balanceInt64

				// Send to executor
				select {
				case m.readyProcesses <- proc:
				case <-ctx.Done():
					return
				default:
					m.logger.Warn("Executor channel full, skipping process",
						zap.String("process_id", proc.ProcessID))
				}
			}
		}
	}
}

// checkTransferInProgress monitors proxy balances for funds arrival from bridge
func (m *Monitor) checkTransferInProgress(ctx context.Context) {
	processes, err := m.manager.db.GetProcessesByStatus(ctx, models.ProcessStatusTransferInProgress)
	if err != nil {
		m.logger.Error("Failed to get transfer in progress processes", zap.Error(err))
		return
	}

	if len(processes) == 0 {
		return
	}

	m.logger.Debug("Checking transfer in progress processes", zap.Int("count", len(processes)))

	for i := range processes {
		proc := &processes[i]

		select {
		case <-ctx.Done():
			return
		default:
		}

		// Check proxy balance on Neutron
		balance, err := m.manager.proxy.GetProxyUSDCBalance(ctx, proc.ProxyAddress)
		if err != nil {
			m.logger.Error("Failed to get proxy balance",
				zap.String("process_id", proc.ProcessID),
				zap.Error(err))
			continue
		}

		// If balance > 0, funds have arrived
		if balance.GT(math.ZeroInt()) {
			m.logger.Info("Funds arrived at proxy",
				zap.String("process_id", proc.ProcessID),
				zap.String("balance", balance.String()))

			// Signal executor to start deposit
			select {
			case m.readyProcesses <- proc:
			case <-ctx.Done():
				return
			default:
				m.logger.Warn("Executor channel full, skipping process",
					zap.String("process_id", proc.ProcessID))
			}
		}
	}
}

// checkDepositInProgress verifies deposit transaction confirmations
func (m *Monitor) checkDepositInProgress(ctx context.Context) {
	processes, err := m.manager.db.GetProcessesByStatus(ctx, models.ProcessStatusDepositInProgress)
	if err != nil {
		m.logger.Error("Failed to get deposit in progress processes", zap.Error(err))
		return
	}

	if len(processes) == 0 {
		return
	}

	m.logger.Debug("Checking deposit in progress processes", zap.Int("count", len(processes)))

	for i := range processes {
		proc := &processes[i]

		select {
		case <-ctx.Done():
			return
		default:
		}

		// Check if deposit tx exists and is confirmed
		if proc.DepositTxHash == nil {
			m.logger.Warn("Deposit in progress but no tx hash",
				zap.String("process_id", proc.ProcessID))
			continue
		}

		confirmed, err := m.manager.cosmosClient.GetTxStatus(ctx, *proc.DepositTxHash)
		if err != nil {
			m.logger.Debug("Deposit tx not confirmed yet",
				zap.String("process_id", proc.ProcessID),
				zap.String("tx_hash", *proc.DepositTxHash))
			continue
		}

		if confirmed {
			// Mark as complete
			if err := m.manager.db.UpdateProcessStatus(ctx, proc.ID, models.ProcessStatusDepositDone); err != nil {
				m.logger.Error("Failed to update process status", zap.Error(err))
				continue
			}

			m.logger.Info("Deposit completed",
				zap.String("process_id", proc.ProcessID),
				zap.String("tx_hash", *proc.DepositTxHash))
		}
	}
}
