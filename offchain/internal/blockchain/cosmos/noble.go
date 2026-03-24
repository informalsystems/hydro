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

	"cosmossdk.io/math"
	"github.com/btcsuite/btcutil/bech32"
	gogoproto "github.com/cosmos/gogoproto/proto"
)

func init() {
	// Register MsgRegisterAccount so proto.MessageName resolves correctly
	// when PackAny builds the type URL for transaction encoding.
	gogoproto.RegisterType((*MsgRegisterAccount)(nil), "noble.forwarding.v1.MsgRegisterAccount")
}

// MsgRegisterAccountTypeURL is the type URL for MsgRegisterAccount
const MsgRegisterAccountTypeURL = "/noble.forwarding.v1.MsgRegisterAccount"
const ForwardingAccountTypeURL = "/noble.forwarding.v1.ForwardingAccount"

// MsgRegisterAccount is a minimal implementation of the Noble forwarding
// register-account message. It implements gogoproto.Message and
// codectypes.ProtoMarshaler so it can be packed into a cosmos-sdk Any.
//
// Proto definition:
//
//	message MsgRegisterAccount {
//	  string signer    = 1;
//	  string recipient = 2;
//	  string channel   = 3;
//	  string fallback  = 4;
//	}
type MsgRegisterAccount struct {
	Signer    string
	Recipient string
	Channel   string
	Fallback  string
}

// gogoproto.Message interface
func (m *MsgRegisterAccount) Reset()        { *m = MsgRegisterAccount{} }
func (m *MsgRegisterAccount) ProtoMessage() {}
func (m *MsgRegisterAccount) String() string {
	return fmt.Sprintf("MsgRegisterAccount{signer:%s, recipient:%s, channel:%s}",
		m.Signer, m.Recipient, m.Channel)
}

// Marshal encodes the message as protobuf bytes.
func (m *MsgRegisterAccount) Marshal() ([]byte, error) {
	return encodeMsgRegisterAccountBytes(m.Signer, m.Recipient, m.Channel, m.Fallback), nil
}

// MarshalTo copies the encoded bytes into dAtA and returns the number of bytes written.
func (m *MsgRegisterAccount) MarshalTo(dAtA []byte) (int, error) {
	encoded := encodeMsgRegisterAccountBytes(m.Signer, m.Recipient, m.Channel, m.Fallback)
	copy(dAtA, encoded)
	return len(encoded), nil
}

// MarshalToSizedBuffer encodes into the tail of dAtA (gogoproto convention).
func (m *MsgRegisterAccount) MarshalToSizedBuffer(dAtA []byte) (int, error) {
	encoded := encodeMsgRegisterAccountBytes(m.Signer, m.Recipient, m.Channel, m.Fallback)
	i := len(dAtA) - len(encoded)
	copy(dAtA[i:], encoded)
	return len(encoded), nil
}

// Size returns the serialized size in bytes.
func (m *MsgRegisterAccount) Size() int {
	return len(encodeMsgRegisterAccountBytes(m.Signer, m.Recipient, m.Channel, m.Fallback))
}

// Unmarshal decodes the message from protobuf bytes (not needed for broadcasting).
func (m *MsgRegisterAccount) Unmarshal(_ []byte) error { return nil }

// encodeMsgRegisterAccountBytes manually encodes MsgRegisterAccount as protobuf.
// Fields: signer (1), recipient (2), channel (3), fallback (4) — all strings.
func encodeMsgRegisterAccountBytes(signer, recipient, channel, fallback string) []byte {
	var buf bytes.Buffer
	if signer != "" {
		buf.WriteByte(0x0a) // field 1, wire type 2
		writeProtobufString(&buf, signer)
	}
	if recipient != "" {
		buf.WriteByte(0x12) // field 2, wire type 2
		writeProtobufString(&buf, recipient)
	}
	if channel != "" {
		buf.WriteByte(0x1a) // field 3, wire type 2
		writeProtobufString(&buf, channel)
	}
	if fallback != "" {
		buf.WriteByte(0x22) // field 4, wire type 2
		writeProtobufString(&buf, fallback)
	}
	return buf.Bytes()
}

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
	JSONRPC string          `json:"jsonrpc"`
	ID      int             `json:"id"`
	Method  string          `json:"method"`
	Params  abciQueryParams `json:"params"`
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

