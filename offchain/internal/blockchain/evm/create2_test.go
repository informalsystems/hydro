package evm

import (
	"testing"

	"github.com/ethereum/go-ethereum/common"
)

func TestComputeForwarderAddress(t *testing.T) {
	tests := []struct {
		name      string
		userEmail string
		chainID   string
		initCode  []byte
		wantErr   bool
	}{
		{
			name:      "valid inputs",
			userEmail: "alice@example.com",
			chainID:   "1",
			initCode:  []byte{0x60, 0x80, 0x60, 0x40}, // Simple bytecode
			wantErr:   false,
		},
		{
			name:      "empty user email",
			userEmail: "",
			chainID:   "1",
			initCode:  []byte{0x60, 0x80},
			wantErr:   true,
		},
		{
			name:      "empty chain ID",
			userEmail: "alice@example.com",
			chainID:   "",
			initCode:  []byte{0x60, 0x80},
			wantErr:   true,
		},
		{
			name:      "empty init code",
			userEmail: "alice@example.com",
			chainID:   "1",
			initCode:  []byte{},
			wantErr:   true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			addr, err := ComputeForwarderAddress(tt.userEmail, tt.chainID, tt.initCode)

			if (err != nil) != tt.wantErr {
				t.Errorf("ComputeForwarderAddress() error = %v, wantErr %v", err, tt.wantErr)
				return
			}

			if !tt.wantErr && addr == (common.Address{}) {
				t.Errorf("ComputeForwarderAddress() returned zero address")
			}
		})
	}
}

func TestComputeForwarderAddressDeterministic(t *testing.T) {
	userEmail := "alice@example.com"
	chainID := "1"
	initCode := []byte{0x60, 0x80, 0x60, 0x40, 0x52}

	// Compute address twice
	addr1, err1 := ComputeForwarderAddress(userEmail, chainID, initCode)
	addr2, err2 := ComputeForwarderAddress(userEmail, chainID, initCode)

	if err1 != nil || err2 != nil {
		t.Fatalf("ComputeForwarderAddress() failed: err1=%v, err2=%v", err1, err2)
	}

	// Addresses should be identical (deterministic)
	if addr1 != addr2 {
		t.Errorf("ComputeForwarderAddress() is not deterministic: addr1=%s, addr2=%s", addr1.Hex(), addr2.Hex())
	}
}

func TestComputeForwarderAddressDifferentUsers(t *testing.T) {
	chainID := "1"
	initCode := []byte{0x60, 0x80, 0x60, 0x40, 0x52}

	addr1, err1 := ComputeForwarderAddress("alice@example.com", chainID, initCode)
	addr2, err2 := ComputeForwarderAddress("bob@example.com", chainID, initCode)

	if err1 != nil || err2 != nil {
		t.Fatalf("ComputeForwarderAddress() failed: err1=%v, err2=%v", err1, err2)
	}

	// Addresses should be different for different users
	if addr1 == addr2 {
		t.Errorf("ComputeForwarderAddress() returned same address for different users")
	}
}

func TestComputeForwarderAddressDifferentChains(t *testing.T) {
	userEmail := "alice@example.com"
	initCode := []byte{0x60, 0x80, 0x60, 0x40, 0x52}

	addr1, err1 := ComputeForwarderAddress(userEmail, "1", initCode)    // Ethereum
	addr2, err2 := ComputeForwarderAddress(userEmail, "8453", initCode) // Base

	if err1 != nil || err2 != nil {
		t.Fatalf("ComputeForwarderAddress() failed: err1=%v, err2=%v", err1, err2)
	}

	// Addresses should be different for different chains
	if addr1 == addr2 {
		t.Errorf("ComputeForwarderAddress() returned same address for different chains")
	}
}

func TestGenerateSalt(t *testing.T) {
	salt1 := GenerateSalt("alice@example.com", "1")
	salt2 := GenerateSalt("alice@example.com", "1")

	// Same inputs should produce same salt
	if salt1 != salt2 {
		t.Errorf("GenerateSalt() is not deterministic")
	}

	salt3 := GenerateSalt("bob@example.com", "1")

	// Different users should produce different salts
	if salt1 == salt3 {
		t.Errorf("GenerateSalt() returned same salt for different users")
	}
}

func TestVerifyForwarderAddress(t *testing.T) {
	userEmail := "alice@example.com"
	chainID := "1"
	initCode := []byte{0x60, 0x80, 0x60, 0x40, 0x52}

	// Compute the expected address
	expectedAddr, err := ComputeForwarderAddress(userEmail, chainID, initCode)
	if err != nil {
		t.Fatalf("ComputeForwarderAddress() failed: %v", err)
	}

	// Verify with correct address
	valid, err := VerifyForwarderAddress(expectedAddr, userEmail, chainID, initCode)
	if err != nil {
		t.Fatalf("VerifyForwarderAddress() failed: %v", err)
	}
	if !valid {
		t.Errorf("VerifyForwarderAddress() returned false for correct address")
	}

	// Verify with wrong address
	wrongAddr := common.HexToAddress("0x0000000000000000000000000000000000000001")
	valid, err = VerifyForwarderAddress(wrongAddr, userEmail, chainID, initCode)
	if err != nil {
		t.Fatalf("VerifyForwarderAddress() failed: %v", err)
	}
	if valid {
		t.Errorf("VerifyForwarderAddress() returned true for incorrect address")
	}
}
