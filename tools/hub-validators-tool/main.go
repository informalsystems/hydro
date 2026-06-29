package main

import (
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"math/big"
	"net/http"
	"os"
	"os/exec"
	"sort"
	"strconv"
	"time"
)

var (
	HUB_API_NODE                  string
	HUB_RPC_NODE                  string
	HUB_CHAIN_ID                  string
	LSM_PROVIDER_CONTRACT_ADDRESS string
	BATCH_SIZE                    int
	KEY_NAME                      string
	KEYRING_BACKEND               string
	HUB_NODE_HOME                 string
)

func init() {
	// HUB_API_NODE
	HUB_API_NODE = os.Getenv("HUB_CHAIN_REST_ADDR")
	if HUB_API_NODE == "" {
		HUB_API_NODE = "https://cosmos-testnet-api.polkachu.com:443"
	}

	// HUB_RPC_NODE
	HUB_RPC_NODE = os.Getenv("HUB_CHAIN_RPC_ADDR")
	if HUB_RPC_NODE == "" {
		HUB_RPC_NODE = "https://cosmos-testnet-rpc.polkachu.com:443"
	}

	// HUB_CHAIN_ID
	HUB_CHAIN_ID = os.Getenv("HUB_CHAIN_ID")
	if HUB_CHAIN_ID == "" {
		HUB_CHAIN_ID = "cosmoshub-4"
	}

	// LSM_PROVIDER_CONTRACT_ADDRESS
	LSM_PROVIDER_CONTRACT_ADDRESS = os.Getenv("LSM_PROVIDER_CONTRACT_ADDRESS")
	if LSM_PROVIDER_CONTRACT_ADDRESS == "" {
		log.Fatal("LSM_PROVIDER_CONTRACT_ADDRESS not set. Please set LSM_PROVIDER_CONTRACT_ADDRESS environment variable.")
	}

	// BATCH_SIZE
	BATCH_SIZE = 100 // Default value
	batchSizeStr := os.Getenv("BATCH_SIZE")
	if batchSizeStr != "" {
		batchSize, err := strconv.Atoi(batchSizeStr)
		if err != nil {
			log.Fatalf("Invalid BATCH_SIZE: %v", err)
		}
		BATCH_SIZE = batchSize
	}

	// KEY_NAME
	KEY_NAME = os.Getenv("HUB_CHAIN_SIGN_KEY_NAME")
	if KEY_NAME == "" {
		log.Fatal("KEY_NAME not set. Please set HUB_CHAIN_SIGN_KEY_NAME environment variable.")
	}

	// KEYRING_BACKEND
	KEYRING_BACKEND = os.Getenv("HUB_CHAIN_KEYRING_BACKEND")
	if KEYRING_BACKEND == "" {
		KEYRING_BACKEND = "os" // Default value
	}

	// HUB_NODE_HOME
	HUB_NODE_HOME = os.Getenv("HUB_CHAIN_HOME_DIR")
	if HUB_NODE_HOME == "" {
		HUB_NODE_HOME = "$HOME/.gaia" // Default value
	}
}

type Response struct {
	Validators []Validator `json:"validators"`
	Pagination Pagination  `json:"pagination"`
}

type Validator struct {
	OperatorAddress   string          `json:"operator_address"`
	ConsensusPubkey   ConsensusPubkey `json:"consensus_pubkey"`
	Jailed            bool            `json:"jailed"`
	Status            string          `json:"status"`
	Tokens            string          `json:"tokens"`
	DelegatorShares   string          `json:"delegator_shares"`
	Description       Description     `json:"description"`
	UnbondingHeight   string          `json:"unbonding_height"`
	UnbondingTime     string          `json:"unbonding_time"`
	Commission        Commission      `json:"commission"`
	MinSelfDelegation string          `json:"min_self_delegation"`
}

type ConsensusPubkey struct {
	Type string `json:"@type"`
	Key  string `json:"key"`
}

type Description struct {
	Moniker         string `json:"moniker"`
	Identity        string `json:"identity"`
	Website         string `json:"website"`
	SecurityContact string `json:"security_contact"`
	Details         string `json:"details"`
}

type Commission struct {
	CommissionRates CommissionRates `json:"commission_rates"`
	UpdateTime      string          `json:"update_time"`
}

type CommissionRates struct {
	Rate          string `json:"rate"`
	MaxRate       string `json:"max_rate"`
	MaxChangeRate string `json:"max_change_rate"`
}

type Pagination struct {
	NextKey string `json:"next_key"`
	Total   string `json:"total"`
}