// QueryAccountType queries the Noble chain for the account type at the given address.
// It returns the proto type URL (e.g. "/noble.forwarding.v1.ForwardingAccount") or
// an empty string when the account does not exist yet (ABCI code != 0).
//
// ABCI query path: /cosmos.auth.v1beta1.Query/Account
// Request:  QueryAccountRequest  { address: string (field 1) }
// Response: QueryAccountResponse { account: google.protobuf.Any (field 1) }
//
//	where Any { type_url: string (field 1), value: bytes (field 2) }
func (n *NobleClient) QueryAccountType(ctx context.Context, address string) (string, error) {
	// Encode QueryAccountRequest { address (field 1) }
	var reqBuf bytes.Buffer
	reqBuf.WriteByte(0x0a) // field 1, wire type 2
	writeProtobufString(&reqBuf, address)

	reqBody := abciQueryRequest{
		JSONRPC: "2.0",
		ID:      1,
		Method:  "abci_query",
		Params: abciQueryParams{
			Path: "/cosmos.auth.v1beta1.Query/Account",
			Data: hex.EncodeToString(reqBuf.Bytes()),
		},
	}

	reqBytes, err := json.Marshal(reqBody)
	if err != nil {
		return "", fmt.Errorf("failed to marshal request: %w", err)
	}

	req, err := http.NewRequestWithContext(ctx, "POST", n.rpcEndpoint, bytes.NewReader(reqBytes))
	if err != nil {
		return "", fmt.Errorf("failed to create request: %w", err)
	}
	req.Header.Set("Content-Type", "application/json")

	resp, err := n.httpClient.Do(req)
	if err != nil {
		return "", fmt.Errorf("failed to query Noble RPC: %w", err)
	}
	defer resp.Body.Close()

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return "", fmt.Errorf("failed to read response: %w", err)
	}

	var result abciQueryResponse
	if err := json.Unmarshal(body, &result); err != nil {
		return "", fmt.Errorf("failed to parse response: %w", err)
	}

	if result.Error != nil {
		return "", fmt.Errorf("RPC error: %s", result.Error.Message)
	}

	// Account does not exist yet
	if result.Result.Response.Code != 0 {
		return "", nil
	}

	valueBytes, err := base64.StdEncoding.DecodeString(result.Result.Response.Value)
	if err != nil {
		return "", fmt.Errorf("failed to decode response value: %w", err)
	}

	// Parse QueryAccountResponse: field 1 is google.protobuf.Any (length-delimited)
	// Inside Any: field 1 is type_url (string)
	typeURL, err := decodeAccountTypeURL(valueBytes)
	if err != nil {
		return "", fmt.Errorf("failed to decode account type URL: %w", err)
	}

	return typeURL, nil
}

// decodeAccountTypeURL extracts the type_url from a QueryAccountResponse.
// QueryAccountResponse { google.protobuf.Any account = 1 }
// google.protobuf.Any  { string type_url = 1; bytes value = 2 }
func decodeAccountTypeURL(data []byte) (string, error) {
	if len(data) == 0 {
		return "", nil
	}

	offset := 0
	for offset < len(data) {
		tag := data[offset]
		fieldNum := tag >> 3
		wireType := tag & 0x07
		offset++

		if fieldNum == 1 && wireType == 2 {
			// Field 1: Any (length-delimited)
			anyLen, n := readVarint(data[offset:])
			offset += n
			if offset+int(anyLen) > len(data) {
				return "", fmt.Errorf("any length exceeds data")
			}
			anyBytes := data[offset : offset+int(anyLen)]

			// Now parse the Any to extract type_url (field 1)
			anyOffset := 0
			for anyOffset < len(anyBytes) {
				anyTag := anyBytes[anyOffset]
				anyFieldNum := anyTag >> 3
				anyWireType := anyTag & 0x07
				anyOffset++

				if anyFieldNum == 1 && anyWireType == 2 {
					// type_url string
					strLen, n2 := readVarint(anyBytes[anyOffset:])
					anyOffset += n2
					if anyOffset+int(strLen) > len(anyBytes) {
						return "", fmt.Errorf("type_url length exceeds data")
					}
					return string(anyBytes[anyOffset : anyOffset+int(strLen)]), nil
				}
				// Skip other fields
				switch anyWireType {
				case 0:
					_, n2 := readVarint(anyBytes[anyOffset:])
					anyOffset += n2
				case 2:
					fLen, n2 := readVarint(anyBytes[anyOffset:])
					anyOffset += n2 + int(fLen)
				default:
					return "", fmt.Errorf("unknown wire type in Any: %d", anyWireType)
				}
			}
			return "", nil
		}

		// Skip other top-level fields
		switch wireType {
		case 0:
			_, n := readVarint(data[offset:])
			offset += n
		case 2:
			fLen, n := readVarint(data[offset:])
			offset += n + int(fLen)
		default:
			return "", fmt.Errorf("unknown wire type: %d", wireType)
		}
	}

	return "", nil
}

