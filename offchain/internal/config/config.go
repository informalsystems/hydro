package config

import (
	"fmt"
	"os"
	"strconv"
)

// Config holds all configuration for the service
type Config struct {
	Server   ServerConfig
	Database DatabaseConfig
	Chains   map[string]ChainConfig
	Neutron  NeutronConfig
	Operator OperatorConfig
}

// ServerConfig holds HTTP server configuration
type ServerConfig struct {
	Port int
}

// DatabaseConfig holds PostgreSQL configuration
type DatabaseConfig struct {
	Host     string
	Port     int
	User     string
	Password string
	DBName   string
	SSLMode  string
}

// ChainConfig holds configuration for an EVM chain
type ChainConfig struct {
	ChainID              string
	Name                 string
	Type                 string // "evm"
	RPCEndpoint          string
	USDCContractAddress  string // USDC ERC20 contract address
	CCTPContractAddress  string // Skip's CCTP contract address
	OperatorAddress      string // Operator address on this chain
	OperationalFeeBps    uint16 // e.g., 50 = 0.5%
	MinOperationalFee    int64  // e.g., 1000000 = 1 USDC (6 decimals)
	MinDepositAmount     int64  // e.g., 10000000 = 10 USDC
	ForwarderBytecode    string // Hex-encoded bytecode with constructor args
	DestinationDomain    uint32 // CCTP destination domain (Noble = 4)
	DestinationCaller    string // Skip relayer address (as hex bytes32)
}

// NeutronConfig holds Neutron-specific configuration
type NeutronConfig struct {
	RPCEndpoint       string
	GRPCEndpoint      string
	RESTEndpoint      string   // REST/LCD API endpoint for queries
	ControlCenters    []string // Control center contract addresses
	Admins            []string // Admin addresses for proxy contracts
	ProxyCodeID       uint64   // Code ID of stored proxy contract
	NobleAPIEndpoint  string   // Noble REST API endpoint for forwarding queries (deprecated, use NobleRPCEndpoint)
	NobleRPCEndpoint  string   // Noble RPC endpoint for ABCI queries
	NobleChannel      string   // IBC channel between Noble and Neutron (e.g., "channel-18")
}

// OperatorConfig holds operator wallet configuration
type OperatorConfig struct {
	EVMPrivateKey    string // For signing EVM transactions
	NeutronMnemonic  string // For signing Neutron transactions
	NeutronAddress   string // Operator's Neutron address
	FeeRecipient     string // Where operational fees are sent (EVM address)
	AdminAddress     string // Admin address for emergency functions
}

// LoadConfig loads configuration from environment variables
func LoadConfig() (*Config, error) {
	cfg := &Config{
		Server: ServerConfig{
			Port: getEnvInt("SERVER_PORT", 8080),
		},
		Database: DatabaseConfig{
			Host:     getEnv("DB_HOST", "localhost"),
			Port:     getEnvInt("DB_PORT", 5432),
			User:     getEnv("DB_USER", "postgres"),
			Password: getEnv("DB_PASSWORD", "postgres"),
			DBName:   getEnv("DB_NAME", "inflow_service"),
			SSLMode:  getEnv("DB_SSL_MODE", "disable"),
		},
		Operator: OperatorConfig{
			EVMPrivateKey:   getEnv("OPERATOR_EVM_PRIVATE_KEY", ""),
			NeutronMnemonic: getEnv("OPERATOR_NEUTRON_MNEMONIC", ""),
			NeutronAddress:  getEnv("OPERATOR_NEUTRON_ADDRESS", ""),
			FeeRecipient:    getEnv("OPERATOR_FEE_RECIPIENT", ""),
			AdminAddress:    getEnv("OPERATOR_ADMIN_ADDRESS", ""),
		},
		Chains: make(map[string]ChainConfig),
	}

	// Load chain configurations
	if err := loadChainConfigs(cfg); err != nil {
		return nil, err
	}

	// Load Neutron configuration
	if err := loadNeutronConfig(cfg); err != nil {
		return nil, err
	}

	// Validate configuration
	if err := cfg.Validate(); err != nil {
		return nil, fmt.Errorf("invalid configuration: %w", err)
	}

	return cfg, nil
}