type GasPrice struct {
	Denom  string `json:"denom"`
	Amount string `json:"amount"`
}

type GasPriceResponse struct {
	Price GasPrice `json:"price"`
}

// Function to fetch gas prices using the gaiad CLI
func fetch_gas_price() (string, error) {
	// Construct the command arguments
	cmdArgs := []string{
		"q", "feemarket", "gas-price", "uatom",
		"--node", HUB_RPC_NODE,
		"-o", "json",
	}

	// Execute the command
	cmd := exec.Command("gaiad", cmdArgs...)

	// Capture the output and error
	output, err := cmd.CombinedOutput()
	if err != nil {
		fmt.Printf("Error executing command: %s\n", string(output))
		return "", fmt.Errorf("failed to execute command: %v", err)
	}

	// Parse the JSON output
	var gasPricesResponse GasPriceResponse
	err = json.Unmarshal(output, &gasPricesResponse)
	if err != nil {
		return "", fmt.Errorf("error decoding JSON: %v", err)
	}

	// Find the gas price for 'uatom'
	if gasPricesResponse.Price.Denom == "uatom" {
		return gasPricesResponse.Price.Amount, nil
	}

	return "", fmt.Errorf("uatom gas price not found: %v", gasPricesResponse)
}

// Function to split a slice into batches
func splitIntoBatches(validators []string, batchSize int) [][]string {
	var batches [][]string
	for batchSize < len(validators) {
		validators, batches = validators[batchSize:], append(batches, validators[0:batchSize:batchSize])
	}
	batches = append(batches, validators)
	return batches
}

// Function to update validator ratios in batches via CLI
func update_validators_ratios(validators []string, contractAddress string) error {
	// Split validators into batches of BATCH_SIZE
	batches := splitIntoBatches(validators, BATCH_SIZE)

	// Fetch gas price
	gasPrice, err := fetch_gas_price()
	if err != nil {
		return fmt.Errorf("error fetching gas price: %v", err)
	}

	fmt.Printf("Using gas price: %s uatom\n", gasPrice)

	// Loop through batches
	for i, batch := range batches {
		fmt.Printf("Processing batch %d/%d\n", i+1, len(batches))

		// Build the execute message
		msg := map[string]interface{}{
			"update_validators_ratios": map[string]interface{}{
				"validators": batch,
			},
		}

		// Convert the message to JSON
		msgBytes, err := json.Marshal(msg)
		if err != nil {
			return fmt.Errorf("failed to marshal execute message: %v", err)
		}
		executeMsg := string(msgBytes)

		// Construct the command
		cmdArgs := []string{
			"tx", "wasm", "execute", contractAddress, executeMsg,
			"--chain-id", HUB_CHAIN_ID,
			"--gas", "auto",
			"--gas-adjustment", "1.3",
			"--gas-prices", fmt.Sprintf("%s%s", gasPrice, "uatom"),
			"--node", HUB_RPC_NODE,
			"--from", KEY_NAME,
			"-y",               // Auto-confirm the transaction
			"--output", "json", // Output format
			"--keyring-backend", KEYRING_BACKEND,
			"--home", HUB_NODE_HOME,
		}

		// Print the command for debugging
		fmt.Printf("Command: gaiad %s\n", cmdArgs)

		// Execute the command
		cmd := exec.Command("gaiad", cmdArgs...)

		// Capture the output and error
		output, err := cmd.CombinedOutput()
		if err != nil {
			fmt.Printf("Error executing command: %s\n", string(output))
			return fmt.Errorf("failed to execute command: %v", err)
		}

		// Print the transaction result
		fmt.Printf("Transaction result:\n%s\n", string(output))

		// Wait for 10 seconds before the next batch
		time.Sleep(10 * time.Second)
	}

	return nil
}

// Function to query Cosmos Hub validators
func query_hub_validators(maxValidators int) ([]Validator, error) {
	// Endpoint to fetch validators
	// TODO: Add pagination support. 1000 is fine for now, because the Hub doesn't have that many anyways
	endpoint := fmt.Sprintf("%s/cosmos/staking/v1beta1/validators?pagination.limit=1000", HUB_API_NODE)

	// HTTP GET request
	resp, err := http.Get(endpoint)
	if err != nil {
		return nil, fmt.Errorf("error fetching data: %v", err)
	}
	defer resp.Body.Close()

	// Check for HTTP errors
	if resp.StatusCode != http.StatusOK {
		bodyBytes, _ := io.ReadAll(resp.Body)
		return nil, fmt.Errorf("HTTP request failed with status: %s, body: %s", resp.Status, string(bodyBytes))
	}

	// Parse JSON response
	var response Response
	err = json.NewDecoder(resp.Body).Decode(&response)
	if err != nil {
		return nil, fmt.Errorf("error decoding JSON: %v", err)
	}

	// Sort validators by tokens in descending order
	sort.Slice(response.Validators, func(i, j int) bool {
		tokensI := new(big.Int)
		tokensI.SetString(response.Validators[i].Tokens, 10)
		tokensJ := new(big.Int)
		tokensJ.SetString(response.Validators[j].Tokens, 10)
		return tokensI.Cmp(tokensJ) > 0
	})

	// Take the top maxValidators validators
	topValidators := response.Validators
	if len(topValidators) > maxValidators {
		topValidators = topValidators[:maxValidators]
	}

	return topValidators, nil
}

