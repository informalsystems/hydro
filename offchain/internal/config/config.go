package config

import (
	"context"
	"fmt"
	"os"
	"strconv"
	"strings"

	"crypto/ecdsa"

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/crypto"

	"hydro/offchain/internal/database"
	"hydro/offchain/internal/models"
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
	ChainID                string
	Name                   string
	Type                   string // "evm"
	RPCEndpoint            string
	USDCContractAddress    string // USDC ERC20 contract address
	CCTPContractAddress    string // Skip's CCTP contract address
	OperationalFeeBps      int64  // e.g., 50 = 0.5%
	MinOperationalFee      int64  // e.g., 1000000 = 1 USDC (6 decimals)
	MinDepositAmount       int64  // e.g., 10000000 = 10 USDC
	ForwarderBytecode      string
	DestinationDomain      uint32
	DestinationCaller      string
	ForwarderContractAdmin string
	FeeRecipient           string // Where operational fees are sent (EVM address)
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
	NobleRESTEndpoint string   // Noble REST/LCD endpoint for account info queries when signing TXs
	NobleChannel      string   // IBC channel between Noble and Neutron (e.g., "channel-18")
}

// OperatorConfig holds operator wallet configuration
type OperatorConfig struct {
	EVMAccountInfo  EVMAccountInfo // For signing EVM transactions
	NeutronMnemonic string         // For signing Neutron transactions
	NobleMnemonic   string         // For signing Noble transactions
	NeutronAddress  string         // Operator's Neutron address
}

type EVMAccountInfo struct {
	PrivateKey *ecdsa.PrivateKey
	PublicKey  *ecdsa.PublicKey
	Address    *common.Address
}

// LoadConfig loads configuration from environment variables
func LoadConfig() (*Config, error) {
	evmAccountInfo, err := parseEVMAccountInfo(getEnv("OPERATOR_EVM_PRIVATE_KEY", ""))
	if err != nil {
		return nil, fmt.Errorf("failed to parse EVM private key: %w", err)
	}

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
			EVMAccountInfo:  *evmAccountInfo,
			NeutronMnemonic: getEnv("OPERATOR_NEUTRON_MNEMONIC", ""),
			NeutronAddress:  getEnv("OPERATOR_NEUTRON_ADDRESS", ""),
			NobleMnemonic:   getEnv("OPERATOR_NOBLE_MNEMONIC", ""),
		},
		Chains: make(map[string]ChainConfig),
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

func parseEVMAccountInfo(privateKeyHex string) (*EVMAccountInfo, error) {
	// Parse private key (remove 0x prefix if present)
	privateKeyHex = strings.TrimPrefix(privateKeyHex, "0x")
	privateKey, err := crypto.HexToECDSA(privateKeyHex)
	if err != nil {
		return nil, fmt.Errorf("failed to parse private key: %w", err)
	}

	// Get public key and address
	publicKey := privateKey.Public()
	publicKeyECDSA, ok := publicKey.(*ecdsa.PublicKey)
	if !ok {
		return nil, fmt.Errorf("failed to cast public key to ECDSA")
	}

	address := crypto.PubkeyToAddress(*publicKeyECDSA)

	return &EVMAccountInfo{
		PrivateKey: privateKey,
		PublicKey:  publicKeyECDSA,
		Address:    &address,
	}, nil
}

// LoadChainConfigs loads chain configurations from the database and populates cfg.Chains.
// EVM-wide env vars (EVM_FORWARDER_BYTECODE, CCTP_DESTINATION_DOMAIN, CCTP_DESTINATION_CALLER)
// are applied to all chains since they can't be stored in the database yet.
func (cfg *Config) LoadChainConfigs(db *database.DB) error {
	rows, err := db.GetAllChains(context.Background())
	if err != nil {
		return fmt.Errorf("failed to query chains from database: %w", err)
	}

	forwarderBytecode := getEnv("EVM_FORWARDER_BYTECODE", "")
	destinationDomain := uint32(getEnvInt("CCTP_DESTINATION_DOMAIN", 4))
	destinationCaller := getEnv("CCTP_DESTINATION_CALLER", "")

	for _, row := range rows {
		chainCfg := chainConfigFromRow(row, forwarderBytecode, destinationDomain, destinationCaller)
		cfg.Chains[row.ChainID] = chainCfg
	}

	return nil
}

func chainConfigFromRow(row models.Chain, forwarderBytecode string, destinationDomain uint32, destinationCaller string) ChainConfig {
	return ChainConfig{
		ChainID:                row.ChainID,
		Name:                   row.Name,
		Type:                   row.Type,
		RPCEndpoint:            row.RPCEndpoint,
		USDCContractAddress:    row.USDCContractAddress,
		CCTPContractAddress:    row.CCTPContractAddress,
		OperationalFeeBps:      row.OperationalFeeBps,
		MinOperationalFee:      row.MinOperationalFee,
		MinDepositAmount:       row.MinDepositAmount,
		ForwarderContractAdmin: row.ForwarderContractAdmin,
		FeeRecipient:           row.FeeRecipient,
		ForwarderBytecode:      forwarderBytecode,
		DestinationDomain:      destinationDomain,
		DestinationCaller:      destinationCaller,
	}
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
		RPCEndpoint:       rpc,
		GRPCEndpoint:      getEnv("NEUTRON_GRPC_ENDPOINT", ""),
		RESTEndpoint:      getEnv("NEUTRON_REST_ENDPOINT", ""),
		ControlCenters:    controlCenters,
		Admins:            admins,
		ProxyCodeID:       uint64(getEnvInt("NEUTRON_PROXY_CODE_ID", 0)),
		NobleAPIEndpoint:  getEnv("NOBLE_API_ENDPOINT", ""),
		NobleRPCEndpoint:  getEnv("NOBLE_RPC_ENDPOINT", "https://noble-rpc.polkachu.com"),
		NobleRESTEndpoint: getEnv("NOBLE_REST_ENDPOINT", ""),
		NobleChannel:      getEnv("NOBLE_NEUTRON_CHANNEL", "channel-18"),
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
