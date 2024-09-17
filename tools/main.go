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

	HYDRO_CONTRACT_ADDRESS = "neutron15e0r3h6nw4d9yhe2y5kslaq9t35pdk4egm5qd8nytfmzwl9msyssew5339"

	NUM_VALIDATORS_TO_ADD = 4
	// the maximal number of validator queries to add in a single block
	// reduce this if you get errors about exceeding the block gas limit
	BATCH_SIZE = 30
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

func fetch_min_icq_deposit() (int, error) {
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
		return 0, fmt.Errorf("failed to execute command: %v\nOutput: %s", err, string(output))
	}

	// Parse the JSON output
	var paramsResponse ICQParamsResponse
	err = json.Unmarshal(output, &paramsResponse)
	if err != nil {
		return 0, fmt.Errorf("error decoding JSON: %v", err)
	}

	// Extract the query_deposit amount
	if len(paramsResponse.Params.QueryDeposit) == 0 {
		return 0, fmt.Errorf("query_deposit is empty")
	}

	deposit := paramsResponse.Params.QueryDeposit[0]

	// Convert the amount to an integer
	amount, ok := new(big.Int).SetString(deposit.Amount, 10)
	if !ok {
		return 0, fmt.Errorf("failed to parse amount: %s", deposit.Amount)
	}

	return int(amount.Int64()), nil
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
	// Split validators into batches of BATCH_SIZE
	batches := splitIntoBatches(validators, BATCH_SIZE)

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
			"--amount", fmt.Sprintf("%d%s", minICQDeposit*len(batch), "untrn"),
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

	// Take the top NUM_VALIDATORS_TO_ADD validators
	topValidators := response.Validators
	if len(topValidators) > NUM_VALIDATORS_TO_ADD {
		topValidators = topValidators[:NUM_VALIDATORS_TO_ADD]
	}

	return topValidators, nil
}

// Struct to hold the response from the Hydro contract
type RegisteredValidatorQueriesResponse struct {
	Data struct {
		QueryIDs [][]interface{} `json:"query_ids"`
	} `json:"data"`
}

// Function to query Hydro validators from a CosmWasm contract
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

	// Print the response body for debugging
	fmt.Printf("Response Body: %s\n", string(body))

	// Parse the JSON response
	var result RegisteredValidatorQueriesResponse
	err = json.Unmarshal(body, &result)
	if err != nil {
		return nil, fmt.Errorf("error decoding JSON: %v", err)
	}

	// Extract validator addresses
	var validatorAddresses []string
	for _, item := range result.Data.QueryIDs {
		if len(item) != 2 {
			return nil, fmt.Errorf("unexpected item length: expected 2, got %d", len(item))
		}

		// Extract the validator address
		validatorAddress, ok := item[0].(string)
		if !ok {
			return nil, fmt.Errorf("expected validator address to be a string")
		}

		// Extract the query ID (if needed)
		queryIDFloat, ok := item[1].(float64)
		if !ok {
			return nil, fmt.Errorf("expected query ID to be a number")
		}
		queryID := uint64(queryIDFloat)

		// Append the validator address to the list
		validatorAddresses = append(validatorAddresses, validatorAddress)

		// Use queryID if needed
		_ = queryID
	}

	return validatorAddresses, nil
}

func main() {
	// Query Cosmos Hub validators
	hubValidators, err := query_hub_validators()
	if err != nil {
		log.Fatalf("Error querying hub validators: %v", err)
	}

	// Print the top NUM_VALIDATORS_TO_ADD Cosmos Hub validators
	fmt.Printf("Top %d Cosmos Hub Validators:", NUM_VALIDATORS_TO_ADD)
	for idx, validator := range hubValidators {
		fmt.Printf("%d: OperatorAddress: %s, Tokens: %s, Moniker: %s\n",
			idx+1, validator.OperatorAddress, validator.Tokens, validator.Description.Moniker)
	}

	fmt.Println()

	// Query Hydro validators
	neutronValidators, err := query_hydro_validators(HYDRO_CONTRACT_ADDRESS)
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

	if len(diffValidators) == 0 {
		fmt.Println("No validators to add")
		return
	}

	// Add validator queries in batches
	err = add_validator_queries(diffValidators, HYDRO_CONTRACT_ADDRESS)
	if err != nil {
		log.Fatalf("Error adding validator queries: %v", err)
	}
}
