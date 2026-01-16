package main

import (
	"context"
	"log"
	"os"
	"os/signal"
	"syscall"
	"time"

	"hydro/offchain/internal/config"
	"hydro/offchain/internal/database"

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

	// TODO: Initialize API server (Phase 2)
	// TODO: Initialize blockchain clients (Phase 3)
	// TODO: Initialize workers (Phase 4)

	logger.Info("Service initialized successfully",
		zap.String("status", "ready"),
		zap.String("phase", "foundation_complete"))

	// Graceful shutdown
	quit := make(chan os.Signal, 1)
	signal.Notify(quit, syscall.SIGINT, syscall.SIGTERM)

	logger.Info("Service is running. Press Ctrl+C to stop.")

	// Wait for interrupt signal
	<-quit

	logger.Info("Shutting down service...")

	// Cleanup
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	// TODO: Shutdown API server (Phase 2)
	// TODO: Shutdown workers (Phase 4)

	select {
	case <-ctx.Done():
		logger.Warn("Shutdown timeout exceeded")
	default:
		logger.Info("Service stopped successfully")
	}
}

func initLogger() (*zap.Logger, error) {
	env := os.Getenv("ENV")
	if env == "production" {
		return zap.NewProduction()
	}
	return zap.NewDevelopment()
}
