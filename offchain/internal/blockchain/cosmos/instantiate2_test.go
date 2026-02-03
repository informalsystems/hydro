package cosmos

import (
	"crypto/sha256"
	"testing"

	"github.com/btcsuite/btcutil/bech32"
)

// generateTestChecksum creates a deterministic test checksum from a string
func generateTestChecksum(input string) []byte {
	hash := sha256.Sum256([]byte(input))
	return hash[:]
}

func TestComputeProxyAddress(t *testing.T) {
	testChecksum := generateTestChecksum("test-wasm-bytecode")
	testSalt := GenerateProxySalt("alice@example.com")

	tests := []struct {
		name           string
		codeChecksum   []byte
		creatorAddress string
		salt           []byte
		msg            []byte
		wantErr        bool
		errMsg         string
	}{
		{
			name:           "valid inputs",
			codeChecksum:   testChecksum,
			creatorAddress: "neutron1m0z0kk0qqug74n9u9ul23e28x5fszr62y9w9dk",
			salt:           testSalt[:],
			msg:            nil,
			wantErr:        false,
		},
		{
			name:           "different checksum",
			codeChecksum:   generateTestChecksum("different-bytecode"),
			creatorAddress: "neutron1m0z0kk0qqug74n9u9ul23e28x5fszr62y9w9dk",
			salt:           testSalt[:],
			msg:            nil,
			wantErr:        false,
		},
		{
			name:           "different salt",
			codeChecksum:   testChecksum,
			creatorAddress: "neutron1m0z0kk0qqug74n9u9ul23e28x5fszr62y9w9dk",
			salt:           func() []byte { s := GenerateProxySalt("bob@example.com"); return s[:] }(),
			msg:            nil,
			wantErr:        false,
		},
		{
			name:           "invalid checksum length",
			codeChecksum:   []byte{1, 2, 3}, // Not 32 bytes
			creatorAddress: "neutron1m0z0kk0qqug74n9u9ul23e28x5fszr62y9w9dk",
			salt:           testSalt[:],
			msg:            nil,
			wantErr:        true,
			errMsg:         "code checksum must be 32 bytes, got 3",
		},
		{
			name:           "empty creator address",
			codeChecksum:   testChecksum,
			creatorAddress: "",
			salt:           testSalt[:],
			msg:            nil,
			wantErr:        true,
			errMsg:         "creator address cannot be empty",
		},
		{
			name:           "empty salt",
			codeChecksum:   testChecksum,
			creatorAddress: "neutron1m0z0kk0qqug74n9u9ul23e28x5fszr62y9w9dk",
			salt:           nil,
			msg:            nil,
			wantErr:        true,
			errMsg:         "salt cannot be empty",
		},
		{
			name:           "invalid bech32 address",
			codeChecksum:   testChecksum,
			creatorAddress: "invalid_address",
			salt:           testSalt[:],
			msg:            nil,
			wantErr:        true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			addr, err := ComputeProxyAddress(tt.codeChecksum, tt.creatorAddress, tt.salt, tt.msg)

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

			// Verify address is 32 bytes (CosmWasm contract address length)
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

			if len(data8bit) != 32 {
				t.Errorf("ComputeProxyAddress() address length = %v bytes, want 32 bytes", len(data8bit))
			}
		})
	}
}

func TestComputeProxyAddressDeterministic(t *testing.T) {
	// Same inputs should always produce same output
	codeChecksum := generateTestChecksum("test-bytecode")
	creatorAddress := "neutron1m0z0kk0qqug74n9u9ul23e28x5fszr62y9w9dk"
	salt := GenerateProxySalt("alice@example.com")

	addr1, err := ComputeProxyAddress(codeChecksum, creatorAddress, salt[:], nil)
	if err != nil {
		t.Fatalf("ComputeProxyAddress() error = %v", err)
	}

	addr2, err := ComputeProxyAddress(codeChecksum, creatorAddress, salt[:], nil)
	if err != nil {
		t.Fatalf("ComputeProxyAddress() error = %v", err)
	}

	if addr1 != addr2 {
		t.Errorf("ComputeProxyAddress() not deterministic: addr1 = %v, addr2 = %v", addr1, addr2)
	}
}

func TestComputeProxyAddressUniqueness(t *testing.T) {
	creatorAddress := "neutron1m0z0kk0qqug74n9u9ul23e28x5fszr62y9w9dk"
	salt := GenerateProxySalt("alice@example.com")

	// Different checksums should produce different addresses
	addr1, err := ComputeProxyAddress(generateTestChecksum("bytecode-1"), creatorAddress, salt[:], nil)
	if err != nil {
		t.Fatalf("ComputeProxyAddress() error = %v", err)
	}

	addr2, err := ComputeProxyAddress(generateTestChecksum("bytecode-2"), creatorAddress, salt[:], nil)
	if err != nil {
		t.Fatalf("ComputeProxyAddress() error = %v", err)
	}

	if addr1 == addr2 {
		t.Errorf("ComputeProxyAddress() same address for different checksums")
	}

	// Different salts should produce different addresses
	checksum := generateTestChecksum("same-bytecode")
	bobSalt := GenerateProxySalt("bob@example.com")
	addr3, err := ComputeProxyAddress(checksum, creatorAddress, bobSalt[:], nil)
	if err != nil {
		t.Fatalf("ComputeProxyAddress() error = %v", err)
	}

	addr4, err := ComputeProxyAddress(checksum, creatorAddress, salt[:], nil)
	if err != nil {
		t.Fatalf("ComputeProxyAddress() error = %v", err)
	}

	if addr3 == addr4 {
		t.Errorf("ComputeProxyAddress() same address for different salts")
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
	codeChecksum := generateTestChecksum("test-bytecode")
	creatorAddress := "neutron1m0z0kk0qqug74n9u9ul23e28x5fszr62y9w9dk"
	salt := GenerateProxySalt("alice@example.com")

	// Compute expected address
	expectedAddr, err := ComputeProxyAddress(codeChecksum, creatorAddress, salt[:], nil)
	if err != nil {
		t.Fatalf("ComputeProxyAddress() error = %v", err)
	}

	tests := []struct {
		name            string
		expectedAddress string
		codeChecksum    []byte
		creatorAddress  string
		salt            []byte
		msg             []byte
		want            bool
		wantErr         bool
	}{
		{
			name:            "matching address",
			expectedAddress: expectedAddr,
			codeChecksum:    codeChecksum,
			creatorAddress:  creatorAddress,
			salt:            salt[:],
			msg:             nil,
			want:            true,
			wantErr:         false,
		},
		{
			name:            "wrong address",
			expectedAddress: "neutron1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqnrql4e",
			codeChecksum:    codeChecksum,
			creatorAddress:  creatorAddress,
			salt:            salt[:],
			msg:             nil,
			want:            false,
			wantErr:         false,
		},
		{
			name:            "invalid inputs",
			expectedAddress: expectedAddr,
			codeChecksum:    []byte{1, 2, 3}, // Invalid length
			creatorAddress:  creatorAddress,
			salt:            salt[:],
			msg:             nil,
			want:            false,
			wantErr:         true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got, err := VerifyProxyAddress(tt.expectedAddress, tt.codeChecksum, tt.creatorAddress, tt.salt, tt.msg)

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
