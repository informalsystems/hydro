package main

import (
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"math/big"
	"net/http"
	"os/exec"
	"sort"
	"time"
)

const (
	NEUTRON_API_NODE = "https://neutron-testnet-api.polkachu.com:443"
	NEUTRON_RPC_NODE = "https://neutron-testnet-rpc.polkachu.com:443"

	HUB_API_NODE = "https://cosmos-testnet-api.polkachu.com:443"

	NEUTRON_CHAIN_ID = "pion-1"
)

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

type GasPricesResponse struct {
	Prices []GasPrice `json:"prices"`
}

// Function to fetch gas prices using the neutrond CLI
func fetch_gas_price() (string, error) {
	// Construct the command arguments
	cmdArgs := []string{
		"q", "feemarket", "gas-prices",
		"--node", NEUTRON_RPC_NODE,
		"-o", "json",
	}

	// Execute the command
	cmd := exec.Command("neutrond", cmdArgs...)

	// Capture the output and error
	output, err := cmd.CombinedOutput()
	if err != nil {
		fmt.Printf("Error executing command: %s\n", string(output))
		return "", fmt.Errorf("failed to execute command: %v", err)
	}

	// Parse the JSON output
	var gasPricesResponse GasPricesResponse
	err = json.Unmarshal(output, &gasPricesResponse)
	if err != nil {
		return "", fmt.Errorf("error decoding JSON: %v", err)
	}

	// Find the gas price for 'untrn'
	for _, price := range gasPricesResponse.Prices {
		if price.Denom == "untrn" {
			return price.Amount, nil
		}
	}

	return "", fmt.Errorf("untrn gas price not found")
}

type QueryDeposit struct {
	Denom  string `json:"denom"`
	Amount string `json:"amount"`
}

type ICQParams struct {
	QuerySubmitTimeout     string         `json:"query_submit_timeout"`
	QueryDeposit           []QueryDeposit `json:"query_deposit"`
	TxQueryRemovalLimit    string         `json:"tx_query_removal_limit"`
	MaxKVQueryKeysCount    string         `json:"max_kv_query_keys_count"`
	MaxTransactionsFilters string         `json:"max_transactions_filters"`
}

type ICQParamsResponse struct {
	Params ICQParams `json:"params"`
}

