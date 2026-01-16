package cosmos

import (
	"testing"

	"github.com/btcsuite/btcutil/bech32"
)

func TestComputeProxyAddress(t *testing.T) {
	tests := []struct {
		name           string
		codeID         uint64
		creatorAddress string
		userEmail      string
		wantErr        bool
		errMsg         string
	}{
		{
			name:           "valid inputs",
			codeID:         123,
			creatorAddress: "neutron1m0z0kk0qqug74n9u9ul23e28x5fszr62y9w9dk",
			userEmail:      "alice@example.com",
			wantErr:        false,
		},
		{
			name:           "different code ID",
			codeID:         456,
			creatorAddress: "neutron1m0z0kk0qqug74n9u9ul23e28x5fszr62y9w9dk",
			userEmail:      "alice@example.com",
			wantErr:        false,
		},
		{
			name:           "different user",
			codeID:         123,
			creatorAddress: "neutron1m0z0kk0qqug74n9u9ul23e28x5fszr62y9w9dk",
			userEmail:      "bob@example.com",
			wantErr:        false,
		},
		{
			name:           "zero code ID",
			codeID:         0,
			creatorAddress: "neutron1m0z0kk0qqug74n9u9ul23e28x5fszr62y9w9dk",
			userEmail:      "alice@example.com",
			wantErr:        true,
			errMsg:         "code ID cannot be zero",
		},
		{
			name:           "empty creator address",
			codeID:         123,
			creatorAddress: "",
			userEmail:      "alice@example.com",
			wantErr:        true,
			errMsg:         "creator address cannot be empty",
		},
		{
			name:           "empty user email",
			codeID:         123,
			creatorAddress: "neutron1m0z0kk0qqug74n9u9ul23e28x5fszr62y9w9dk",
			userEmail:      "",
			wantErr:        true,
			errMsg:         "user email cannot be empty",
		},
		{
			name:           "invalid bech32 address",
			codeID:         123,
			creatorAddress: "invalid_address",
			userEmail:      "alice@example.com",
			wantErr:        true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			addr, err := ComputeProxyAddress(tt.codeID, tt.creatorAddress, tt.userEmail)

			if tt.wantErr {
				if err == nil {
					t.Errorf("ComputeProxyAddress() expected error but got none")
					return
				}
				if tt.errMsg != "" && err.Error() != tt.errMsg {
					t.Errorf("ComputeProxyAddress() error = %v, want %v", err.Error(), tt.errMsg)
				}
				return
			}

			if err != nil {
				t.Errorf("ComputeProxyAddress() unexpected error = %v", err)
				return
			}

			// Verify address is valid bech32 with neutron prefix
			prefix, _, err := bech32.Decode(addr)
			if err != nil {
				t.Errorf("ComputeProxyAddress() returned invalid bech32 address: %v", err)
				return
			}

			if prefix != "neutron" {
				t.Errorf("ComputeProxyAddress() prefix = %v, want neutron", prefix)
			}

			// Verify address is 20 bytes (standard Cosmos address length)
			// After bech32 decoding and bit conversion
			_, data5bit, err := bech32.Decode(addr)
			if err != nil {
				t.Errorf("Failed to decode returned address: %v", err)
				return
			}

			data8bit, err := bech32.ConvertBits(data5bit, 5, 8, false)
			if err != nil {
				t.Errorf("Failed to convert address bits: %v", err)
				return
			}

			if len(data8bit) != 20 {
				t.Errorf("ComputeProxyAddress() address length = %v bytes, want 20 bytes", len(data8bit))
			}
		})
	}
}

func TestComputeProxyAddressDeterministic(t *testing.T) {
	// Same inputs should always produce same output
	codeID := uint64(123)
	creatorAddress := "neutron1m0z0kk0qqug74n9u9ul23e28x5fszr62y9w9dk"
	userEmail := "alice@example.com"

	addr1, err := ComputeProxyAddress(codeID, creatorAddress, userEmail)
	if err != nil {
		t.Fatalf("ComputeProxyAddress() error = %v", err)
	}

	addr2, err := ComputeProxyAddress(codeID, creatorAddress, userEmail)
	if err != nil {
		t.Fatalf("ComputeProxyAddress() error = %v", err)
	}

	if addr1 != addr2 {
		t.Errorf("ComputeProxyAddress() not deterministic: addr1 = %v, addr2 = %v", addr1, addr2)
	}
}

