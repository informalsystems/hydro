package config

import (
	"context"
	"fmt"
	"os"
	"strconv"
	"strings"

	"crypto/ecdsa"

	"github.com/cosmos/cosmos-sdk/crypto/hd"
	"github.com/cosmos/cosmos-sdk/types"
	"github.com/cosmos/go-bip39"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/crypto"

	"hydro/offchain/internal/database"
	"hydro/offchain/internal/models"
)

// Config holds all configuration for the service
type Config struct {
	Server       ServerConfig
	Database     DatabaseConfig
	Chains       map[string]ChainConfig
	CosmosChains CosmosChainsConfig
	Operator     OperatorConfig
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

// CosmosChainsConfig holds Cosmos chains configuration
type CosmosChainsConfig struct {
	NeutronRPCEndpoint  string   // CometBFT RPC endpoint for signing and broadcasting transactions
	NeutronRESTEndpoint string   // API REST endpoint for querying SDK modules
	ControlCenters      []string // Inflow Control Center contract addresses on Neutron
	Admins              []string // Addresses to be used as admins of Proxy contracts on Neutron
	ProxyCodeID         uint64   // Code ID of stored Proxy contract code on Neutron
	NobleRPCEndpoint    string   // CometBFT RPC endpoint for signing and broadcasting transactions
	NobleRESTEndpoint   string   // API REST endpoint for querying SDK modules
	NobleNeutronChannel string   // IBC channel between Noble and Neutron (on mainnet: "channel-18")
}

// OperatorConfig holds operator wallet configuration
type OperatorConfig struct {
	EVMAccountInfo     EVMAccountInfo    // For signing EVM transactions
	NeutronAccountInfo CosmosAccountInfo // For signing Neutron transactions
	NobleAccountInfo   CosmosAccountInfo // For signing Noble transactions
}

type EVMAccountInfo struct {
	PrivateKey *ecdsa.PrivateKey
	PublicKey  *ecdsa.PublicKey
	Address    *common.Address
}

type CosmosAccountInfo struct {
	Mnemonic string
	Address  string
}

// LoadConfig loads configuration from environment variables
func LoadConfig() (*Config, error) {
	operatorEVMPrivateKey, err := getEnvString("OPERATOR_EVM_PRIVATE_KEY")
	if err != nil {
		return nil, err
	}

	operatorEVMAccountInfo, err := parseEVMAccountInfo(operatorEVMPrivateKey)
	if err != nil {
		return nil, fmt.Errorf("failed to parse EVM private key: %w", err)
	}

	operatorNeutronMnemonic, err := getEnvString("OPERATOR_NEUTRON_MNEMONIC")
	if err != nil {
		return nil, err
	}
	operatorNeutronAccountInfo, err := parseCosmosAccountInfo(operatorNeutronMnemonic, "neutron")
	if err != nil {
		return nil, fmt.Errorf("failed to parse Neutron mnemonic: %w", err)
	}

	operatorNobleMnemonic, err := getEnvString("OPERATOR_NOBLE_MNEMONIC")
	if err != nil {
		return nil, err
	}
	operatorNobleAccountInfo, err := parseCosmosAccountInfo(operatorNobleMnemonic, "noble")
	if err != nil {
		return nil, fmt.Errorf("failed to parse Noble mnemonic: %w", err)
	}

	serverPort, err := getEnvInt("SERVER_PORT")
	if err != nil {
		return nil, err
	}

	dbHost, err := getEnvString("DB_HOST")
	if err != nil {
		return nil, err
	}
	dbPort, err := getEnvInt("DB_PORT")
	if err != nil {
		return nil, err
	}
	dbUser, err := getEnvString("DB_USER")
	if err != nil {
		return nil, err
	}
	dbPassword, err := getEnvString("DB_PASSWORD")
	if err != nil {
		return nil, err
	}
	dbName, err := getEnvString("DB_NAME")
	if err != nil {
		return nil, err
	}
	dbSSLMode, err := getEnvString("DB_SSL_MODE")
	if err != nil {
		return nil, err
	}

	cfg := &Config{
		Server: ServerConfig{
			Port: serverPort,
		},
		Database: DatabaseConfig{
			Host:     dbHost,
			Port:     dbPort,
			User:     dbUser,
			Password: dbPassword,
			DBName:   dbName,
			SSLMode:  dbSSLMode,
		},
		Operator: OperatorConfig{
			EVMAccountInfo:     *operatorEVMAccountInfo,
			NeutronAccountInfo: *operatorNeutronAccountInfo,
			NobleAccountInfo:   *operatorNobleAccountInfo,
		},
		Chains: make(map[string]ChainConfig),
	}

	// Load Cosmos chains configuration
	if err := loadCosmosChainsConfig(cfg); err != nil {
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

func parseCosmosAccountInfo(mnemonic string, prefix string) (*CosmosAccountInfo, error) {
	// Validate mnemonic
	if !bip39.IsMnemonicValid(mnemonic) {
		return nil, fmt.Errorf("invalid mnemonic: %s", mnemonic)
	}

	// Cosmos derivation path: m/44'/118'/0'/0/0
	hdPath := hd.CreateHDPath(118, 0, 0).String()

	// Derive private key from mnemonic
	derivedPriv, err := hd.Secp256k1.Derive()(mnemonic, "", hdPath)
	if err != nil {
		return nil, fmt.Errorf("failed to derive private key: %w", err)
	}

	// Generate private key
	privKey := hd.Secp256k1.Generate()(derivedPriv)

	// Get public key
	pubKey := privKey.PubKey()

	// Convert to address bytes
	addr := types.AccAddress(pubKey.Address())

	// Convert to bech32 with custom prefix
	bech32Addr, err := types.Bech32ifyAddressBytes(prefix, addr)
	if err != nil {
		return nil, fmt.Errorf("failed to encode address: %w", err)
	}

	return &CosmosAccountInfo{
		Mnemonic: mnemonic,
		Address:  bech32Addr,
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

	forwarderBytecode, err := getEnvString("EVM_FORWARDER_BYTECODE")
	if err != nil {
		return err
	}
	destinationDomainInt, err := getEnvInt("CCTP_DESTINATION_DOMAIN")
	if err != nil {
		return err
	}
	destinationDomain := uint32(destinationDomainInt)
	destinationCaller, err := getEnvString("CCTP_DESTINATION_CALLER")
	if err != nil {
		return err
	}

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

// loadCosmosChainsConfig loads Neutron and Noble chains configurations
func loadCosmosChainsConfig(cfg *Config) error {
	neutronRPCEndpoint, err := getEnvString("NEUTRON_RPC_ENDPOINT")
	if err != nil {
		return err
	}

	// Parse control centers (comma-separated)
	controlCentersStr, err := getEnvString("NEUTRON_CONTROL_CENTERS")
	if err != nil {
		return err
	}

	controlCenters := splitAndTrim(controlCentersStr, ",")
	if len(controlCenters) == 0 {
		return fmt.Errorf("NEUTRON_CONTROL_CENTERS is required")
	}

	// Parse admins (comma-separated)
	adminsStr, err := getEnvString("NEUTRON_ADMINS")
	if err != nil {
		return err
	}
	admins := splitAndTrim(adminsStr, ",")
	if len(admins) == 0 {
		return fmt.Errorf("NEUTRON_ADMINS is required")
	}

	proxyCodeID, err := getEnvInt("NEUTRON_PROXY_CODE_ID")
	if err != nil {
		return err
	}

	neutronRESTEndpoint, err := getEnvString("NEUTRON_REST_ENDPOINT")
	if err != nil {
		return err
	}

	nobleRPCEndpoint, err := getEnvString("NOBLE_RPC_ENDPOINT")
	if err != nil {
		return err
	}

	nobleRESTEndpoint, err := getEnvString("NOBLE_REST_ENDPOINT")
	if err != nil {
		return err
	}

	nobleNeutronChannel, err := getEnvString("NOBLE_NEUTRON_CHANNEL")
	if err != nil {
		return err
	}

	cfg.CosmosChains = CosmosChainsConfig{
		NeutronRPCEndpoint:  neutronRPCEndpoint,
		NeutronRESTEndpoint: neutronRESTEndpoint,
		ControlCenters:      controlCenters,
		Admins:              admins,
		ProxyCodeID:         uint64(proxyCodeID),
		NobleRPCEndpoint:    nobleRPCEndpoint,
		NobleRESTEndpoint:   nobleRESTEndpoint,
		NobleNeutronChannel: nobleNeutronChannel,
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

func getEnvString(key string) (string, error) {
	if value := os.Getenv(key); value != "" {
		return value, nil
	}
	return "", fmt.Errorf("environment variable %s is required", key)
}

func getEnvInt(key string) (int, error) {
	if value := os.Getenv(key); value != "" {
		if intValue, err := strconv.Atoi(value); err == nil {
			return intValue, nil
		}
	}
	return 0, fmt.Errorf("environment variable %s is required", key)
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
