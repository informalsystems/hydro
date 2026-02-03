package cosmos

import (
	"context"
	"os"
	"testing"
	"time"
)

func TestNewNobleClient(t *testing.T) {
	tests := []struct {
		name        string
		rpcEndpoint string
		wantErr     bool
		errMsg      string
	}{
		{
			name:        "valid endpoint",
			rpcEndpoint: "https://noble-rpc.polkachu.com",
			wantErr:     false,
		},
		{
			name:        "empty endpoint",
			rpcEndpoint: "",
			wantErr:     true,
			errMsg:      "Noble RPC endpoint cannot be empty",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			client, err := NewNobleClient(tt.rpcEndpoint)

			if tt.wantErr {
				if err == nil {
					t.Errorf("NewNobleClient() expected error but got none")
					return
				}
				if err.Error() != tt.errMsg {
					t.Errorf("NewNobleClient() error = %v, want %v", err.Error(), tt.errMsg)
				}
				return
			}

			if err != nil {
				t.Errorf("NewNobleClient() unexpected error = %v", err)
				return
			}

			if client == nil {
				t.Error("NewNobleClient() returned nil client")
			}
		})
	}
}

// TestQueryForwardingAddressIntegration tests against the real Noble RPC endpoint.
// Run with: go test -v -run TestQueryForwardingAddressIntegration -integration
func TestQueryForwardingAddressIntegration(t *testing.T) {
	if os.Getenv("INTEGRATION_TEST") == "" {
		t.Skip("Skipping integration test. Set INTEGRATION_TEST=1 to run.")
	}

	client, err := NewNobleClient("https://noble-rpc.polkachu.com")
	if err != nil {
		t.Fatalf("Failed to create client: %v", err)
	}

	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()

	// Test with a known proxy address
	proxyAddress := "neutron1lx6fsftukg6xs80vrdxrjmcupslf0qjg8explknna2p4fe85pzrsc2dz50"
	expectedForwardingAddr := "noble1xh5ntwqcsjfhcw7rj0hrtpw0xne9tdeykjzwyf"

	forwardingAddr, err := client.QueryForwardingAddress(ctx, "channel-18", proxyAddress)
	if err != nil {
		t.Fatalf("QueryForwardingAddress() error: %v", err)
	}

	if forwardingAddr != expectedForwardingAddr {
		t.Errorf("QueryForwardingAddress() = %v, want %v", forwardingAddr, expectedForwardingAddr)
	}
}

func TestQueryForwardingAddress(t *testing.T) {
	client, err := NewNobleClient("https://noble-rpc.polkachu.com")
	if err != nil {
		t.Fatalf("Failed to create client: %v", err)
	}

	tests := []struct {
		name      string
		channel   string
		recipient string
		wantErr   bool
	}{
		{
			name:      "empty channel",
			channel:   "",
			recipient: "neutron1test",
			wantErr:   true,
		},
		{
			name:      "empty recipient",
			channel:   "channel-18",
			recipient: "",
			wantErr:   true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			ctx := context.Background()
			_, err := client.QueryForwardingAddress(ctx, tt.channel, tt.recipient)

			if (err != nil) != tt.wantErr {
				t.Errorf("QueryForwardingAddress() error = %v, wantErr %v", err, tt.wantErr)
				return
			}
		})
	}
}

func TestProtobufEncoding(t *testing.T) {
	// Test the protobuf encoding for QueryAddressRequest
	channel := "channel-18"
	recipient := "neutron1test"

	encoded := encodeQueryAddressRequest(channel, recipient)

	// Verify the encoded bytes are not empty
	if len(encoded) == 0 {
		t.Error("encodeQueryAddressRequest() returned empty bytes")
	}

	// The encoding should contain both strings
	// Field 1 (channel): tag 0x0a, length, data
	// Field 2 (recipient): tag 0x12, length, data
	if encoded[0] != 0x0a {
		t.Errorf("Expected first field tag 0x0a, got 0x%02x", encoded[0])
	}
}

func TestConvertToBytes32(t *testing.T) {
	tests := []struct {
		name      string
		nobleAddr string
		wantErr   bool
	}{
		{
			name:      "valid noble address (using neutron format for testing)",
			nobleAddr: "neutron1m0z0kk0qqug74n9u9ul23e28x5fszr62y9w9dk",
			wantErr:   false,
		},
		{
			name:      "empty address",
			nobleAddr: "",
			wantErr:   true,
		},
		{
			name:      "invalid bech32",
			nobleAddr: "invalid_address",
			wantErr:   true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got, err := ConvertToBytes32(tt.nobleAddr)

			if (err != nil) != tt.wantErr {
				t.Errorf("ConvertToBytes32() error = %v, wantErr %v", err, tt.wantErr)
				return
			}

			if !tt.wantErr {
				// Verify result is 32 bytes
				if len(got) != 32 {
					t.Errorf("ConvertToBytes32() returned %d bytes, want 32", len(got))
				}

				// Verify we can convert back to hex
				hexStr := ConvertBytes32ToHex(got)
				if hexStr[:2] != "0x" {
					t.Errorf("ConvertBytes32ToHex() should start with 0x, got %s", hexStr[:2])
				}
			}
		})
	}
}

func TestConvertBytes32ToHex(t *testing.T) {
	// Test with known bytes32
	var testBytes [32]byte
	for i := 0; i < 32; i++ {
		testBytes[i] = byte(i)
	}

	result := ConvertBytes32ToHex(testBytes)

	// Should start with 0x
	if result[:2] != "0x" {
		t.Errorf("ConvertBytes32ToHex() should start with 0x, got %s", result[:2])
	}

	// Should be 66 characters total (0x + 64 hex chars)
	if len(result) != 66 {
		t.Errorf("ConvertBytes32ToHex() length = %d, want 66", len(result))
	}
}

func TestNobleClientClose(t *testing.T) {
	client, err := NewNobleClient("https://test.com")
	if err != nil {
		t.Fatalf("Failed to create client: %v", err)
	}

	// Close should not return error
	if err := client.Close(); err != nil {
		t.Errorf("Close() returned error: %v", err)
	}
}