// Struct to hold the response from the contract config query
type ContractConfigResponse struct {
	Data struct {
		Config struct {
			HydroContractAddress            string `json:"hydro_contract_address"`
			MaxValidatorSharesParticipating int    `json:"max_validator_shares_participating"`
		} `json:"config"`
	} `json:"data"`
}

// Function to query the lsm-hub-token-info-provider contract config
func query_contract_config(contractAddress string) (int, error) {
	// Prepare the query message
	queryMsg := map[string]interface{}{
		"config": map[string]interface{}{},
	}

	// Convert the query message to JSON
	queryMsgJSON, err := json.Marshal(queryMsg)
	if err != nil {
		return 0, fmt.Errorf("error marshaling query message: %v", err)
	}

	// Base64 encode the query message
	queryMsgBase64 := base64.StdEncoding.EncodeToString(queryMsgJSON)

	// Construct the endpoint URL
	endpoint := fmt.Sprintf("%s/cosmwasm/wasm/v1/contract/%s/smart/%s", HUB_API_NODE, contractAddress, queryMsgBase64)

	// HTTP GET request
	resp, err := http.Get(endpoint)
	if err != nil {
		return 0, fmt.Errorf("error fetching data: %v", err)
	}
	defer resp.Body.Close()

	// Check for HTTP errors
	if resp.StatusCode != http.StatusOK {
		bodyBytes, _ := io.ReadAll(resp.Body)
		return 0, fmt.Errorf("HTTP request failed with status: %s, body: %s", resp.Status, string(bodyBytes))
	}

	// Read and parse the response body
	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return 0, fmt.Errorf("error reading response body: %v", err)
	}

	fmt.Printf("Config response body: %s\n", string(body))

	var result ContractConfigResponse
	err = json.Unmarshal(body, &result)
	if err != nil {
		return 0, fmt.Errorf("error decoding JSON: %v", err)
	}

	return result.Data.Config.MaxValidatorSharesParticipating, nil
}

func main() {
	fmt.Printf("HUB_API_NODE: %s\n", HUB_API_NODE)
	fmt.Printf("HUB_RPC_NODE: %s\n", HUB_RPC_NODE)
	fmt.Printf("HUB_CHAIN_ID: %s\n", HUB_CHAIN_ID)
	fmt.Printf("LSM_PROVIDER_CONTRACT_ADDRESS: %s\n", LSM_PROVIDER_CONTRACT_ADDRESS)
	fmt.Printf("BATCH_SIZE: %d\n", BATCH_SIZE)

	// Query the contract config to determine how many top validators to fetch
	maxValidators, err := query_contract_config(LSM_PROVIDER_CONTRACT_ADDRESS)
	if err != nil {
		log.Fatalf("Error querying contract config: %v", err)
	}

	fmt.Printf("max_validator_shares_participating: %d\n", maxValidators)

	// Query Cosmos Hub validators
	hubValidators, err := query_hub_validators(maxValidators)
	if err != nil {
		log.Fatalf("Error querying hub validators: %v", err)
	}

	// Print the top validators
	fmt.Printf("Top %d Cosmos Hub Validators:\n", maxValidators)
	validatorAddresses := make([]string, 0, len(hubValidators))
	for idx, validator := range hubValidators {
		fmt.Printf("%d: OperatorAddress: %s, Tokens: %s, Moniker: %s\n",
			idx+1, validator.OperatorAddress, validator.Tokens, validator.Description.Moniker)
		validatorAddresses = append(validatorAddresses, validator.OperatorAddress)
	}

	fmt.Println()

	// Update validator ratios in batches
	err = update_validators_ratios(validatorAddresses, LSM_PROVIDER_CONTRACT_ADDRESS)
	if err != nil {
		log.Fatalf("Error updating validator ratios: %v", err)
	}
}
