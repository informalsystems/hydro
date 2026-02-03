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

3. **Configure environment:**
```bash
cp deployments/.env.example deployments/.env
# Edit deployments/.env with your configuration (see Configuration section below)
```

4. **Run service:**
```bash
make run
# Or directly with environment:
source deployments/.env && go run cmd/server/main.go
```

## Local Testing Guide

### Step 1: Setup Database

```bash
# Start PostgreSQL with Docker
docker-compose -f deployments/docker-compose.yml up -d postgres

# Run migrations
make migrate
```

### Step 2: Configure Environment

Copy `.env.example` to `.env` and configure the following:

**Required for local testing:**
```bash
# Database (defaults work with docker-compose)
DB_HOST=localhost
DB_PORT=5432
DB_USER=postgres
DB_PASSWORD=postgres
DB_NAME=inflow_service

# Ethereum RPC (use public endpoint for testing)
ETH_RPC_ENDPOINT=https://ethereum-rpc.publicnode.com

# Your operator wallet (MUST have ETH for gas and be same address used for CCTP relay)
OPERATOR_EVM_PRIVATE_KEY=0x...your_private_key...
ETH_OPERATOR_ADDRESS=0x...your_operator_address...

# Neutron configuration (use mainnet for real testing)
NEUTRON_RPC_ENDPOINT=https://rpc-kralum.neutron-1.neutron.org:443
OPERATOR_NEUTRON_MNEMONIC="your 24 word mnemonic..."
OPERATOR_NEUTRON_ADDRESS=neutron1...your_address...

# Fee recipient (where operational fees go)
OPERATOR_FEE_RECIPIENT=0x...your_fee_address...
OPERATOR_ADMIN_ADDRESS=0x...your_admin_address...
```

### Step 3: Start the Service

```bash
# Load environment and run
source deployments/.env && go run cmd/server/main.go
```

You should see logs like:
```
{"level":"info","msg":"Starting server on port 8080"}
{"level":"info","msg":"Starting balance monitor worker"}
{"level":"info","msg":"Starting process executor worker"}
```

### Step 4: Test API Endpoints

**Health check:**
```bash
curl http://localhost:8080/health
# {"status":"ok","version":"1.0.0"}
```

**Get contract addresses for a user:**
```bash
curl -X POST http://localhost:8080/api/v1/contracts/addresses \
  -H "Content-Type: application/json" \
  -d '{"email":"test@example.com","chain_ids":["1"]}'
```

Response:
```json
{
  "email": "test@example.com",
  "contracts": {
    "1": {
      "forwarder": "0x...",
      "proxy": "neutron1..."
    }
  }
}
```

**Calculate fees:**
```bash
curl -X POST http://localhost:8080/api/v1/fees/calculate \
  -H "Content-Type: application/json" \
  -d '{"chain_id":"1","amount_usdc":"1000000"}'
```

**Get user processes:**
```bash
curl http://localhost:8080/api/v1/processes/user/test@example.com
```

### Step 5: Test Full Flow

1. **Get forwarder address:**
```bash
curl -X POST http://localhost:8080/api/v1/contracts/addresses \
  -H "Content-Type: application/json" \
  -d '{"email":"myemail@example.com","chain_ids":["1"]}'
```

2. **Send USDC to the forwarder address** from your CEX or wallet

3. **Monitor the process:**
```bash
# Watch logs
tail -f /var/log/offchain.log

# Or check process status
curl http://localhost:8080/api/v1/processes/user/myemail@example.com
```

4. **Process states:**
   - `PENDING_FUNDS` - Waiting for sufficient balance
   - `TRANSFER_IN_PROGRESS` - Bridge called, funds moving EVM → Noble → Neutron
   - `DEPOSIT_IN_PROGRESS` - ForwardToInflow called on Neutron
   - `DEPOSIT_DONE` - Complete!

### Database Inspection

```bash
# Connect to database
PGPASSWORD=postgres psql -h localhost -U postgres -d inflow_service

# View users
SELECT * FROM users;

# View contracts
SELECT * FROM contracts;

# View processes
SELECT id, process_id, user_email, status, amount_usdc, error_message FROM processes;

# Clear stuck process
DELETE FROM processes WHERE id = <id>;
```