// loadChainConfigs loads configuration for all supported chains
func loadChainConfigs(cfg *Config) error {
	// Ethereum
	if rpc := getEnv("ETH_RPC_ENDPOINT", ""); rpc != "" {
		cfg.Chains["1"] = ChainConfig{
			ChainID:              "1",
			Name:                 "Ethereum",
			Type:                 "evm",
			RPCEndpoint:          rpc,
			USDCContractAddress:  getEnv("ETH_USDC_ADDRESS", "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
			CCTPContractAddress:  getEnv("ETH_CCTP_CONTRACT", ""),
			OperatorAddress:      getEnv("ETH_OPERATOR_ADDRESS", ""),
			OperationalFeeBps:    uint16(getEnvInt("ETH_OPERATIONAL_FEE_BPS", 50)),
			MinOperationalFee:    int64(getEnvInt("ETH_MIN_OPERATIONAL_FEE", 1000000)),
			MinDepositAmount:     int64(getEnvInt("ETH_MIN_DEPOSIT", 50000000)),
			ForwarderBytecode:    getEnv("ETH_FORWARDER_BYTECODE", ""),
			DestinationDomain:    uint32(getEnvInt("CCTP_DESTINATION_DOMAIN", 4)),
			DestinationCaller:    getEnv("CCTP_DESTINATION_CALLER", ""),
		}
	}

	// Base
	if rpc := getEnv("BASE_RPC_ENDPOINT", ""); rpc != "" {
		cfg.Chains["8453"] = ChainConfig{
			ChainID:              "8453",
			Name:                 "Base",
			Type:                 "evm",
			RPCEndpoint:          rpc,
			USDCContractAddress:  getEnv("BASE_USDC_ADDRESS", "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"),
			CCTPContractAddress:  getEnv("BASE_CCTP_CONTRACT", ""),
			OperatorAddress:      getEnv("BASE_OPERATOR_ADDRESS", ""),
			OperationalFeeBps:    uint16(getEnvInt("BASE_OPERATIONAL_FEE_BPS", 50)),
			MinOperationalFee:    int64(getEnvInt("BASE_MIN_OPERATIONAL_FEE", 1000000)),
			MinDepositAmount:     int64(getEnvInt("BASE_MIN_DEPOSIT", 10000000)),
			ForwarderBytecode:    getEnv("BASE_FORWARDER_BYTECODE", ""),
			DestinationDomain:    uint32(getEnvInt("CCTP_DESTINATION_DOMAIN", 4)),
			DestinationCaller:    getEnv("CCTP_DESTINATION_CALLER", ""),
		}
	}

	return nil
}

// loadNeutronConfig loads Neutron-specific configuration
func loadNeutronConfig(cfg *Config) error {
	rpc := getEnv("NEUTRON_RPC_ENDPOINT", "")
	if rpc == "" {
		return fmt.Errorf("NEUTRON_RPC_ENDPOINT is required")
	}

	// Parse control centers (comma-separated)
	controlCentersStr := getEnv("NEUTRON_CONTROL_CENTERS", "")
	controlCenters := splitAndTrim(controlCentersStr, ",")
	if len(controlCenters) == 0 {
		return fmt.Errorf("NEUTRON_CONTROL_CENTERS is required")
	}

	// Parse admins (comma-separated)
	adminsStr := getEnv("NEUTRON_ADMINS", "")
	admins := splitAndTrim(adminsStr, ",")
	if len(admins) == 0 {
		return fmt.Errorf("NEUTRON_ADMINS is required")
	}

	cfg.Neutron = NeutronConfig{
		RPCEndpoint:      rpc,
		GRPCEndpoint:     getEnv("NEUTRON_GRPC_ENDPOINT", ""),
		RESTEndpoint:     getEnv("NEUTRON_REST_ENDPOINT", ""),
		ControlCenters:   controlCenters,
		Admins:           admins,
		ProxyCodeID:      uint64(getEnvInt("NEUTRON_PROXY_CODE_ID", 0)),
		NobleAPIEndpoint: getEnv("NOBLE_API_ENDPOINT", ""),
		NobleRPCEndpoint: getEnv("NOBLE_RPC_ENDPOINT", "https://noble-rpc.polkachu.com"),
		NobleChannel:     getEnv("NOBLE_NEUTRON_CHANNEL", "channel-18"),
	}

	return nil
}

// Validate checks if the configuration is valid
func (c *Config) Validate() error {
	if c.Server.Port <= 0 {
		return fmt.Errorf("invalid server port: %d", c.Server.Port)
	}

	if c.Database.Host == "" {
		return fmt.Errorf("database host is required")
	}

	if c.Operator.EVMPrivateKey == "" {
		return fmt.Errorf("operator EVM private key is required")
	}

	if len(c.Chains) == 0 {
		return fmt.Errorf("at least one chain must be configured")
	}

	return nil
}

// Helper functions

func getEnv(key, defaultValue string) string {
	if value := os.Getenv(key); value != "" {
		return value
	}
	return defaultValue
}

func getEnvInt(key string, defaultValue int) int {
	if value := os.Getenv(key); value != "" {
		if intValue, err := strconv.Atoi(value); err == nil {
			return intValue
		}
	}
	return defaultValue
}

// splitAndTrim splits a comma-separated string and trims whitespace
func splitAndTrim(s, sep string) []string {
	if s == "" {
		return nil
	}
	parts := make([]string, 0)
	for _, part := range split(s, sep) {
		trimmed := trim(part)
		if trimmed != "" {
			parts = append(parts, trimmed)
		}
	}
	return parts
}

func split(s, sep string) []string {
	result := []string{}
	current := ""
	for _, c := range s {
		if string(c) == sep {
			result = append(result, current)
			current = ""
		} else {
			current += string(c)
		}
	}
	if current != "" {
		result = append(result, current)
	}
	return result
}

func trim(s string) string {
	start := 0
	end := len(s)
	for start < end && (s[start] == ' ' || s[start] == '\t' || s[start] == '\n') {
		start++
	}
	for end > start && (s[end-1] == ' ' || s[end-1] == '\t' || s[end-1] == '\n') {
		end--
	}
	return s[start:end]
}
