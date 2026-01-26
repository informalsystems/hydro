package cosmos

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"time"

	"github.com/btcsuite/btcutil/bech32"
)

// NobleAddressPrefix is the bech32 prefix for Noble addresses
const NobleAddressPrefix = "noble"

// NobleClient handles queries to the Noble blockchain via REST API
type NobleClient struct {
	apiEndpoint string
	httpClient  *http.Client
}

// NewNobleClient creates a new Noble REST API client
func NewNobleClient(apiEndpoint string) (*NobleClient, error) {
	if apiEndpoint == "" {
		return nil, fmt.Errorf("Noble API endpoint cannot be empty")
	}

	return &NobleClient{
		apiEndpoint: apiEndpoint,
		httpClient: &http.Client{
			Timeout: 10 * time.Second,
		},
	}, nil
}

// nobleForwardingResponse represents the response from Noble forwarding API
type nobleForwardingResponse struct {
	Address string `json:"address"`
}

// QueryForwardingAddress queries Noble for the forwarding address
// corresponding to a Neutron proxy address via IBC channel using REST API
//
// Parameters:
//   - channel: IBC channel between Noble and Neutron (e.g., "channel-18")
//   - recipient: Neutron proxy address (bech32 format)
//
// Returns the Noble forwarding address (bech32 format)
//
// API endpoint: GET {apiEndpoint}/forwarding/v1/address/{channel}/{recipient}
func (n *NobleClient) QueryForwardingAddress(
	ctx context.Context,
	channel string,
	recipient string,
) (string, error) {
	if channel == "" {
		return "", fmt.Errorf("channel cannot be empty")
	}
	if recipient == "" {
		return "", fmt.Errorf("recipient cannot be empty")
	}

	// Build request URL
	// Example: https://noble-api.polkachu.com/forwarding/v1/address/channel-18/neutron1...
	url := fmt.Sprintf("%s/forwarding/v1/address/%s/%s", n.apiEndpoint, channel, recipient)

	// Create HTTP request with context
	req, err := http.NewRequestWithContext(ctx, "GET", url, nil)
	if err != nil {
		return "", fmt.Errorf("failed to create request: %w", err)
	}

	// Set headers
	req.Header.Set("Accept", "application/json")

	// Execute request
	resp, err := n.httpClient.Do(req)
	if err != nil {
		return "", fmt.Errorf("failed to query Noble API: %w", err)
	}
	defer resp.Body.Close()

	// Check status code
	if resp.StatusCode != http.StatusOK {
		body, _ := io.ReadAll(resp.Body)
		return "", fmt.Errorf("Noble API returned status %d: %s", resp.StatusCode, string(body))
	}

	// Read response body
	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return "", fmt.Errorf("failed to read response body: %w", err)
	}

	// Parse JSON response
	var result nobleForwardingResponse
	if err := json.Unmarshal(body, &result); err != nil {
		return "", fmt.Errorf("failed to parse response: %w", err)
	}

	if result.Address == "" {
		return "", fmt.Errorf("empty address in response")
	}

	return result.Address, nil
}

// ConvertToBytes32 converts a Noble bech32 address to bytes32 format for EVM
//
// Steps:
// 1. Decode bech32 address to get raw bytes
// 2. Convert from 5-bit to 8-bit encoding
// 3. Hex encode
// 4. Pad to 32 bytes
//
// Parameters:
//   - nobleAddr: Noble address in bech32 format (e.g., "noble1...")
//
// Returns a 32-byte array suitable for EVM contract parameters
func ConvertToBytes32(nobleAddr string) ([32]byte, error) {
	var result [32]byte

	if nobleAddr == "" {
		return result, fmt.Errorf("noble address cannot be empty")
	}

	// Decode bech32 to get 5-bit encoded data
	_, data5bit, err := bech32.Decode(nobleAddr)
	if err != nil {
		return result, fmt.Errorf("failed to decode bech32 address: %w", err)
	}

	// Convert from 5-bit to 8-bit encoding
	data8bit, err := bech32.ConvertBits(data5bit, 5, 8, false)
	if err != nil {
		return result, fmt.Errorf("failed to convert address bits: %w", err)
	}

	// Hex encode the raw bytes
	hexStr := hex.EncodeToString(data8bit)

	// Pad to 64 characters (32 bytes)
	if len(hexStr) > 64 {
		return result, fmt.Errorf("address too long: %d hex chars (max 64)", len(hexStr))
	}

	// Left-pad with zeros
	paddedHex := fmt.Sprintf("%064s", hexStr)

	// Decode hex string to bytes
	bytes, err := hex.DecodeString(paddedHex)
	if err != nil {
		return result, fmt.Errorf("failed to decode hex string: %w", err)
	}

	// Copy to fixed-size array
	copy(result[:], bytes)

	return result, nil
}

// ConvertBytes32ToHex converts a [32]byte array to hex string with 0x prefix
func ConvertBytes32ToHex(bytes32 [32]byte) string {
	return "0x" + hex.EncodeToString(bytes32[:])
}

// Close closes the HTTP client (no-op since HTTP client doesn't need closing)
func (n *NobleClient) Close() error {
	// HTTP client doesn't require explicit closing
	// Connections are managed automatically by the transport
	return nil
}

// ComputeNobleForwardingAddress computes the deterministic Noble forwarding account address
// that will forward tokens to the given destination address via the specified IBC channel.
//
// Noble forwarding accounts are derived using:
// address = sha256("forwarding" | channel | recipient)[:20]
//
// Parameters:
//   - channel: IBC channel ID (e.g., "channel-18" for Noble -> Neutron)
//   - recipient: Destination address on the target chain (e.g., Neutron proxy address)
//
// Returns the bech32-encoded Noble address (noble1...)
func ComputeNobleForwardingAddress(channel string, recipient string) (string, error) {
	if channel == "" {
		return "", fmt.Errorf("channel cannot be empty")
	}
	if recipient == "" {
		return "", fmt.Errorf("recipient cannot be empty")
	}

	// Build the preimage: "forwarding" + channel + recipient
	preimage := append([]byte("forwarding"), []byte(channel)...)
	preimage = append(preimage, []byte(recipient)...)

	// Hash and take first 20 bytes
	hash := sha256.Sum256(preimage)
	addressBytes := hash[:20]

	// Convert to bech32
	conv, err := bech32.ConvertBits(addressBytes, 8, 5, true)
	if err != nil {
		return "", fmt.Errorf("failed to convert bits for bech32: %w", err)
	}

	address, err := bech32.Encode(NobleAddressPrefix, conv)
	if err != nil {
		return "", fmt.Errorf("failed to encode bech32 address: %w", err)
	}

	return address, nil
}

// ComputeNobleForwardingAddressForProxy computes the Noble forwarding address
// that forwards to a user's Neutron proxy contract.
//
// This is a convenience function that combines proxy address computation
// with Noble forwarding address computation.
func ComputeNobleForwardingAddressForProxy(
	proxyCodeID uint64,
	operatorAddress string,
	userEmail string,
	nobleChannel string,
) (string, error) {
	// First compute the Neutron proxy address
	proxyAddress, err := ComputeProxyAddress(proxyCodeID, operatorAddress, userEmail)
	if err != nil {
		return "", fmt.Errorf("failed to compute proxy address: %w", err)
	}

	// Then compute the Noble forwarding address that forwards to this proxy
	forwardingAddress, err := ComputeNobleForwardingAddress(nobleChannel, proxyAddress)
	if err != nil {
		return "", fmt.Errorf("failed to compute forwarding address: %w", err)
	}

	return forwardingAddress, nil
}
