package main

import (
	"encoding/base64"
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"log"
	"net/http"
	"os"
	"strings"

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

type denomResponse struct {
	Denom denomInfo `json:"denom"`
}

// --- constants ---

const CmdQueryCurrentLockups = "query-current-lockups"

const dAtomNeutronDenom = "factory/neutron1k6hr0f83e7un2wjf29cspk7j69jrnskk65k3ek2nj9dztrlzpj6q00rtsa/udatom"

const stAtomHubIBCDenom = "ibc/B05539B66B72E2739B986B86391E5D08F12B8D5D2C2A7F8F8CF9ADF674DFA231"
const dAtomHubIBCDenom = "ibc/AFC2F1B2FD45D549E34445E63921ECDECF1EAC68DA72412C2E087BEB503693F2"

const hubAddressPrefix = "cosmos"

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
	key := base64.StdEncoding.EncodeToString([]byte("lock_id"))
	url := fmt.Sprintf("%s/cosmwasm/wasm/v1/contract/%s/raw/%s", node, contract, key)

	var envelope rawQueryEnvelope
	if err := getJSON(url, &envelope); err != nil {
		return 0, fmt.Errorf("raw query lock_id: %w", err)
	}

	valueBytes, err := base64.StdEncoding.DecodeString(envelope.Data)
	if err != nil {
		return 0, fmt.Errorf("decode lock_id data: %w", err)
	}

	var lockID uint64
	if err := json.Unmarshal(valueBytes, &lockID); err != nil {
		return 0, fmt.Errorf("unmarshal lock_id: %w", err)
	}
	return lockID, nil
}

// queryAllLockups pages through AllLockups until no next_lock_id is returned.
func queryAllLockups(node, contract string, limit uint64) ([]LockEntry, error) {
	var all []LockEntry
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

		fmt.Printf("Fetched page %d, got %d lockups\n", page, len(result.Lockups))
		all = append(all, result.Lockups...)

		if result.NextLockID == nil {
			break
		}
		startLockID = result.NextLockID
	}
	return all, nil
}

// resolveDenom converts an on-Neutron denom to its Hub-native equivalent.
// Results are cached to avoid redundant IBC trace queries.
func resolveDenom(node, denom string, cache map[string]string) (string, error) {
	// dATOM is Neutron native, so we replace it with its Hub IBC denom.
	if denom == dAtomNeutronDenom {
		return dAtomHubIBCDenom, nil
	}

	if !strings.HasPrefix(denom, "ibc/") {
		return "", fmt.Errorf("unexpected non-IBC denom: %s", denom)
	}

	if resolved, ok := cache[denom]; ok {
		return resolved, nil
	}

	hash := strings.TrimPrefix(denom, "ibc/")
	url := fmt.Sprintf("%s/ibc/apps/transfer/v1/denoms/%s", node, hash)

	var resp denomResponse
	if err := getJSON(url, &resp); err != nil {
		return "", fmt.Errorf("resolve denom trace for %s: %w", denom, err)
	}

	base := resp.Denom.Base
	var resolved string
	switch {
	case base == "stuatom":
		// stATOM: replace with its Hub IBC denom on the Hub chain.
		resolved = stAtomHubIBCDenom
	case strings.Contains(base, "/"):
		// LSM share: base is "<cosmosvaloper...>/<shareID>", which is exactly
		// the native denom format on the Hub.
		resolved = base
	default:
		return "", fmt.Errorf("unrecognized IBC denom %s (base=%s, hops_count=%d)", denom, base, len(resp.Denom.Trace))
	}

	cache[denom] = resolved
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

// --- main ---

func main() {
	if len(os.Args) < 2 {
		printUsage()
		os.Exit(1)
	}

	switch os.Args[1] {
	case CmdQueryCurrentLockups:
		runQueryCurrentLockups(os.Args[2:])
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
}

func runQueryCurrentLockups(args []string) {
	fs := flag.NewFlagSet(CmdQueryCurrentLockups, flag.ExitOnError)
	node := fs.String("node", "", "Neutron LCD REST endpoint (required)")
	contract := fs.String("contract", "", "Neutron Hydro contract address (required)")
	limit := fs.Uint64("limit", 100, "Number of lockups to fetch per AllLockups page")
	output := fs.String("output", "lockups.json", "Output JSON file path")
	fs.Parse(args)

	if *node == "" {
		log.Fatal("--node is required")
	}
	if *contract == "" {
		log.Fatal("--contract is required")
	}

	fmt.Printf("Node:     %s\n", *node)
	fmt.Printf("Contract: %s\n", *contract)
	fmt.Printf("Limit:    %d\n", *limit)
	fmt.Printf("Output:   %s\n\n", *output)

	// Step 1: read the current lock_id counter from raw storage.
	nextLockID, err := fetchNextLockID(*node, *contract)
	if err != nil {
		log.Fatalf("Error fetching next lock_id: %v", err)
	}

	// Step 2: paginate through all lockup entries.
	initialLockups, err := queryAllLockups(*node, *contract, *limit)
	if err != nil {
		log.Fatalf("Error fetching lockups: %v", err)
	}

	// Step 3: resolve every denom to its Hub-native equivalent.
	cache := make(map[string]string)
	lockups := make([]LockEntry, 0, len(initialLockups))
	for _, entry := range initialLockups {
		hubDenom, err := resolveDenom(*node, entry.Funds.Denom, cache)
		if err != nil {
			log.Fatalf("Error resolving denom for lock_id %d: %v", entry.LockID, err)
		}

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

	// Step 4: write the output JSON file.
	outBytes, err := json.MarshalIndent(lockups, "", "  ")
	if err != nil {
		log.Fatalf("Error marshaling output: %v", err)
	}
	if err := os.WriteFile(*output, outBytes, 0644); err != nil {
		log.Fatalf("Error writing %s: %v", *output, err)
	}

	fmt.Printf("\nTotal lockups: %d\n", len(lockups))
	fmt.Printf("Written to:    %s\n", *output)
	fmt.Printf("Next lock_id:  %d\n", nextLockID)
}
