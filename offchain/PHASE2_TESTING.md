# Phase 2 API Testing Guide

## Overview
Phase 2 implements the REST API layer for the Inflow offchain service. This includes contract address management, fee calculation, and process status endpoints.

## Prerequisites

1. PostgreSQL database running
2. Environment variables configured (see `.env.example`)
3. At least one chain configured (Ethereum or Base)

## Starting the Service

```bash
# Build the service
go build -o server ./cmd/server

# Run with environment variables
export SERVER_PORT=8080
export DB_HOST=localhost
export DB_PORT=5432
export DB_USER=postgres
export DB_PASSWORD=postgres
export DB_NAME=inflow_service

# Configure at least one chain (example for Base testnet)
export BASE_RPC_ENDPOINT=https://base-sepolia.g.alchemy.com/v2/YOUR_KEY
export BASE_USDC_ADDRESS=0x036CbD53842c5426634e7929541eC2318f3dCF7e
export BASE_CCTP_CONTRACT=0x...
export BASE_OPERATOR_ADDRESS=0x...
export BASE_FORWARDER_BYTECODE=0x...
export BASE_OPERATIONAL_FEE_BPS=50
export BASE_MIN_OPERATIONAL_FEE=1000000
export BASE_MIN_DEPOSIT=10000000

# Neutron configuration
export NEUTRON_RPC_ENDPOINT=https://rpc-palvus.pion-1.ntrn.tech
export NEUTRON_CONTROL_CENTERS=neutron1...
export NEUTRON_ADMINS=neutron1...
export NEUTRON_PROXY_CODE_ID=1234

# Operator configuration
export OPERATOR_EVM_PRIVATE_KEY=0x...
export OPERATOR_NEUTRON_MNEMONIC="word1 word2 ... word24"
export OPERATOR_NEUTRON_ADDRESS=neutron1...
export OPERATOR_FEE_RECIPIENT=0x...

# Run the service
./server
```

## API Endpoints

### 1. Health Check

```bash
curl http://localhost:8080/health
```

Expected response:
```json
{
  "status": "ok",
  "version": "1.0.0"
}
```

### 2. Get Contract Addresses

Gets or creates forwarder and proxy addresses for a user on specified chains.

```bash
curl -X POST http://localhost:8080/api/v1/contracts/addresses \
  -H "Content-Type: application/json" \
  -d '{
    "email": "alice@example.com",
    "chain_ids": ["8453"]
  }'
```

Expected response:
```json
{
  "email": "alice@example.com",
  "contracts": {
    "8453": {
      "forwarder": "0x1234567890abcdef1234567890abcdef12345678",
      "proxy": "neutron1abcdefghijklmnopqrstuvwxyz1234567890ab"
    }
  }
}
```

**Notes:**
- Addresses are deterministically computed using CREATE2 (EVM) and instantiate2 (Neutron)
- Same email will always get the same addresses
- Proxy address is shared across all EVM chains
- Forwarder address is unique per user per chain

### 3. Calculate Fees

Calculates the bridge fee for a given chain and amount.

```bash
curl -X POST http://localhost:8080/api/v1/fees/calculate \
  -H "Content-Type: application/json" \
  -d '{
    "chain_id": "8453",
    "amount_usdc": "100000000"
  }'
```

Expected response:
```json
{
  "bridge_fee_usdc": "1000000",
  "min_deposit_usdc": "10000000"
}
```

**Notes:**
- Amounts are in base units (6 decimals): 1000000 = 1 USDC
- Fee = max(amount * operationalFeeBps / 10000, minOperationalFee)
- Example: 100 USDC * 50 bps / 10000 = 0.5 USDC fee

### 4. Get Process Status

Gets the status of a specific deposit process.

```bash
curl http://localhost:8080/api/v1/processes/status/alice@example.com_8453_001
```

Expected response:
```json
{
  "process_id": "alice@example.com_8453_001",
  "status": "PENDING_FUNDS",
  "amount_usdc": "100000000",
  "tx_hashes": {
    "bridge": null,
    "deposit": null
  }
}
```

**Process statuses:**
- `PENDING_FUNDS` - Waiting for user to send USDC
- `TRANSFER_IN_PROGRESS` - Bridge transaction in progress (EVM → Noble → Neutron)
- `DEPOSIT_IN_PROGRESS` - Depositing into Inflow vault
- `DEPOSIT_DONE` - Process complete
- `FAILED` - Process failed (check error field)

### 5. Get User Processes

Lists all processes for a user.

```bash
curl http://localhost:8080/api/v1/processes/user/alice@example.com
```

Optional query parameters:
- `limit` - Number of processes to return (default: 50)
- `offset` - Number of processes to skip (default: 0)

