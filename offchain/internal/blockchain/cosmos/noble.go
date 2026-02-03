package cosmos

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/base64"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strings"
	"time"

	"github.com/btcsuite/btcutil/bech32"
)

// NobleAddressPrefix is the bech32 prefix for Noble addresses
const NobleAddressPrefix = "noble"

// NobleClient handles queries to the Noble blockchain via RPC
type NobleClient struct {
	rpcEndpoint string
	httpClient  *http.Client
}

// NewNobleClient creates a new Noble RPC client
func NewNobleClient(rpcEndpoint string) (*NobleClient, error) {
	if rpcEndpoint == "" {
		return nil, fmt.Errorf("Noble RPC endpoint cannot be empty")
	}

	// Ensure the endpoint doesn't have a trailing slash
	rpcEndpoint = strings.TrimSuffix(rpcEndpoint, "/")

	return &NobleClient{
		rpcEndpoint: rpcEndpoint,
		httpClient: &http.Client{
			Timeout: 15 * time.Second,
		},
	}, nil
}

// abciQueryRequest represents a JSON-RPC ABCI query request
type abciQueryRequest struct {
	JSONRPC string            `json:"jsonrpc"`
	ID      int               `json:"id"`
	Method  string            `json:"method"`
	Params  abciQueryParams   `json:"params"`
}

// abciQueryParams represents the params for an ABCI query
type abciQueryParams struct {
	Path   string `json:"path"`
	Data   string `json:"data"`
	Height string `json:"height,omitempty"`
	Prove  bool   `json:"prove,omitempty"`
}

// abciQueryResponse represents a JSON-RPC ABCI query response
type abciQueryResponse struct {
	JSONRPC string `json:"jsonrpc"`
	ID      int    `json:"id"`
	Result  struct {
		Response struct {
			Code      int    `json:"code"`
			Log       string `json:"log"`
			Info      string `json:"info"`
			Index     string `json:"index"`
			Key       string `json:"key"`
			Value     string `json:"value"`
			ProofOps  any    `json:"proofOps"`
			Height    string `json:"height"`
			Codespace string `json:"codespace"`
		} `json:"response"`
	} `json:"result"`
	Error *struct {
		Code    int    `json:"code"`
		Message string `json:"message"`
		Data    string `json:"data"`
	} `json:"error,omitempty"`
}

// QueryForwardingAddress queries Noble for the forwarding address
// corresponding to a Neutron proxy address via IBC channel using Tendermint RPC
//
// Parameters:
//   - channel: IBC channel between Noble and Neutron (e.g., "channel-18")
//   - recipient: Neutron proxy address (bech32 format)
//
// Returns the Noble forwarding address (bech32 format)
//
// Uses ABCI query via Tendermint RPC: POST /abci_query with path "/noble.forwarding.v1.Query/Address"
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

	// Encode the query request as protobuf
	// QueryAddressRequest { channel: string (field 1), recipient: string (field 2) }
	queryData := encodeQueryAddressRequest(channel, recipient)

	// Build JSON-RPC request for ABCI query
	reqBody := abciQueryRequest{
		JSONRPC: "2.0",
		ID:      1,
		Method:  "abci_query",
		Params: abciQueryParams{
			Path: "/noble.forwarding.v1.Query/Address",
			Data: hex.EncodeToString(queryData),
		},
	}

	reqBytes, err := json.Marshal(reqBody)
	if err != nil {
		return "", fmt.Errorf("failed to marshal request: %w", err)
	}

	// Create HTTP request
	req, err := http.NewRequestWithContext(ctx, "POST", n.rpcEndpoint, bytes.NewReader(reqBytes))
	if err != nil {
		return "", fmt.Errorf("failed to create request: %w", err)
	}
	req.Header.Set("Content-Type", "application/json")

	// Execute request
	resp, err := n.httpClient.Do(req)
	if err != nil {
		return "", fmt.Errorf("failed to query Noble RPC: %w", err)
	}
	defer resp.Body.Close()

	// Read response
	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return "", fmt.Errorf("failed to read response: %w", err)
	}

	// Parse JSON-RPC response
	var result abciQueryResponse
	if err := json.Unmarshal(body, &result); err != nil {
		return "", fmt.Errorf("failed to parse response: %w", err)
	}

	// Check for JSON-RPC error
	if result.Error != nil {
		return "", fmt.Errorf("RPC error: %s", result.Error.Message)
	}

	// Check ABCI response code
	if result.Result.Response.Code != 0 {
		return "", fmt.Errorf("ABCI query failed (code %d): %s", result.Result.Response.Code, result.Result.Response.Log)
	}

	// Decode the response value (base64 -> protobuf)
	valueBytes, err := base64.StdEncoding.DecodeString(result.Result.Response.Value)
	if err != nil {
		return "", fmt.Errorf("failed to decode response value: %w", err)
	}

	// Parse the QueryAddressResponse protobuf
	// QueryAddressResponse { address: string (field 1), exists: bool (field 2) }
	address, err := decodeQueryAddressResponse(valueBytes)
	if err != nil {
		return "", fmt.Errorf("failed to decode address response: %w", err)
	}

	if address == "" {
		return "", fmt.Errorf("empty address in response")
	}

	return address, nil
}

