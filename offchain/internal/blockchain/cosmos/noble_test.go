package cosmos

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestNewNobleClient(t *testing.T) {
	tests := []struct {
		name        string
		apiEndpoint string
		wantErr     bool
		errMsg      string
	}{
		{
			name:        "valid endpoint",
			apiEndpoint: "https://noble-api.polkachu.com",
			wantErr:     false,
		},
		{
			name:        "empty endpoint",
			apiEndpoint: "",
			wantErr:     true,
			errMsg:      "Noble API endpoint cannot be empty",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			client, err := NewNobleClient(tt.apiEndpoint)

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

func TestQueryForwardingAddress(t *testing.T) {
	// Create test server
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		// Verify request path
		expectedPath := "/forwarding/v1/address/channel-18/neutron1test"
		if r.URL.Path != expectedPath {
			t.Errorf("Expected path %s, got %s", expectedPath, r.URL.Path)
		}

		// Verify method
		if r.Method != "GET" {
			t.Errorf("Expected GET method, got %s", r.Method)
		}

		// Return mock response
		response := map[string]string{
			"address": "noble1mockforwardingaddress",
		}
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(response)
	}))
	defer server.Close()

	// Create client with test server URL
	client, err := NewNobleClient(server.URL)
	if err != nil {
		t.Fatalf("Failed to create client: %v", err)
	}

	tests := []struct {
		name      string
		channel   string
		recipient string
		want      string
		wantErr   bool
	}{
		{
			name:      "valid query",
			channel:   "channel-18",
			recipient: "neutron1test",
			want:      "noble1mockforwardingaddress",
			wantErr:   false,
		},
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
			got, err := client.QueryForwardingAddress(ctx, tt.channel, tt.recipient)

			if (err != nil) != tt.wantErr {
				t.Errorf("QueryForwardingAddress() error = %v, wantErr %v", err, tt.wantErr)
				return
			}

			if !tt.wantErr && got != tt.want {
				t.Errorf("QueryForwardingAddress() = %v, want %v", got, tt.want)
			}
		})
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