```bash
curl "http://localhost:8080/api/v1/processes/user/alice@example.com?limit=10&offset=0"
```

Expected response:
```json
{
  "processes": [
    {
      "process_id": "alice@example.com_8453_001",
      "chain_id": "8453",
      "forwarder_address": "0x1234...",
      "proxy_address": "neutron1abc...",
      "status": "DEPOSIT_DONE",
      "amount_usdc": "100000000",
      "tx_hashes": {
        "bridge": "0xabc...",
        "deposit": "DEF123..."
      }
    }
  ]
}
```

## Testing Flow

### Complete User Deposit Flow

1. **Get contract addresses**
   ```bash
   curl -X POST http://localhost:8080/api/v1/contracts/addresses \
     -H "Content-Type: application/json" \
     -d '{"email": "test@example.com", "chain_ids": ["8453"]}'
   ```
   Save the `forwarder` address from the response.

2. **Calculate fees**
   ```bash
   curl -X POST http://localhost:8080/api/v1/fees/calculate \
     -H "Content-Type: application/json" \
     -d '{"chain_id": "8453", "amount_usdc": "50000000"}'
   ```
   Note: You need at least `amount + bridge_fee` in the forwarder address.

3. **Check process status** (will show empty initially)
   ```bash
   curl http://localhost:8080/api/v1/processes/user/test@example.com
   ```

4. **Send USDC to forwarder address** (using wallet/script)
   - In Phase 4, workers will automatically detect the deposit
   - For now, processes need to be created manually via database

5. **Monitor process status**
   ```bash
   # Check specific process
   curl http://localhost:8080/api/v1/processes/status/test@example.com_8453_001

   # Or check all user processes
   curl http://localhost:8080/api/v1/processes/user/test@example.com
   ```

## Error Responses

All errors follow this format:

```json
{
  "error": "Short error description",
  "message": "Detailed error message with context"
}
```

Common HTTP status codes:
- `200` - Success
- `400` - Bad request (invalid parameters)
- `404` - Resource not found
- `500` - Internal server error

## Next Steps (Phase 3 & 4)

Phase 2 provides the API layer, but deposit processing is not yet automated:

- **Phase 3**: Blockchain clients for interacting with EVM and Cosmos chains
- **Phase 4**: Background workers that monitor forwarder balances and execute deposits

Currently, you can:
- ✅ Get precomputed contract addresses
- ✅ Calculate fees
- ✅ Query process status (if manually created)
- ❌ Automatic deposit processing (Phase 4)
- ❌ Contract deployment (Phase 3)
- ❌ Balance monitoring (Phase 4)

## Database Queries for Testing

To manually create a process for testing:

```sql
-- Insert a test process
INSERT INTO processes (
  process_id, user_email, chain_id, forwarder_address, proxy_address, status
) VALUES (
  'test@example.com_8453_001',
  'test@example.com',
  '8453',
  '0x1234567890abcdef1234567890abcdef12345678',
  'neutron1abcdefghijklmnopqrstuvwxyz1234567890ab',
  'PENDING_FUNDS'
);

-- Query processes
SELECT * FROM processes WHERE user_email = 'test@example.com';

-- Query contracts
SELECT * FROM contracts WHERE user_email = 'test@example.com';
```

## Troubleshooting

### Service won't start
- Check database connection (DB_HOST, DB_PORT, DB_USER, DB_PASSWORD)
- Ensure at least one chain is configured
- Check that NEUTRON_RPC_ENDPOINT is set

### Contract address endpoint fails
- Verify chain configuration (RPC endpoint, forwarder bytecode)
- Check NEUTRON_PROXY_CODE_ID is set
- Ensure OPERATOR_NEUTRON_ADDRESS is valid bech32 address

### Fee calculation fails
- Verify chain_id matches a configured chain
- Check amount_usdc is a valid positive integer

## Configuration Reference

See [docs/offchain-service/implementation-plan.md](../../docs/offchain-service/implementation-plan.md) for full configuration details.

Minimum required environment variables:
- Database: `DB_HOST`, `DB_PORT`, `DB_USER`, `DB_PASSWORD`, `DB_NAME`
- Server: `SERVER_PORT`
- At least one chain: `{CHAIN}_RPC_ENDPOINT`, `{CHAIN}_FORWARDER_BYTECODE`, etc.
- Neutron: `NEUTRON_RPC_ENDPOINT`, `NEUTRON_CONTROL_CENTERS`, `NEUTRON_ADMINS`, `NEUTRON_PROXY_CODE_ID`
- Operator: `OPERATOR_EVM_PRIVATE_KEY`, `OPERATOR_NEUTRON_MNEMONIC`, `OPERATOR_NEUTRON_ADDRESS`
