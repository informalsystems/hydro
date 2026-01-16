.PHONY: help build run test clean docker-up docker-down migrate

help: ## Show this help message
	@echo 'Usage: make [target]'
	@echo ''
	@echo 'Available targets:'
	@awk 'BEGIN {FS = ":.*?## "} /^[a-zA-Z_-]+:.*?## / {printf "  %-15s %s\n", $$1, $$2}' $(MAKEFILE_LIST)

build: ## Build the server binary
	go build -o bin/server ./cmd/server

run: ## Run the server locally
	go run ./cmd/server

test: ## Run tests
	go test -v ./...

test-create2: ## Run CREATE2 tests only
	go test -v ./internal/blockchain/evm -run TestComputeForwarderAddress

clean: ## Clean build artifacts
	rm -rf bin/

docker-up: ## Start docker-compose services
	docker-compose -f deployments/docker-compose.yml up -d

docker-down: ## Stop docker-compose services
	docker-compose -f deployments/docker-compose.yml down

docker-logs: ## View docker-compose logs
	docker-compose -f deployments/docker-compose.yml logs -f

migrate: ## Run database migrations
	@echo "Running migrations..."
	@PGPASSWORD=postgres psql -h localhost -U postgres -d inflow_service -f internal/database/migrations/001_schema.sql

migrate-docker: ## Run migrations on docker postgres
	docker-compose -f deployments/docker-compose.yml exec postgres psql -U postgres -d inflow_service -f /migrations/001_schema.sql

deps: ## Download dependencies
	go mod download
	go mod tidy

fmt: ## Format code
	go fmt ./...

lint: ## Run linter
	golangci-lint run ./...