func TestComputeProxyAddressUniqueness(t *testing.T) {
	creatorAddress := "neutron1m0z0kk0qqug74n9u9ul23e28x5fszr62y9w9dk"
	userEmail := "alice@example.com"

	// Different code IDs should produce different addresses
	addr1, err := ComputeProxyAddress(123, creatorAddress, userEmail)
	if err != nil {
		t.Fatalf("ComputeProxyAddress() error = %v", err)
	}

	addr2, err := ComputeProxyAddress(456, creatorAddress, userEmail)
	if err != nil {
		t.Fatalf("ComputeProxyAddress() error = %v", err)
	}

	if addr1 == addr2 {
		t.Errorf("ComputeProxyAddress() same address for different code IDs")
	}

	// Different users should produce different addresses
	addr3, err := ComputeProxyAddress(123, creatorAddress, "bob@example.com")
	if err != nil {
		t.Fatalf("ComputeProxyAddress() error = %v", err)
	}

	if addr1 == addr3 {
		t.Errorf("ComputeProxyAddress() same address for different users")
	}
}

func TestGenerateProxySalt(t *testing.T) {
	tests := []struct {
		name      string
		userEmail string
		want      bool // true if should be deterministic
	}{
		{
			name:      "alice@example.com",
			userEmail: "alice@example.com",
			want:      true,
		},
		{
			name:      "bob@example.com",
			userEmail: "bob@example.com",
			want:      true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			salt1 := GenerateProxySalt(tt.userEmail)
			salt2 := GenerateProxySalt(tt.userEmail)

			if tt.want && salt1 != salt2 {
				t.Errorf("GenerateProxySalt() not deterministic")
			}

			// Salt should be 32 bytes
			if len(salt1) != 32 {
				t.Errorf("GenerateProxySalt() length = %v, want 32", len(salt1))
			}
		})
	}
}

func TestGenerateProxySaltUniqueness(t *testing.T) {
	// Different emails should produce different salts
	salt1 := GenerateProxySalt("alice@example.com")
	salt2 := GenerateProxySalt("bob@example.com")

	if salt1 == salt2 {
		t.Errorf("GenerateProxySalt() same salt for different emails")
	}
}

func TestVerifyProxyAddress(t *testing.T) {
	codeID := uint64(123)
	creatorAddress := "neutron1m0z0kk0qqug74n9u9ul23e28x5fszr62y9w9dk"
	userEmail := "alice@example.com"

	// Compute expected address
	expectedAddr, err := ComputeProxyAddress(codeID, creatorAddress, userEmail)
	if err != nil {
		t.Fatalf("ComputeProxyAddress() error = %v", err)
	}

	tests := []struct {
		name            string
		expectedAddress string
		codeID          uint64
		creatorAddress  string
		userEmail       string
		want            bool
		wantErr         bool
	}{
		{
			name:            "matching address",
			expectedAddress: expectedAddr,
			codeID:          codeID,
			creatorAddress:  creatorAddress,
			userEmail:       userEmail,
			want:            true,
			wantErr:         false,
		},
		{
			name:            "wrong address",
			expectedAddress: "neutron1qqqqqqqqqqqqqqqqqqqqqqqqqqqqq4sphkm",
			codeID:          codeID,
			creatorAddress:  creatorAddress,
			userEmail:       userEmail,
			want:            false,
			wantErr:         false,
		},
		{
			name:            "invalid inputs",
			expectedAddress: expectedAddr,
			codeID:          0,
			creatorAddress:  creatorAddress,
			userEmail:       userEmail,
			want:            false,
			wantErr:         true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got, err := VerifyProxyAddress(tt.expectedAddress, tt.codeID, tt.creatorAddress, tt.userEmail)

			if (err != nil) != tt.wantErr {
				t.Errorf("VerifyProxyAddress() error = %v, wantErr %v", err, tt.wantErr)
				return
			}

			if !tt.wantErr && got != tt.want {
				t.Errorf("VerifyProxyAddress() = %v, want %v", got, tt.want)
			}
		})
	}
}