func fetch_min_icq_deposit() (string, error) {
	// Construct the command
	cmdArgs := []string{
		"q", "interchainqueries", "params",
		"--node", NEUTRON_RPC_NODE,
		"-o", "json",
	}

	// Execute the command
	cmd := exec.Command("neutrond", cmdArgs...)

	// Capture the output and error
	output, err := cmd.CombinedOutput()
	if err != nil {
		return "", fmt.Errorf("failed to execute command: %v\nOutput: %s", err, string(output))
	}

	// Parse the JSON output
	var paramsResponse ICQParamsResponse
	err = json.Unmarshal(output, &paramsResponse)
	if err != nil {
		return "", fmt.Errorf("error decoding JSON: %v", err)
	}

	// Extract the query_deposit amount
	if len(paramsResponse.Params.QueryDeposit) == 0 {
		return "", fmt.Errorf("query_deposit is empty")
	}

	// Assuming you want the first deposit amount
	deposit := paramsResponse.Params.QueryDeposit[0]
	amountWithDenom := fmt.Sprintf("%s%s", deposit.Amount, deposit.Denom)

	return amountWithDenom, nil
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

// Function to add validator queries in batches via CLI
func add_validator_queries(validators []string, contractAddress string) error {
	// Split validators into batches of 30
	batches := splitIntoBatches(validators, 30)

	// Fetch gas price
	gasPrice, err := fetch_gas_price()
	if err != nil {
		return fmt.Errorf("error fetching gas price: %v", err)
	}

	fmt.Printf("Using gas price: %s untrn\n", gasPrice)

	// Fetch minimum ICQ deposit
	minICQDeposit, err := fetch_min_icq_deposit()
	if err != nil {
		return fmt.Errorf("error fetching minimum ICQ deposit: %v", err)
	}

	// Loop through batches
	for i, batch := range batches {
		fmt.Printf("Processing batch %d/%d\n", i+1, len(batches))

		// Build the execute message
		msg := map[string]interface{}{
			"create_icqs_for_validators": map[string]interface{}{
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
			"--chain-id", NEUTRON_CHAIN_ID,
			"--gas", "auto",
			"--gas-adjustment", "1.3",
			"--gas-prices", fmt.Sprintf("%s%s", gasPrice, "untrn"),
			"--node", NEUTRON_RPC_NODE,
			"--from", "money",
			"-y",               // Auto-confirm the transaction
			"--output", "json", // Output format
			"--amount", minICQDeposit,
		}

		// Execute the command
		cmd := exec.Command("neutrond", cmdArgs...)

		// Capture the output and error
		output, err := cmd.CombinedOutput()
		if err != nil {
			fmt.Printf("Error executing command: %s\n", string(output))
			return fmt.Errorf("failed to execute command: %v", err)
		}

		// Print the transaction result
		fmt.Printf("Transaction result:\n%s\n", string(output))

		// Wait for 20 seconds before the next batch
		time.Sleep(20 * time.Second)
	}

	return nil
}

// Function to query Cosmos Hub validators
func query_hub_validators() ([]Validator, error) {
	// Endpoint to fetch validators
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

	// Take the top 1 validators
	topValidators := response.Validators
	if len(topValidators) > 1 {
		topValidators = topValidators[:1]
	}

	return topValidators, nil
}

// Struct for Neutron contract query response
type RegisteredValidatorQueriesResponse struct {
	QueryIDs []struct {
		ValidatorAddress string `json:"validator_address"`
		QueryID          uint64 `json:"query_id"`
	} `json:"query_ids"`
}

// Function to query Neutron validators from a CosmWasm contract
func query_hydro_validators(contractAddress string) ([]string, error) {
	// Prepare the query message
	queryMsg := map[string]interface{}{
		"registered_validator_queries": map[string]interface{}{},
	}

	// Convert the query message to JSON
	queryMsgJSON, err := json.Marshal(queryMsg)
	if err != nil {
		return nil, fmt.Errorf("error marshaling query message: %v", err)
	}

	// Base64 encode the query message
	queryMsgBase64 := base64.StdEncoding.EncodeToString(queryMsgJSON)

	// Construct the endpoint URL
	endpoint := fmt.Sprintf("%s/cosmwasm/wasm/v1/contract/%s/smart/%s", NEUTRON_API_NODE, contractAddress, queryMsgBase64)

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

	// Read the response body
	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, fmt.Errorf("error reading response body: %v", err)
	}

	// Parse the JSON response
	var result struct {
		Data RegisteredValidatorQueriesResponse `json:"data"`
	}
	err = json.Unmarshal(body, &result)
	if err != nil {
		return nil, fmt.Errorf("error decoding JSON: %v", err)
	}

	// Extract validator addresses
	var validatorAddresses []string
	for _, item := range result.Data.QueryIDs {
		validatorAddresses = append(validatorAddresses, item.ValidatorAddress)
	}

	return validatorAddresses, nil
}

func main() {
	// Query Cosmos Hub validators
	hubValidators, err := query_hub_validators()
	if err != nil {
		log.Fatalf("Error querying hub validators: %v", err)
	}

	// Print the top 300 Cosmos Hub validators
	fmt.Println("Top 300 Cosmos Hub Validators:")
	for idx, validator := range hubValidators {
		fmt.Printf("%d: OperatorAddress: %s, Tokens: %s, Moniker: %s\n",
			idx+1, validator.OperatorAddress, validator.Tokens, validator.Description.Moniker)
	}

	fmt.Println()

	contractAddress := "neutron15e0r3h6nw4d9yhe2y5kslaq9t35pdk4egm5qd8nytfmzwl9msyssew5339"

	// Query Hydro validators
	neutronValidators, err := query_hydro_validators(contractAddress)
	if err != nil {
		log.Fatalf("Error querying Hydro validators: %v", err)
	}

	// Print Neutron validator addresses
	fmt.Println("Hydro Validator Addresses:")
	for _, addr := range neutronValidators {
		fmt.Println(addr)
	}

	// Get every Hub validator who is not a validator in the Hydro contract
	diffValidators := []string{}
	for _, hubValidator := range hubValidators {
		found := false
		for _, neutronValidator := range neutronValidators {
			if hubValidator.OperatorAddress == neutronValidator {
				found = true
				break
			}
		}
		if !found {
			diffValidators = append(diffValidators, hubValidator.OperatorAddress)
		}
	}

	// Print Hub validators not in Hydro
	fmt.Println("Hub Validators not in Hydro:")
	for _, addr := range diffValidators {
		fmt.Println(addr)
	}

	// Add validator queries in batches
	err = add_validator_queries(diffValidators, contractAddress)
	if err != nil {
		log.Fatalf("Error adding validator queries: %v", err)
	}
}
