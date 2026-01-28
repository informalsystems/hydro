package main

import (
	"context"
	"fmt"
	"log"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"

	"hydro/offchain/internal/api"
	"hydro/offchain/internal/config"
	"hydro/offchain/internal/database"
	"hydro/offchain/internal/service"
	"hydro/offchain/internal/worker"

	"go.uber.org/zap"
)

func main() {
	// Initialize logger
	logger, err := initLogger()
	if err != nil {
		log.Fatalf("Failed to initialize logger: %v", err)
	}
	defer logger.Sync()

	logger.Info("Starting Inflow Offchain Service")

	// Load configuration
	cfg, err := config.LoadConfig()
	if err != nil {
		logger.Fatal("Failed to load configuration", zap.Error(err))
	}

	logger.Info("Configuration loaded",
		zap.Int("server_port", cfg.Server.Port),
		zap.String("db_host", cfg.Database.Host),
		zap.Int("num_chains", len(cfg.Chains)))

	// Connect to database
	db, err := database.Connect(database.Config{
		Host:     cfg.Database.Host,
		Port:     cfg.Database.Port,
		User:     cfg.Database.User,
		Password: cfg.Database.Password,
		DBName:   cfg.Database.DBName,
		SSLMode:  cfg.Database.SSLMode,
	})
	if err != nil {
		logger.Fatal("Failed to connect to database", zap.Error(err))
	}
	defer db.Close()

	logger.Info("Database connected successfully")

	// Run migrations
	migrationPath := "internal/database/migrations/001_schema.sql"
	if err := database.RunMigrations(db, migrationPath); err != nil {
		logger.Warn("Failed to run migrations (may already be applied)", zap.Error(err))
	} else {
		logger.Info("Database migrations applied successfully")
	}

	// Test database connection with a simple query
	if err := db.Ping(); err != nil {
		logger.Fatal("Failed to ping database", zap.Error(err))
	}

	logger.Info("Database health check passed")

	// Initialize services (Phase 2)
	contractService := service.NewContractService(db, cfg, logger)
	feeService := service.NewFeeService(cfg, logger)

	logger.Info("Services initialized")

	// Initialize API handlers (Phase 2)
	apiHandler := api.NewHandler(db, contractService, feeService, logger)
	router := api.SetupRouter(apiHandler, logger)

	// Create HTTP server
	serverAddr := fmt.Sprintf(":%d", cfg.Server.Port)
	httpServer := &http.Server{
		Addr:         serverAddr,
		Handler:      router,
		ReadTimeout:  15 * time.Second,
		WriteTimeout: 15 * time.Second,
		IdleTimeout:  60 * time.Second,
	}

	// Start HTTP server in goroutine
	serverErrors := make(chan error, 1)
	go func() {
		logger.Info("Starting HTTP server",
			zap.String("addr", serverAddr))
		serverErrors <- httpServer.ListenAndServe()
	}()

	// Initialize workers (Phase 4)
	workerManager, err := worker.NewWorkerManager(db, cfg, contractService, feeService, logger)
	if err != nil {
		logger.Fatal("Failed to initialize worker manager", zap.Error(err))
	}

	// Start workers
	workerManager.Start()
	logger.Info("Workers started")

	logger.Info("Service initialized successfully",
		zap.String("status", "ready"),
		zap.String("phase", "workers_complete"),
		zap.Int("port", cfg.Server.Port))

	// Graceful shutdown
	quit := make(chan os.Signal, 1)
	signal.Notify(quit, syscall.SIGINT, syscall.SIGTERM)

	// Wait for interrupt signal or server error
	select {
	case err := <-serverErrors:
		logger.Fatal("HTTP server error", zap.Error(err))
	case sig := <-quit:
		logger.Info("Received shutdown signal", zap.String("signal", sig.String()))
	}

	logger.Info("Shutting down service...")

	// Shutdown HTTP server
	shutdownCtx, shutdownCancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer shutdownCancel()

	// Shutdown workers first
	if err := workerManager.Shutdown(10 * time.Second); err != nil {
		logger.Error("Worker shutdown error", zap.Error(err))
	}

	if err := httpServer.Shutdown(shutdownCtx); err != nil {
		logger.Error("HTTP server shutdown error", zap.Error(err))
		httpServer.Close()
	} else {
		logger.Info("HTTP server stopped gracefully")
	}

	logger.Info("Service stopped successfully")
}

func initLogger() (*zap.Logger, error) {
	env := os.Getenv("ENV")
	if env == "production" {
		return zap.NewProduction()
	}
	return zap.NewDevelopment()
}