// QueryUSDCBalance queries the uusdc balance for an address on Noble.
// Returns math.ZeroInt() when the account or balance does not exist.
//
// ABCI query path: /cosmos.bank.v1beta1.Query/Balance
// Request:  QueryBalanceRequest  { address: string (field 1), denom: string (field 2) }
// Response: QueryBalanceResponse { balance: Coin (field 1) }
//
//	where Coin { denom: string (field 1), amount: string (field 2) }
func (n *NobleClient) QueryUSDCBalance(ctx context.Context, address string) (math.Int, error) {
	var reqBuf bytes.Buffer
	reqBuf.WriteByte(0x0a) // field 1, wire type 2
	writeProtobufString(&reqBuf, address)
	reqBuf.WriteByte(0x12) // field 2, wire type 2
	writeProtobufString(&reqBuf, "uusdc")

	reqBody := abciQueryRequest{
		JSONRPC: "2.0",
		ID:      1,
		Method:  "abci_query",
		Params: abciQueryParams{
			Path: "/cosmos.bank.v1beta1.Query/Balance",
			Data: hex.EncodeToString(reqBuf.Bytes()),
		},
	}

	reqBytes, err := json.Marshal(reqBody)
	if err != nil {
		return math.ZeroInt(), fmt.Errorf("failed to marshal request: %w", err)
	}

	req, err := http.NewRequestWithContext(ctx, "POST", n.rpcEndpoint, bytes.NewReader(reqBytes))
	if err != nil {
		return math.ZeroInt(), fmt.Errorf("failed to create request: %w", err)
	}
	req.Header.Set("Content-Type", "application/json")

	resp, err := n.httpClient.Do(req)
	if err != nil {
		return math.ZeroInt(), fmt.Errorf("failed to query Noble RPC: %w", err)
	}
	defer resp.Body.Close()

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return math.ZeroInt(), fmt.Errorf("failed to read response: %w", err)
	}

	var result abciQueryResponse
	if err := json.Unmarshal(body, &result); err != nil {
		return math.ZeroInt(), fmt.Errorf("failed to parse response: %w", err)
	}

	if result.Error != nil {
		return math.ZeroInt(), fmt.Errorf("RPC error: %s", result.Error.Message)
	}

	if result.Result.Response.Code != 0 {
		// Account or balance not found
		return math.ZeroInt(), nil
	}

	valueBytes, err := base64.StdEncoding.DecodeString(result.Result.Response.Value)
	if err != nil {
		return math.ZeroInt(), fmt.Errorf("failed to decode response value: %w", err)
	}

	amount, err := decodeBalanceResponse(valueBytes)
	if err != nil {
		return math.ZeroInt(), fmt.Errorf("failed to decode balance response: %w", err)
	}

	return amount, nil
}

// decodeBalanceResponse extracts the amount from a QueryBalanceResponse.
// QueryBalanceResponse { Coin balance = 1 }
// Coin { string denom = 1; string amount = 2 }
func decodeBalanceResponse(data []byte) (math.Int, error) {
	if len(data) == 0 {
		return math.ZeroInt(), nil
	}

	offset := 0
	for offset < len(data) {
		tag := data[offset]
		fieldNum := tag >> 3
		wireType := tag & 0x07
		offset++

		if fieldNum == 1 && wireType == 2 {
			// Coin (length-delimited)
			coinLen, n := readVarint(data[offset:])
			offset += n
			if offset+int(coinLen) > len(data) {
				return math.ZeroInt(), fmt.Errorf("coin length exceeds data")
			}
			coinBytes := data[offset : offset+int(coinLen)]

			// Parse Coin: field 2 is amount string
			coinOffset := 0
			for coinOffset < len(coinBytes) {
				coinTag := coinBytes[coinOffset]
				coinFieldNum := coinTag >> 3
				coinWireType := coinTag & 0x07
				coinOffset++

				strLen, n2 := readVarint(coinBytes[coinOffset:])
				coinOffset += n2

				if coinOffset+int(strLen) > len(coinBytes) {
					return math.ZeroInt(), fmt.Errorf("field length exceeds coin data")
				}

				if coinFieldNum == 2 && coinWireType == 2 {
					amountStr := string(coinBytes[coinOffset : coinOffset+int(strLen)])
					amount, ok := math.NewIntFromString(amountStr)
					if !ok {
						return math.ZeroInt(), fmt.Errorf("invalid amount: %s", amountStr)
					}
					return amount, nil
				}
				coinOffset += int(strLen)
			}
			return math.ZeroInt(), nil
		}

		// Skip other fields
		switch wireType {
		case 0:
			_, n := readVarint(data[offset:])
			offset += n
		case 2:
			fLen, n := readVarint(data[offset:])
			offset += n + int(fLen)
		default:
			return math.ZeroInt(), fmt.Errorf("unknown wire type: %d", wireType)
		}
	}

	return math.ZeroInt(), nil
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
