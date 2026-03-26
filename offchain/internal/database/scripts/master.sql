-- Offchain Service Database Schema
-- Simplified schema for managing USDC deposits to Inflow vaults

-- Users table (email as primary key)
CREATE TABLE IF NOT EXISTS users (
    email VARCHAR(255) PRIMARY KEY
);

-- Contracts table (stores precomputed addresses and deployment status)
CREATE TABLE IF NOT EXISTS contracts (
    id BIGSERIAL PRIMARY KEY,
    user_email VARCHAR(255) NOT NULL REFERENCES users(email) ON DELETE CASCADE,
    chain_id VARCHAR(50) NOT NULL,                    -- e.g., "1" (Ethereum), "8453" (Base)
    contract_type VARCHAR(20) NOT NULL,               -- "forwarder" or "proxy"
    address VARCHAR(66) NOT NULL,                     -- Precomputed CREATE2 address
    deployed BOOLEAN NOT NULL DEFAULT FALSE,          -- Has contract been deployed?
    deploy_tx_hash VARCHAR(66),                       -- Transaction hash of deployment
    deployed_at TIMESTAMP,                            -- When contract was deployed
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),      -- When record was created
    UNIQUE(user_email, chain_id, contract_type)
);

-- Processes table (tracks individual deposit operations)
CREATE TABLE IF NOT EXISTS processes (
    id BIGSERIAL PRIMARY KEY,
    process_id VARCHAR(200) NOT NULL UNIQUE,          -- e.g., "alice@example.com_1_001"
    user_email VARCHAR(255) NOT NULL REFERENCES users(email),
    chain_id VARCHAR(50) NOT NULL,
    forwarder_address VARCHAR(66) NOT NULL,           -- Denormalized for convenience
    proxy_address VARCHAR(66) NOT NULL,               -- Denormalized for convenience
    status VARCHAR(30) NOT NULL,                      -- PENDING_FUNDS, TRANSFER_IN_PROGRESS, etc.
    amount_usdc BIGINT,                               -- Amount in USDC base units (6 decimals)

    -- Transaction hashes for tracking
    bridge_tx_hash VARCHAR(66),                       -- EVM forwarder.bridge() tx
    deposit_tx_hash VARCHAR(66),                      -- Neutron proxy ForwardToInflow tx

    -- Error tracking (simplified)
    error_message TEXT,
    retry_count INT NOT NULL DEFAULT 0,

    -- Timestamps
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);

-- Chains table (supported EVM chains and their configuration)
CREATE TABLE IF NOT EXISTS chains (
    chain_id                 VARCHAR(50)  PRIMARY KEY,
    name                     VARCHAR(100) NOT NULL,
    type                     VARCHAR(20)  NOT NULL,           -- e.g. "evm"
    rpc_endpoint             TEXT         NOT NULL,
    usdc_contract_address    VARCHAR(66)  NOT NULL,
    cctp_contract_address    VARCHAR(66)  NOT NULL,
    operational_fee_bps      BIGINT       NOT NULL DEFAULT 0,
    min_operational_fee      BIGINT       NOT NULL DEFAULT 0,
    min_deposit_amount       BIGINT       NOT NULL DEFAULT 0,
    forwarder_contract_admin VARCHAR(66)  NOT NULL,
    fee_recipient            VARCHAR(66)  NOT NULL
);

-- Indices for performance
CREATE INDEX IF NOT EXISTS idx_contracts_user_email ON contracts(user_email);
CREATE INDEX IF NOT EXISTS idx_contracts_deployed ON contracts(deployed);
CREATE INDEX IF NOT EXISTS idx_processes_user_email ON processes(user_email);
CREATE INDEX IF NOT EXISTS idx_processes_status ON processes(status);
CREATE INDEX IF NOT EXISTS idx_processes_chain_id ON processes(chain_id);

INSERT INTO chains (chain_id, name, type, rpc_endpoint, usdc_contract_address, cctp_contract_address, operational_fee_bps, min_operational_fee, min_deposit_amount, forwarder_contract_admin, fee_recipient)
VALUES
    ('1',    'Ethereum', 'evm', 'https://ethereum-rpc.publicnode.com', '0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48', '0xBC8552339dA68EB65C8b88B414B5854E0E366cFc', 50, 10000, 50000, '0x5FD9c2335B1247566f53f6304873dC3046Ef907a', '0x5FD9c2335B1247566f53f6304873dC3046Ef907a'),
    ('8453', 'Base',     'evm', 'https://base-rpc.publicnode.com',     '0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913', '0xB6615B2662b35fc3533F8479002e62D0523341De', 50, 10000, 50000, '0x5FD9c2335B1247566f53f6304873dC3046Ef907a', '0x5FD9c2335B1247566f53f6304873dC3046Ef907a')
ON CONFLICT (chain_id) DO NOTHING;
