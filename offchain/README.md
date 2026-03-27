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
# Edit ./internal/database/scripts/master.sql to configure supported EVM chains (See "Database CHAINS Table Entries Configuration Reference" section below)

make docker-up
```

2. **Initialize database:**
```bash
make db-init
```

3. **Configure environment:**
```bash
cp deployments/.env.example deployments/.env
# Edit deployments/.env with your configuration (see "Environment Variables Configuration Reference" section below)
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
# Edit ./internal/database/scripts/master.sql to configure supported EVM chains

# Start PostgreSQL with Docker
docker compose -f deployments/docker-compose.yml up -d postgres

# Create database
make db-init
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

# Neutron configuration
NEUTRON_RPC_ENDPOINT=https://rpc-kralum.neutron-1.neutron.org:443
NEUTRON_REST_ENDPOINT=https://neutron-rest.publicnode.com
NEUTRON_ADMINS=PROXY_CONTRACTS_ADMINS

# Noble configuration
NOBLE_RPC_ENDPOINT=https://noble-rpc.polkachu.com
NOBLE_REST_ENDPOINT=https://noble-api.polkachu.com

# Your EVM operator wallet (MUST have ETH for gas on all supported EVM chains)
OPERATOR_EVM_PRIVATE_KEY=0x...your_private_key...
# Your Neutron operator wallet mnemonic
OPERATOR_NEUTRON_MNEMONIC="your 24 word mnemonic..."
# Your Noble operator wallet mnemonic
OPERATOR_NOBLE_MNEMONIC="your 24 word mnemonic..."
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

# View supported EVM chains
SELECT * FROM chains;

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

## Environment Variables Configuration Reference

| Variable | Description | Example |
|----------|-------------|---------|
| `SERVER_PORT` | `8080` | HTTP server port |
| `ETH_FORWARDER_BYTECODE` | Compiled forwarder init code | `0x6101a0...` |
| `NEUTRON_RPC_ENDPOINT` | Neutron RPC URL | `https://rpc-kralum.neutron-1.neutron.org:443` |
| `NEUTRON_CONTROL_CENTERS` | Inflow control center addresses | `neutron1...,neutron1...` |
| `NEUTRON_ADMINS` | Proxy admin addresses | `neutron1...` |
| `NEUTRON_PROXY_CODE_ID` | Deployed proxy contract code ID | `5081` |
| `NOBLE_RPC_ENDPOINT` | Noble RPC for forwarding queries | `https://noble-rpc.polkachu.com` |
| `NOBLE_NEUTRON_CHANNEL` | `channel-18` | IBC transfer channel from Noble to Neutron |
| `CCTP_DESTINATION_DOMAIN` | Noble CCTP domain | `4` |
| `CCTP_DESTINATION_CALLER` | Skip relayer address (bytes32) | `000000000000000000000000691cf4641d5608f085b2c1921172120bb603d074` |
| `OPERATOR_EVM_PRIVATE_KEY` | Operator EVM private key | `0x...` |
| `OPERATOR_NEUTRON_MNEMONIC` | Operator Neutron mnemonic | `word1 word2...` |
| `OPERATOR_NOBLE_MNEMONIC` | Operator Noble mnemonic | `word1 word2...` |

## Database CHAINS Table Entries Configuration Reference

| Column | Description | Example |
|----------|---------|-------------|
| `rpc_endpoint` | EVM chain node RPC URL | `https://ethereum-rpc.publicnode.com` |
| `usdc_contract_address` | USDC EVM contract address | `0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48` |
| `cctp_contract_address` | Skip CCTP bridge contract address | `0xBC8552339dA68EB65C8b88B414B5854E0E366cFc` |
| `operational_fee_bps` | `50` | Operational fee for each transfer from EVM chain, expressed in basis points (1% = 100 bps) |
| `min_operational_fee` | `10000` | Minimum operational fee charged, for small transfers; expressed in uUSDC (1 USDC = 1,000,000 uUSDC) |
| `min_deposit_amount` | `50000` | Minimum deposit amount; If less than this value is sent to the Forwarder contract, transfer will not be performed; expressed in uUSDC |
| `forwarder_contract_admin` | Address allowed to pause Forwarder contract in case of emergency | `0x5FD9c2335B1247566f53f6304873dC3046Ef907a` |
| `fee_recipient` | Operational fee recipient on EVM chain | `0x5FD9c2335B1247566f53f6304873dC3046Ef907a` |

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
- Detects when funds arrive at Noble
- Registers Forwarding Account on Noble, which in turn sends tokens to proxy on Neutron via IBC

### Executor
- Handles state transitions triggered by monitor
- Deploys contracts when needed (forwarder on EVM, proxy on Neutron)
- Calls `bridge()` on forwarder
- Calls `ForwardToInflow` on proxy
- Implements exponential backoff retry (max 3 retries)

## Troubleshooting

### Process stuck in TRANSFER_IN_PROGRESS

The bridge process takes 15-20 minutes. If stuck longer:
1. Check the bridge tx on Etherscan
2. Verify CCTP attestation at https://iris-api.circle.com/attestations/{txHash}
3. Check Noble explorer for IBC transfer
4. Manually update or delete the process if needed

### Querying of Noble forwarding address

The service queries Noble RPC for the correct forwarding address. Verify with:
```bash
nobled q forwarding address channel-18 <neutron_proxy_address> --node https://noble-rpc.polkachu.com/
```

### Insufficient balance errors

Ensure the forwarder has enough USDC to cover:
- Transfer amount
- Operational fee (0.5% or min fee; configured in the database)
- Smart relay fee (40,000 = 0.04 USDC)

## License

Same as parent Hydro project