## API Endpoints

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/health` | Health check |
| POST | `/api/v1/contracts/addresses` | Get/create contract addresses for user |
| POST | `/api/v1/fees/calculate` | Calculate bridge fees |
| GET | `/api/v1/processes/status/:processId` | Get process status |
| GET | `/api/v1/processes/user/:email` | List user's processes |

## Architecture

See [docs/offchain-service/implementation-plan.md](../docs/offchain-service/implementation-plan.md) for complete architecture documentation.

### Folder Structure

```
offchain/
├── cmd/server/              # Application entry point
├── internal/
│   ├── api/                 # HTTP handlers & routing
│   ├── blockchain/          # Blockchain clients (EVM & Cosmos)
│   │   ├── evm/             # Ethereum client, CREATE2, forwarder
│   │   └── cosmos/          # Neutron client, Noble RPC queries
│   ├── config/              # Configuration management
│   ├── database/            # Database connection & queries
│   ├── models/              # Data models
│   ├── service/             # Business logic
│   └── worker/              # Background workers (monitor, executor)
├── deployments/             # Docker & deployment files
└── Makefile                 # Build automation
```

## Configuration Reference

### Required Variables

| Variable | Description | Example |
|----------|-------------|---------|
| `ETH_RPC_ENDPOINT` | Ethereum RPC URL | `https://ethereum-rpc.publicnode.com` |
| `ETH_CCTP_CONTRACT` | Skip CCTP bridge contract | `0xBC8552339dA68EB65C8b88B414B5854E0E366cFc` |
| `ETH_OPERATOR_ADDRESS` | Operator wallet address | `0x...` |
| `ETH_FORWARDER_BYTECODE` | Compiled forwarder init code | `0x6101a0...` |
| `NEUTRON_RPC_ENDPOINT` | Neutron RPC URL | `https://rpc-kralum.neutron-1.neutron.org:443` |
| `NEUTRON_CONTROL_CENTERS` | Inflow control center addresses | `neutron1...,neutron1...` |
| `NEUTRON_ADMINS` | Proxy admin addresses | `neutron1...` |
| `NEUTRON_PROXY_CODE_ID` | Deployed proxy contract code ID | `5081` |
| `NOBLE_RPC_ENDPOINT` | Noble RPC for forwarding queries | `https://noble-rpc.polkachu.com` |
| `CCTP_DESTINATION_DOMAIN` | Noble CCTP domain | `4` |
| `CCTP_DESTINATION_CALLER` | Skip relayer address (bytes32) | `000000...` |
| `OPERATOR_EVM_PRIVATE_KEY` | Operator EVM private key | `0x...` |
| `OPERATOR_NEUTRON_MNEMONIC` | Operator Neutron mnemonic | `word1 word2...` |

### Optional Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `SERVER_PORT` | `8080` | HTTP server port |
| `ETH_OPERATIONAL_FEE_BPS` | `50` | Fee in basis points (50 = 0.5%) |
| `ETH_MIN_OPERATIONAL_FEE` | `10` | Minimum fee in USDC base units |
| `ETH_MIN_DEPOSIT` | `50000` | Minimum deposit in USDC base units |
| `NOBLE_NEUTRON_CHANNEL` | `channel-18` | IBC channel Noble→Neutron |

## Development

### Running Tests

```bash
make test                    # All tests
make test-create2           # CREATE2 tests only

# Integration test for Noble RPC
INTEGRATION_TEST=1 go test -v -run TestQueryForwardingAddressIntegration ./internal/blockchain/cosmos/
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

## Background Workers

The service runs two background workers:

### Monitor (every 30s)
- Scans all forwarder addresses for new deposits
- Checks process states and triggers transitions
- Detects when funds arrive at proxy on Neutron

### Executor
- Handles state transitions triggered by monitor
- Deploys contracts when needed (forwarder on EVM, proxy on Neutron)
- Calls `bridge()` on forwarder
- Calls `ForwardToInflow` on proxy
- Implements exponential backoff retry (max 3 retries)

## Troubleshooting

### Process stuck in TRANSFER_IN_PROGRESS

The bridge process takes 5-15 minutes. If stuck longer:
1. Check the bridge tx on Etherscan
2. Verify CCTP attestation at https://iris-api.circle.com/attestations/{txHash}
3. Check Noble explorer for IBC transfer
4. Manually update or delete the process if needed

### Noble forwarding address mismatch

The service queries Noble RPC for the correct forwarding address. Verify with:
```bash
nobled q forwarding address channel-18 <neutron_proxy_address> --node https://noble-rpc.polkachu.com/
```

### Insufficient balance errors

Ensure the forwarder has enough USDC to cover:
- Transfer amount
- Operational fee (0.5% or min fee)
- Smart relay fee (100,000 = 0.1 USDC)

## License

Same as parent Hydro project
