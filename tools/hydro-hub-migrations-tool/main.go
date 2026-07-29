package main

import (
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"log"
	"net/http"
	"net/url"
	"os"
	"os/exec"
	"strings"
	"time"

	"github.com/btcsuite/btcd/btcutil/bech32"
)

// --- types ---

type Coin struct {
	Denom  string `json:"denom"`
	Amount string `json:"amount"`
}

type LockEntry struct {
	LockID               uint64 `json:"lock_id"`
	Owner                string `json:"owner"`
	OwnerHub             string `json:"owner_hub"`
	IsSmartContractOwner bool   `json:"is_smart_contract_owner"`
	Funds                Coin   `json:"funds"`
	DenomNeutron         string `json:"denom_neutron"`
	LockStart            string `json:"lock_start"`
	LockEnd              string `json:"lock_end"`
}

type allLockupsArgs struct {
	StartLockID *uint64 `json:"start_lock_id"`
	Limit       uint64  `json:"limit"`
}

type allLockupsMsg struct {
	AllLockups allLockupsArgs `json:"all_lockups"`
}

type allLockupsResponse struct {
	Lockups    []LockEntry `json:"lockups"`
	NextLockID *uint64     `json:"next_lock_id"`
}

type allAvailableConversionFundsArgs struct {
	StartAfter *string `json:"start_after"`
	Limit      *uint32 `json:"limit"`
}

type allAvailableConversionFundsMsg struct {
	AllAvailableConversionFunds allAvailableConversionFundsArgs `json:"all_available_conversion_funds"`
}

type conversionFundInfo struct {
	Denom               string `json:"denom"`
	Amount              string `json:"amount"`
	Ratio               string `json:"ratio"`
	BaseTokenEquivalent string `json:"base_token_equivalent"`
}

type allAvailableConversionFundsResponse struct {
	Funds                    []conversionFundInfo `json:"funds"`
	TotalBaseTokenEquivalent string               `json:"total_base_token_equivalent"`
	HasMore                  bool                 `json:"has_more"`
}

type smartQueryEnvelope struct {
	Data json.RawMessage `json:"data"`
}

type rawQueryEnvelope struct {
	Data string `json:"data"`
}

type denomHop struct {
	PortID    string `json:"port_id"`
	ChannelID string `json:"channel_id"`
}

type denomInfo struct {
	Base  string     `json:"base"`
	Trace []denomHop `json:"trace"`
}

type pageResponse struct {
	NextKey string `json:"next_key"`
	Total   string `json:"total"`
}

type denomsResponse struct {
	Denoms     []denomInfo  `json:"denoms"`
	Pagination pageResponse `json:"pagination"`
}

// lockupToMint mirrors the Rust LockupToMint type used by ExecuteMsg::MintLockups.
type lockupToMint struct {
	LockID    uint64 `json:"lock_id"`
	Owner     string `json:"owner"`
	Funds     Coin   `json:"funds"`
	LockStart string `json:"lock_start"`
	LockEnd   string `json:"lock_end"`
}

type mintLockupsArgs struct {
	Lockups []lockupToMint `json:"lockups"`
}

type mintLockupsMsg struct {
	MintLockups mintLockupsArgs `json:"mint_lockups"`
}

type gasPrice struct {
	Denom  string `json:"denom"`
	Amount string `json:"amount"`
}

type gasPriceResponse struct {
	Price gasPrice `json:"price"`
}

type txResponse struct {
	TxHash string `json:"txhash"`
	Code   int    `json:"code"`
	RawLog string `json:"raw_log"`
}

// --- constants ---

const CmdExportHydroState = "export-hydro-state"
const CmdMintHubLockups = "mint-hub-lockups"

// All Hydro paginated queries are limited to 100 items per page
const HydroQueryPageLimit = 100

const gaiadBinary = "gaiad"
const uatomDenom = "uatom"
const testKeyringBackend = "test"

const stAtomNeutronDenom = "ibc/B7864B03E1B9FD4F049243E92ABD691586F682137037A9F3FCA5222815620B3C"
const dAtomNeutronDenom = "factory/neutron1k6hr0f83e7un2wjf29cspk7j69jrnskk65k3ek2nj9dztrlzpj6q00rtsa/udatom"

