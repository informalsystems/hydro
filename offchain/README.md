# Offchain Service for Inflow Deposits

Backend service for managing USDC deposits from CEX users into Hydro Inflow vaults.

## Quick Start

### Prerequisites
- Go 1.22+
- Docker & Docker Compose
- PostgreSQL 16 (or use Docker)

### Local Development

1. **Start database:**
```bash
make docker-up
```

2. **Run migrations:**
```bash
make migrate
```

3. **Run service:**
```bash
make run
```

### Environment Variables

Copy `.env.example` to `.env` and configure:

```bash
cp deployments/.env.example .env
# Edit .env with your configuration
```

## API Endpoints

- `GET /health` - Health check
- `POST /api/v1/contracts/addresses` - Get/create contract addresses for user
- `POST /api/v1/fees/calculate` - Calculate bridge fees
- `GET /api/v1/processes/status/:processId` - Get process status
- `GET /api/v1/processes/user/:email` - List user's processes

## Architecture

See [docs/offchain-service/implementation-plan.md](../docs/offchain-service/implementation-plan.md) for complete architecture documentation.

### Folder Structure

```
offchain/
├── cmd/server/              # Application entry point
├── internal/
│   ├── api/                 # HTTP handlers & routing
│   ├── blockchain/          # Blockchain clients (EVM & Cosmos)
│   ├── config/              # Configuration management
│   ├── database/            # Database connection & queries
│   ├── models/              # Data models
│   ├── service/             # Business logic
│   └── worker/              # Background workers
├── deployments/             # Docker & deployment files
└── Makefile                 # Build automation
```

## Development

### Running Tests

```bash
make test                    # All tests
make test-create2           # CREATE2 tests only
```

### Building

```bash
make build                  # Build binary to bin/server
```

### Docker

```bash
make docker-up              # Start services
make docker-down            # Stop services
make docker-logs            # View logs
```

## Process Flow

1. User calls `/contracts/addresses` to get forwarder address
2. User sends USDC to forwarder address from CEX
3. Service monitors balance and deploys contracts when sufficient
4. Service calls `bridge()` to initiate CCTP transfer
5. Service monitors proxy on Neutron for funds arrival
6. Service calls `ForwardToInflow` to deposit into vault
7. Vault shares minted to proxy contract

## Status States

- `PENDING_FUNDS` - Waiting for user to send sufficient funds
- `TRANSFER_IN_PROGRESS` - Bridge initiated, funds moving EVM → Neutron
- `DEPOSIT_IN_PROGRESS` - Calling ForwardToInflow on proxy
- `DEPOSIT_DONE` - Deposit complete

## Configuration

Key environment variables:

- `SERVER_PORT` - HTTP server port (default: 8080)
- `DB_HOST`, `DB_PORT`, `DB_USER`, `DB_PASSWORD`, `DB_NAME` - PostgreSQL connection
- `ETH_RPC_ENDPOINT`, `BASE_RPC_ENDPOINT` - EVM RPC endpoints
- `NEUTRON_RPC_ENDPOINT` - Neutron RPC endpoint
- `OPERATOR_EVM_PRIVATE_KEY` - Operator wallet private key (EVM)
- `OPERATOR_NEUTRON_MNEMONIC` - Operator wallet mnemonic (Neutron)

See `deployments/.env.example` for full configuration.

## License

Same as parent Hydro project