// encodeQueryAddressRequest encodes a QueryAddressRequest as protobuf
// Proto definition: message QueryAddressRequest { string channel = 1; string recipient = 2; }
func encodeQueryAddressRequest(channel, recipient string) []byte {
	var buf bytes.Buffer

	// Field 1: channel (string, wire type 2 = length-delimited)
	buf.WriteByte(0x0a) // field 1, wire type 2
	writeProtobufString(&buf, channel)

	// Field 2: recipient (string, wire type 2 = length-delimited)
	buf.WriteByte(0x12) // field 2, wire type 2
	writeProtobufString(&buf, recipient)

	return buf.Bytes()
}

// decodeQueryAddressResponse decodes a QueryAddressResponse from protobuf
// Proto definition: message QueryAddressResponse { string address = 1; bool exists = 2; }
func decodeQueryAddressResponse(data []byte) (string, error) {
	if len(data) == 0 {
		return "", fmt.Errorf("empty response data")
	}

	var address string
	offset := 0

	for offset < len(data) {
		if offset >= len(data) {
			break
		}

		// Read field tag
		tag := data[offset]
		fieldNum := tag >> 3
		wireType := tag & 0x07
		offset++

		switch fieldNum {
		case 1: // address (string)
			if wireType != 2 {
				return "", fmt.Errorf("unexpected wire type for address field: %d", wireType)
			}
			// Read length
			length, n := readVarint(data[offset:])
			offset += n
			if offset+int(length) > len(data) {
				return "", fmt.Errorf("address length exceeds data")
			}
			address = string(data[offset : offset+int(length)])
			offset += int(length)

		case 2: // exists (bool)
			if wireType != 0 {
				return "", fmt.Errorf("unexpected wire type for exists field: %d", wireType)
			}
			// Read varint (bool is encoded as varint)
			_, n := readVarint(data[offset:])
			offset += n

		default:
			// Skip unknown fields
			switch wireType {
			case 0: // varint
				_, n := readVarint(data[offset:])
				offset += n
			case 2: // length-delimited
				length, n := readVarint(data[offset:])
				offset += n + int(length)
			default:
				return "", fmt.Errorf("unknown wire type: %d", wireType)
			}
		}
	}

	return address, nil
}

// writeProtobufString writes a string as a length-delimited protobuf field
func writeProtobufString(buf *bytes.Buffer, s string) {
	// Write length as varint
	writeVarint(buf, uint64(len(s)))
	// Write string bytes
	buf.WriteString(s)
}

// writeVarint writes a varint-encoded uint64 to the buffer
func writeVarint(buf *bytes.Buffer, v uint64) {
	var b [binary.MaxVarintLen64]byte
	n := binary.PutUvarint(b[:], v)
	buf.Write(b[:n])
}

// readVarint reads a varint from the data and returns the value and bytes consumed
func readVarint(data []byte) (uint64, int) {
	v, n := binary.Uvarint(data)
	return v, n
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
//
// Parameters:
//   - codeChecksum: SHA256 checksum of the proxy contract wasm bytecode (32 bytes)
//   - operatorAddress: Neutron operator address (bech32)
//   - userEmail: User's email for deterministic salt generation
//   - nobleChannel: IBC channel between Noble and Neutron (e.g., "channel-18")
func ComputeNobleForwardingAddressForProxy(
	codeChecksum []byte,
	operatorAddress string,
	userEmail string,
	nobleChannel string,
) (string, error) {
	// Generate salt from user email
	salt := GenerateProxySalt(userEmail)

	// First compute the Neutron proxy address
	proxyAddress, err := ComputeProxyAddress(codeChecksum, operatorAddress, salt[:], nil)
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
