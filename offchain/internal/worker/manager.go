package worker

import (
	"context"
	"fmt"
	"sync"
	"time"

	"go.uber.org/zap"

	"hydro/offchain/internal/blockchain/cosmos"
	"hydro/offchain/internal/blockchain/evm"
	"hydro/offchain/internal/config"
	"hydro/offchain/internal/database"
	"hydro/offchain/internal/service"
)

// Constants for worker configuration
const (
	DefaultPollInterval = 30 * time.Second
	MaxRetries          = 3
	BaseRetryDelay      = 5 * time.Second
	BridgeTimeout       = 5 * time.Minute
	DepositTimeout      = 2 * time.Minute
	DeploymentTimeout   = 3 * time.Minute
	MonitorTimeout      = 30 * time.Second
)

// WorkerManager orchestrates background workers for deposit processing
type WorkerManager struct {
	db     *database.DB
	cfg    *config.Config
	logger *zap.Logger

	// Blockchain clients
	evmClients   map[string]*evm.Client    // chainID -> client
	forwarders   map[string]*evm.Forwarder // chainID -> forwarder
	cosmosClient *cosmos.Client
	proxy        *cosmos.Proxy

	// Services
	processService  *service.ProcessService
	feeService      *service.FeeService
	contractService *service.ContractService

	// Worker components
	monitor  *Monitor
	executor *Executor

	// Control
	ctx    context.Context
	cancel context.CancelFunc
	wg     sync.WaitGroup
}

// NewWorkerManager creates a new worker manager with all required dependencies
func NewWorkerManager(
	db *database.DB,
	cfg *config.Config,
	contractService *service.ContractService,
	feeService *service.FeeService,
	logger *zap.Logger,
) (*WorkerManager, error) {
	logger = logger.Named("worker")

	// Initialize EVM clients for each configured chain
	evmClients := make(map[string]*evm.Client)
	forwarders := make(map[string]*evm.Forwarder)

	for chainID, chainCfg := range cfg.Chains {
		chainCfgCopy := chainCfg // Create copy for pointer

		client, err := evm.NewClient(&chainCfgCopy, cfg.Operator.EVMPrivateKey, logger)
		if err != nil {
			// Close already-created clients
			for _, c := range evmClients {
				c.Close()
			}
			return nil, fmt.Errorf("failed to create EVM client for chain %s: %w", chainID, err)
		}
		evmClients[chainID] = client

		forwarder, err := evm.NewForwarder(client, &chainCfgCopy, logger)
		if err != nil {
			// Close all clients
			for _, c := range evmClients {
				c.Close()
			}
			return nil, fmt.Errorf("failed to create forwarder for chain %s: %w", chainID, err)
		}
		forwarders[chainID] = forwarder

		logger.Info("EVM chain initialized",
			zap.String("chain_id", chainID),
			zap.String("chain_name", chainCfg.Name))
	}

	// Initialize Cosmos client
	cosmosClient, err := cosmos.NewClient(&cfg.Neutron, cfg.Operator.NeutronMnemonic, logger)
	if err != nil {
		// Close EVM clients
		for _, c := range evmClients {
			c.Close()
		}
		return nil, fmt.Errorf("failed to create Cosmos client: %w", err)
	}

	// Create proxy handler
	proxy := cosmos.NewProxy(cosmosClient, &cfg.Neutron, logger)

	// Create process service
	processService := service.NewProcessService(db, cfg, logger)

	// Create context with cancellation
	ctx, cancel := context.WithCancel(context.Background())

	wm := &WorkerManager{
		db:              db,
		cfg:             cfg,
		logger:          logger,
		evmClients:      evmClients,
		forwarders:      forwarders,
		cosmosClient:    cosmosClient,
		proxy:           proxy,
		processService:  processService,
		feeService:      feeService,
		contractService: contractService,
		ctx:             ctx,
		cancel:          cancel,
	}

	// Create monitor and executor
	wm.monitor = NewMonitor(wm)
	wm.executor = NewExecutor(wm)

	return wm, nil
}

// Start starts all worker goroutines
func (wm *WorkerManager) Start() {
	wm.logger.Info("Starting worker manager",
		zap.Int("num_evm_chains", len(wm.evmClients)),
		zap.Duration("poll_interval", DefaultPollInterval))

	// Start monitor goroutine
	wm.wg.Add(1)
	go func() {
		defer wm.wg.Done()
		wm.monitor.Run(wm.ctx)
	}()

	// Start executor goroutine
	wm.wg.Add(1)
	go func() {
		defer wm.wg.Done()
		wm.executor.Run(wm.ctx)
	}()

	wm.logger.Info("Worker manager started")
}

// Shutdown gracefully stops all workers
func (wm *WorkerManager) Shutdown(timeout time.Duration) error {
	wm.logger.Info("Shutting down worker manager")

	// Signal workers to stop
	wm.cancel()

	// Wait for workers to finish with timeout
	done := make(chan struct{})
	go func() {
		wm.wg.Wait()
		close(done)
	}()

	select {
	case <-done:
		wm.logger.Info("Workers stopped gracefully")
	case <-time.After(timeout):
		wm.logger.Warn("Worker shutdown timed out")
	}

	// Close blockchain clients
	for chainID, client := range wm.evmClients {
		client.Close()
		wm.logger.Debug("Closed EVM client", zap.String("chain_id", chainID))
	}

	if err := wm.cosmosClient.Close(); err != nil {
		wm.logger.Error("Error closing Cosmos client", zap.Error(err))
	}

	wm.logger.Info("Worker manager shutdown complete")
	return nil
}

// GetChainConfig returns the chain configuration for a given chain ID
func (wm *WorkerManager) GetChainConfig(chainID string) (*config.ChainConfig, bool) {
	cfg, ok := wm.cfg.Chains[chainID]
	if !ok {
		return nil, false
	}
	return &cfg, true
}