const stAtomHubIBCDenom = "ibc/B05539B66B72E2739B986B86391E5D08F12B8D5D2C2A7F8F8CF9ADF674DFA231"
const dAtomHubIBCDenom = "ibc/AFC2F1B2FD45D549E34445E63921ECDECF1EAC68DA72412C2E087BEB503693F2"

const hubAddressPrefix = "cosmos"
const hubValoperPrefix = "cosmosvaloper"

const transferPort = "transfer"
const hubChannelID = "channel-1"
const strideChannelID = "channel-8"

// neutronWalletAddrLen is the bech32 string length of a standard Neutron
// wallet address (20-byte hash). Smart contract addresses use a 32-byte
// hash and are therefore longer.
const neutronWalletAddrLen = 46

// --- helpers ---

func getJSON(url string, target interface{}) error {
	resp, err := http.Get(url)
	if err != nil {
		return fmt.Errorf("GET %s: %w", url, err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		body, _ := io.ReadAll(resp.Body)
		return fmt.Errorf("GET %s: status %s, body: %s", url, resp.Status, string(body))
	}
	return json.NewDecoder(resp.Body).Decode(target)
}

// fetchNextLockID performs a RawQuery for the LOCK_ID Item<u64> and returns its value.
func fetchNextLockID(node, contract string) (uint64, error) {
	return queryRawU64Field(node, contract, "lock_id")
}

// fetchNextPropID performs a RawQuery for the PROP_ID Item<u64> and returns its value.
func fetchNextPropID(node, contract string) (uint64, error) {
	return queryRawU64Field(node, contract, "prop_id")
}

func queryRawU64Field(node, contract, field string) (uint64, error) {
	key := base64.StdEncoding.EncodeToString([]byte(field))
	url := fmt.Sprintf("%s/cosmwasm/wasm/v1/contract/%s/raw/%s", node, contract, key)

	var envelope rawQueryEnvelope
	if err := getJSON(url, &envelope); err != nil {
		return 0, fmt.Errorf("raw query %s: %w", field, err)
	}

	valueBytes, err := base64.StdEncoding.DecodeString(envelope.Data)
	if err != nil {
		return 0, fmt.Errorf("decode %s data: %w", field, err)
	}

	var lockID uint64
	if err := json.Unmarshal(valueBytes, &lockID); err != nil {
		return 0, fmt.Errorf("unmarshal %s: %w", field, err)
	}
	return lockID, nil
}

// queryAllLockups pages through AllLockups until no next_lock_id is returned.
func queryAllLockups(node, contract string, limit uint64) ([]LockEntry, error) {
	var allLockups []LockEntry
	var startLockID *uint64
	page := 0

	for {
		page++

		msg := allLockupsMsg{
			AllLockups: allLockupsArgs{
				StartLockID: startLockID,
				Limit:       limit,
			},
		}
		msgBytes, err := json.Marshal(msg)
		if err != nil {
			return nil, fmt.Errorf("marshal AllLockups query: %w", err)
		}

		encoded := base64.StdEncoding.EncodeToString(msgBytes)
		url := fmt.Sprintf("%s/cosmwasm/wasm/v1/contract/%s/smart/%s", node, contract, encoded)

		var envelope smartQueryEnvelope
		if err := getJSON(url, &envelope); err != nil {
			return nil, fmt.Errorf("AllLockups page %d: %w", page, err)
		}

		var result allLockupsResponse
		if err := json.Unmarshal(envelope.Data, &result); err != nil {
			return nil, fmt.Errorf("unmarshal AllLockups page %d: %w", page, err)
		}

		allLockups = append(allLockups, result.Lockups...)

		if result.NextLockID == nil {
			break
		}

		startLockID = result.NextLockID
	}

	return allLockups, nil
}

// queryAllAvailableConversionFunds pages through AllAvailableConversionFunds
// until has_more is false, keeping only entries with a non-zero amount and
// resolving each kept entry's denom to its Hub-native equivalent.
func queryAllAvailableConversionFunds(node, contract string, limit uint32, ibcDenomMap map[string]string) ([]conversionFundInfo, error) {
	var conversionFunds []conversionFundInfo
	var startAfter *string
	page := 0

	for {
		page++

		msg := allAvailableConversionFundsMsg{
			AllAvailableConversionFunds: allAvailableConversionFundsArgs{
				StartAfter: startAfter,
				Limit:      &limit,
			},
		}
		msgBytes, err := json.Marshal(msg)
		if err != nil {
			return nil, fmt.Errorf("marshal AllAvailableConversionFunds query: %w", err)
		}

		encoded := base64.StdEncoding.EncodeToString(msgBytes)
		url := fmt.Sprintf("%s/cosmwasm/wasm/v1/contract/%s/smart/%s", node, contract, encoded)

		var envelope smartQueryEnvelope
		if err := getJSON(url, &envelope); err != nil {
			return nil, fmt.Errorf("AllAvailableConversionFunds page %d: %w", page, err)
		}

		var result allAvailableConversionFundsResponse
		if err := json.Unmarshal(envelope.Data, &result); err != nil {
			return nil, fmt.Errorf("unmarshal AllAvailableConversionFunds page %d: %w", page, err)
		}

		var lastDenom string
		for _, fund := range result.Funds {
			lastDenom = fund.Denom
			if fund.Amount == "0" {
				continue
			}

			hubDenom, err := resolveDenom(fund.Denom, ibcDenomMap)
			if err != nil {
				return nil, fmt.Errorf("resolving denom for conversion fund %q: %w", fund.Denom, err)
			}
			fund.Denom = hubDenom
			conversionFunds = append(conversionFunds, fund)
		}

		if !result.HasMore {
			break
		}

		startAfter = &lastDenom
	}

	return conversionFunds, nil
}

// fetchAllIbcDenoms pages through the ibc-transfer module's Denoms query, collecting
// every known IBC denom trace on the chain. This lets us resolve all lockup denoms
// with a single set of paginated requests instead of one request per unique denom.
func fetchAllIbcDenoms(node string, pageLimit uint64) ([]denomInfo, error) {
	var all []denomInfo
	nextKey := ""
	page := 0

	for {
		page++

		reqURL := fmt.Sprintf("%s/ibc/apps/transfer/v1/denoms?pagination.limit=%d", node, pageLimit)
		if nextKey != "" {
			reqURL += "&pagination.key=" + url.QueryEscape(nextKey)
		}

		var resp denomsResponse
		if err := getJSON(reqURL, &resp); err != nil {
			return nil, fmt.Errorf("Denoms page %d: %w", page, err)
		}

		all = append(all, resp.Denoms...)

		if resp.Pagination.NextKey == "" {
			break
		}
		nextKey = resp.Pagination.NextKey
	}

	return all, nil
}

// hashDenomTrace computes the "ibc/{HASH}" denom for a denom trace the same way
// ibc-go does: sha256 of the hops joined as "port_id/channel_id/.../base", hex-encoded
// in uppercase.
func hashDenomTrace(d denomInfo) string {
	var sb strings.Builder
	for _, hop := range d.Trace {
		sb.WriteString(hop.PortID)
		sb.WriteByte('/')
		sb.WriteString(hop.ChannelID)
		sb.WriteByte('/')
	}
	sb.WriteString(d.Base)

	hash := sha256.Sum256([]byte(sb.String()))
	return "ibc/" + strings.ToUpper(hex.EncodeToString(hash[:]))
}

func buildIbcDenomMap(denoms []denomInfo) map[string]string {
	denomMap := make(map[string]string)

	isLsmShare := func(d denomInfo) bool {
		return strings.HasPrefix(d.Base, hubValoperPrefix) && len(d.Trace) == 1 && d.Trace[0].PortID == transferPort && d.Trace[0].ChannelID == hubChannelID
	}

	isStAtom := func(d denomInfo) bool {
		return d.Base == "stuatom" && len(d.Trace) == 1 && d.Trace[0].PortID == transferPort && d.Trace[0].ChannelID == strideChannelID
	}

	for _, d := range denoms {
		var resolved string

		switch {
		case isStAtom(d):
			// stATOM: replace with its Hub IBC denom on the Hub chain.
			resolved = stAtomHubIBCDenom
		case isLsmShare(d):
			// LSM share: base is "<cosmosvaloper...>/<shareID>", which is exactly the native denom format on the Hub.
			resolved = d.Base
		default:
			// We are only interested in stATOM and LSM share denoms, so skip everything else.
			continue
		}

		denomMap[hashDenomTrace(d)] = resolved
	}

	return denomMap
}

// resolveDenom converts an on-Neutron denom to its Hub-native equivalent, using the
// prebuilt denom map so no additional network requests are needed per lockup.
func resolveDenom(denom string, ibcDenomMap map[string]string) (string, error) {
	// dATOM is Neutron native, so we replace it with its Hub IBC denom.
	if denom == dAtomNeutronDenom {
		return dAtomHubIBCDenom, nil
	}

	if !strings.HasPrefix(denom, "ibc/") {
		return "", fmt.Errorf("unexpected non-IBC denom: %s", denom)
	}

	resolved, ok := ibcDenomMap[denom]
	if !ok {
		return "", fmt.Errorf("unrecognized IBC denom: %s", denom)
	}
	return resolved, nil
}

// convertNeutronToHubAddress re-encodes a Neutron bech32 address with the
// Cosmos Hub prefix. Neutron and Hub addresses share the same underlying
// bytes, so only the bech32 human-readable part changes.
func convertNeutronToHubAddress(neutronAddr string) (string, error) {
	_, data, err := bech32.Decode(neutronAddr)
	if err != nil {
		return "", fmt.Errorf("bech32 decode %q: %w", neutronAddr, err)
	}
	hubAddr, err := bech32.Encode(hubAddressPrefix, data)
	if err != nil {
		return "", fmt.Errorf("bech32 encode %q: %w", neutronAddr, err)
	}
	return hubAddr, nil
}

// extractJSONObject scans CLI output for the first top-level '{' and returns
// everything from there onward, discarding any warnings the CLI printed first.
func extractJSONObject(output string) string {
	idx := strings.Index(output, "{")
	if idx == -1 {
		return output
	}
	return output[idx:]
}

// fetchGasPrice queries the current uatom gas price via the feemarket module.
func fetchGasPrice(node string) (string, error) {
	cmd := exec.Command(gaiadBinary, "q", "feemarket", "gas-price", uatomDenom, "--node", node, "-o", "json")
	output, err := cmd.CombinedOutput()
	if err != nil {
		return "", fmt.Errorf("fetch gas price: %w, output: %s", err, string(output))
	}

	var resp gasPriceResponse
	if err := json.Unmarshal([]byte(extractJSONObject(string(output))), &resp); err != nil {
		return "", fmt.Errorf("unmarshal gas price response: %w", err)
	}
	if resp.Price.Denom != uatomDenom {
		return "", fmt.Errorf("unexpected gas price denom in response: %+v", resp)
	}
	return resp.Price.Amount, nil
}

// chunkLockEntries splits entries into chunks of at most chunkSize.
func chunkLockEntries(entries []LockEntry, chunkSize int) [][]LockEntry {
	var chunks [][]LockEntry
	for chunkSize < len(entries) {
		entries, chunks = entries[chunkSize:], append(chunks, entries[0:chunkSize:chunkSize])
	}
	chunks = append(chunks, entries)
	return chunks
}

// toLockupsToMint converts LockEntry records (as produced by query-current-lockups)
// into the LockupToMint shape expected by ExecuteMsg::MintLockups.
func toLockupsToMint(entries []LockEntry) []lockupToMint {
	lockups := make([]lockupToMint, 0, len(entries))
	for _, entry := range entries {
		lockups = append(lockups, lockupToMint{
			LockID:    entry.LockID,
			Owner:     entry.OwnerHub,
			Funds:     entry.Funds,
			LockStart: entry.LockStart,
			LockEnd:   entry.LockEnd,
		})
	}
	return lockups
}

// broadcastMintLockupsTx sends a single MintLockups tx for the given chunk and returns the resulting tx hash.
func broadcastMintLockupsTx(chunk []LockEntry, contract, chainID, node, nodeHome, wallet, gasAdjustment, gasPriceAmount string) (string, error) {
	msg := mintLockupsMsg{MintLockups: mintLockupsArgs{Lockups: toLockupsToMint(chunk)}}
	msgBytes, err := json.Marshal(msg)
	if err != nil {
		return "", fmt.Errorf("marshal MintLockups message: %w", err)
	}

	cmdArgs := []string{
		"tx", "wasm", "execute", contract, string(msgBytes),
		"--chain-id", chainID,
		"--gas", "auto",
		"--gas-adjustment", gasAdjustment,
		"--gas-prices", fmt.Sprintf("%s%s", gasPriceAmount, uatomDenom),
		"--node", node,
		"--from", wallet,
		"--keyring-backend", testKeyringBackend,
		"-y",
		"--output", "json",
	}
	if nodeHome != "" {
		cmdArgs = append(cmdArgs, "--home", nodeHome)
	}

	cmd := exec.Command(gaiadBinary, cmdArgs...)
	output, err := cmd.CombinedOutput()
	if err != nil {
		return "", fmt.Errorf("broadcast MintLockups tx: %w, output: %s", err, string(output))
	}

	var resp txResponse
	if err := json.Unmarshal([]byte(extractJSONObject(string(output))), &resp); err != nil {
		return "", fmt.Errorf("unmarshal broadcast response: %w, output: %s", err, string(output))
	}
	if resp.Code != 0 {
		return "", fmt.Errorf("broadcast rejected (code %d): %s", resp.Code, resp.RawLog)
	}
	if resp.TxHash == "" {
		return "", fmt.Errorf("broadcast response missing txhash, output: %s", string(output))
	}
	return resp.TxHash, nil
}

// waitForTx polls until the given tx is indexed and confirms it succeeded.
func waitForTx(txHash, node, nodeHome string) error {
	time.Sleep(6 * time.Second)

	cmdArgs := []string{"q", "tx", txHash, "--node", node, "--output", "json"}
	if nodeHome != "" {
		cmdArgs = append(cmdArgs, "--home", nodeHome)
	}

	const maxAttempts = 5
	var lastErr error
	for attempt := 1; attempt <= maxAttempts; attempt++ {
		cmd := exec.Command(gaiadBinary, cmdArgs...)
		output, err := cmd.CombinedOutput()
		if err != nil {
			lastErr = fmt.Errorf("query tx %s: %w, output: %s", txHash, err, string(output))
			time.Sleep(1 * time.Second)
			continue
		}

		var resp txResponse
		if err := json.Unmarshal([]byte(extractJSONObject(string(output))), &resp); err != nil {
			return fmt.Errorf("unmarshal tx query response for %s: %w", txHash, err)
		}
		if resp.Code != 0 {
			return fmt.Errorf("tx %s failed (code %d): %s", txHash, resp.Code, resp.RawLog)
		}
		return nil
	}
	return fmt.Errorf("tx %s not confirmed after %d attempts: %w", txHash, maxAttempts, lastErr)
}

// --- main ---

func main() {
	if len(os.Args) < 2 {
		printUsage()
		os.Exit(1)
	}

	switch os.Args[1] {
	case CmdExportHydroState:
		runExportHydroState(os.Args[2:])
	case CmdMintHubLockups:
		runMintHubLockups(os.Args[2:])
	default:
		fmt.Fprintf(os.Stderr, "Unknown command: %s\n\n", os.Args[1])
		printUsage()
		os.Exit(1)
	}
}

func printUsage() {
	fmt.Fprintln(os.Stderr, "Usage: hydro-hub-migrations-tool <command> [flags]")
	fmt.Fprintln(os.Stderr, "\nCommands:")
	fmt.Fprintln(os.Stderr, "  query-current-lockups   Fetch all current lockups from the Neutron Hydro contract")
	fmt.Fprintln(os.Stderr, "  mint-hub-lockups        Mint historical lockups on the Cosmos Hub Hydro contract")
}

func runExportHydroState(args []string) {
	fs := flag.NewFlagSet(CmdExportHydroState, flag.ExitOnError)
	contract := fs.String("contract", "", "Neutron Hydro contract address (required)")
	node := fs.String("node", "", "Neutron LCD REST endpoint (required)")
	output := fs.String("output", "lockups.json", "Output JSON file path")
	availableConversionFundsOutput := fs.String("available-conversion-funds-output", "available_conversion_funds.json", "Output JSON file path for available conversion funds")
	fs.Parse(args)

	if *node == "" {
		log.Fatal("--node is required")
	}
	if *contract == "" {
		log.Fatal("--contract is required")
	}

	fmt.Printf("Contract: %s\n", *contract)
	fmt.Printf("Node:     %s\n", *node)
	fmt.Printf("Output:   %s\n\n", *output)

	// Step 1: read the current lock_id and prop_id counters from raw storage.
	nextLockID, err := fetchNextLockID(*node, *contract)
	if err != nil {
		log.Fatalf("Error fetching next lock_id: %v", err)
	}
	nextPropID, err := fetchNextPropID(*node, *contract)
	if err != nil {
		log.Fatalf("Error fetching next prop_id: %v", err)
	}

	// Step 2: paginate through all lockup entries.
	fmt.Println("Fetching lockups from Hydro contract...")
	initialLockups, err := queryAllLockups(*node, *contract, HydroQueryPageLimit)
	if err != nil {
		log.Fatalf("Error fetching lockups: %v", err)
	}

	// Step 3: fetch all known IBC denom traces once and build a resolution map.
	fmt.Println("Resolving lockup denoms...")
	allIbcDenoms, err := fetchAllIbcDenoms(*node, 10000)
	if err != nil {
		log.Fatalf("Error fetching denoms: %v", err)
	}

	ibcDenomMap := buildIbcDenomMap(allIbcDenoms)

	// Step 4: resolve every denom to its Hub-native equivalent.
	lockups := make([]LockEntry, 0, len(initialLockups))
	for _, entry := range initialLockups {
		hubDenom, err := resolveDenom(entry.Funds.Denom, ibcDenomMap)
		if err != nil {
			log.Fatalf("Error resolving denom for lock_id %d: %v", entry.LockID, err)
		}

		// Store the original Neutron denom for reference
		entry.DenomNeutron = entry.Funds.Denom
		entry.Funds.Denom = hubDenom
		entry.IsSmartContractOwner = len(entry.Owner) != neutronWalletAddrLen

		// Only convert EOA addresses automatically. Smart contract addresses will be provided separately by the owning teams.
		if !entry.IsSmartContractOwner {
			hubAddr, err := convertNeutronToHubAddress(entry.Owner)
			if err != nil {
				log.Fatalf("Error converting owner address for lock_id %d: %v", entry.LockID, err)
			}
			entry.OwnerHub = hubAddr
		}

		lockups = append(lockups, entry)
	}

	// Step 5: write the output JSON file.
	outBytes, err := json.MarshalIndent(lockups, "", "  ")
	if err != nil {
		log.Fatalf("Error marshaling output: %v", err)
	}
	if err := os.WriteFile(*output, outBytes, 0644); err != nil {
		log.Fatalf("Error writing %s: %v", *output, err)
	}

	// Step 6: fetch available conversion funds and write them out.
	conversionFunds, err := queryAllAvailableConversionFunds(*node, *contract, HydroQueryPageLimit, ibcDenomMap)
	if err != nil {
		log.Fatalf("Error fetching available conversion funds: %v", err)
	}

	conversionFundsBytes, err := json.MarshalIndent(conversionFunds, "", "  ")
	if err != nil {
		log.Fatalf("Error marshaling available conversion funds: %v", err)
	}
	if err := os.WriteFile(*availableConversionFundsOutput, conversionFundsBytes, 0644); err != nil {
		log.Fatalf("Error writing %s: %v", *availableConversionFundsOutput, err)
	}

	fmt.Printf("\nTotal lockups: %d\n", len(lockups))
	fmt.Printf("Written to:    %s\n", *output)
	fmt.Printf("\nAvailable conversion funds tokens: %d\n", len(conversionFunds))
	fmt.Printf("Written to:    %s\n", *availableConversionFundsOutput)
	fmt.Printf("\nNext lock ID:      %d\n", nextLockID)
	fmt.Printf("Next proposal ID:  %d\n", nextPropID)
}

func runMintHubLockups(args []string) {
	fs := flag.NewFlagSet(CmdMintHubLockups, flag.ExitOnError)
	inputJsonPath := fs.String("input-json-path", "", "Path to the JSON file produced by query-current-lockups (required)")
	chunkSize := fs.Int("chunk-size", 100, "Number of lockups to include per MintLockups transaction")
	wallet := fs.String("wallet", "", "Wallet name (keyring-backend test) to sign transactions with (required)")
	contract := fs.String("contract", "", "Cosmos Hub Hydro contract address (required)")
	chainID := fs.String("chain-id", "cosmoshub-4", "Cosmos Hub chain ID")
	hubNode := fs.String("hub-node", "", "Cosmos Hub RPC node endpoint (required)")
	hubNodeHome := fs.String("hub-node-home", "", "Optional gaiad --home directory")
	gasAdjustment := fs.String("gas-adjustment", "1.5", "Gas adjustment used for the transactions")
	fs.Parse(args)

	if *inputJsonPath == "" {
		log.Fatal("--input-json-path is required")
	}
	if *wallet == "" {
		log.Fatal("--wallet is required")
	}
	if *contract == "" {
		log.Fatal("--contract is required")
	}
	if *hubNode == "" {
		log.Fatal("--hub-node is required")
	}
	if *chunkSize <= 0 {
		log.Fatal("--chunk-size must be greater than 0")
	}

	fmt.Printf("Input:          %s\n", *inputJsonPath)
	fmt.Printf("Contract:       %s\n", *contract)
	fmt.Printf("Chain ID:       %s\n", *chainID)
	fmt.Printf("Hub node:       %s\n", *hubNode)
	fmt.Printf("Wallet:         %s\n", *wallet)
	fmt.Printf("Chunk size:     %d\n", *chunkSize)
	fmt.Printf("Gas adjustment: %s\n\n", *gasAdjustment)

	inputBytes, err := os.ReadFile(*inputJsonPath)
	if err != nil {
		log.Fatalf("Error reading %s: %v", *inputJsonPath, err)
	}
	var entries []LockEntry
	if err := json.Unmarshal(inputBytes, &entries); err != nil {
		log.Fatalf("Error unmarshaling %s: %v", *inputJsonPath, err)
	}

	var missingOwnerHub []uint64
	for _, entry := range entries {
		if entry.OwnerHub == "" {
			missingOwnerHub = append(missingOwnerHub, entry.LockID)
		}
	}
	if len(missingOwnerHub) > 0 {
		log.Fatalf("Aborting: %d lockup(s) missing owner_hub, fill these in before minting: %v", len(missingOwnerHub), missingOwnerHub)
	}

	gasPriceAmount, err := fetchGasPrice(*hubNode)
	if err != nil {
		log.Fatalf("Error fetching gas price: %v", err)
	}
	fmt.Printf("Gas price:      %s%s\n\n", gasPriceAmount, uatomDenom)

	chunks := chunkLockEntries(entries, *chunkSize)
	fmt.Printf("Total lockups: %d, in %d chunk(s)\n\n", len(entries), len(chunks))

	for i, chunk := range chunks {
		firstLockID := chunk[0].LockID
		lastLockID := chunk[len(chunk)-1].LockID
		fmt.Printf("Chunk %d/%d: lock_ids %d..%d (%d lockups)\n", i+1, len(chunks), firstLockID, lastLockID, len(chunk))

		txHash, err := broadcastMintLockupsTx(chunk, *contract, *chainID, *hubNode, *hubNodeHome, *wallet, *gasAdjustment, gasPriceAmount)
		if err != nil {
			log.Fatalf("Error broadcasting chunk %d/%d: %v", i+1, len(chunks), err)
		}
		fmt.Printf("  Broadcast tx: %s, waiting for confirmation...\n", txHash)

		if err := waitForTx(txHash, *hubNode, *hubNodeHome); err != nil {
			log.Fatalf("Error confirming chunk %d/%d (tx %s): %v", i+1, len(chunks), txHash, err)
		}
		fmt.Printf("  Confirmed.\n\n")
	}

	fmt.Printf("Done. Minted %d lockups across %d transaction(s).\n", len(entries), len(chunks))
}
