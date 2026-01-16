package evm

import (
	"testing"

	"github.com/ethereum/go-ethereum/common"
)

func TestComputeForwarderAddress(t *testing.T) {
	tests := []struct {
		name            string
		deployerAddress string
		userEmail       string
		chainID         string
		initCode        []byte
		wantErr         bool
	}{
		{
			name:            "valid inputs",
			deployerAddress: "0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb",
			userEmail:       "alice@example.com",
			chainID:         "1",
			initCode:        []byte{0x60, 0x80, 0x60, 0x40}, // Simple bytecode
			wantErr:         false,
		},
		{
			name:            "empty deployer address",
			deployerAddress: "0x0000000000000000000000000000000000000000",
			userEmail:       "alice@example.com",
			chainID:         "1",
			initCode:        []byte{0x60, 0x80},
			wantErr:         true,
		},
		{
			name:            "empty user email",
			deployerAddress: "0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb",
			userEmail:       "",
			chainID:         "1",
			initCode:        []byte{0x60, 0x80},
			wantErr:         true,
		},
		{
			name:            "empty chain ID",
			deployerAddress: "0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb",
			userEmail:       "alice@example.com",
			chainID:         "",
			initCode:        []byte{0x60, 0x80},
			wantErr:         true,
		},
		{
			name:            "empty init code",
			deployerAddress: "0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb",
			userEmail:       "alice@example.com",
			chainID:         "1",
			initCode:        []byte{},
			wantErr:         true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			deployer := common.HexToAddress(tt.deployerAddress)
			addr, err := ComputeForwarderAddress(deployer, tt.userEmail, tt.chainID, tt.initCode)

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
	deployer := common.HexToAddress("0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb")
	userEmail := "alice@example.com"
	chainID := "1"
	initCode := []byte{0x60, 0x80, 0x60, 0x40, 0x52}

	// Compute address twice
	addr1, err1 := ComputeForwarderAddress(deployer, userEmail, chainID, initCode)
	addr2, err2 := ComputeForwarderAddress(deployer, userEmail, chainID, initCode)

	if err1 != nil || err2 != nil {
		t.Fatalf("ComputeForwarderAddress() failed: err1=%v, err2=%v", err1, err2)
	}

	// Addresses should be identical (deterministic)
	if addr1 != addr2 {
		t.Errorf("ComputeForwarderAddress() is not deterministic: addr1=%s, addr2=%s", addr1.Hex(), addr2.Hex())
	}
}

func TestComputeForwarderAddressDifferentUsers(t *testing.T) {
	deployer := common.HexToAddress("0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb")
	chainID := "1"
	initCode := []byte{0x60, 0x80, 0x60, 0x40, 0x52}

	addr1, err1 := ComputeForwarderAddress(deployer, "alice@example.com", chainID, initCode)
	addr2, err2 := ComputeForwarderAddress(deployer, "bob@example.com", chainID, initCode)

	if err1 != nil || err2 != nil {
		t.Fatalf("ComputeForwarderAddress() failed: err1=%v, err2=%v", err1, err2)
	}

	// Addresses should be different for different users
	if addr1 == addr2 {
		t.Errorf("ComputeForwarderAddress() returned same address for different users")
	}
}

func TestComputeForwarderAddressDifferentChains(t *testing.T) {
	deployer := common.HexToAddress("0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb")
	userEmail := "alice@example.com"
	initCode := []byte{0x60, 0x80, 0x60, 0x40, 0x52}

	addr1, err1 := ComputeForwarderAddress(deployer, userEmail, "1", initCode)      // Ethereum
	addr2, err2 := ComputeForwarderAddress(deployer, userEmail, "8453", initCode)   // Base

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
	deployer := common.HexToAddress("0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb")
	userEmail := "alice@example.com"
	chainID := "1"
	initCode := []byte{0x60, 0x80, 0x60, 0x40, 0x52}

	// Compute the expected address
	expectedAddr, err := ComputeForwarderAddress(deployer, userEmail, chainID, initCode)
	if err != nil {
		t.Fatalf("ComputeForwarderAddress() failed: %v", err)
	}

	// Verify with correct address
	valid, err := VerifyForwarderAddress(expectedAddr, deployer, userEmail, chainID, initCode)
	if err != nil {
		t.Fatalf("VerifyForwarderAddress() failed: %v", err)
	}
	if !valid {
		t.Errorf("VerifyForwarderAddress() returned false for correct address")
	}

	// Verify with wrong address
	wrongAddr := common.HexToAddress("0x0000000000000000000000000000000000000001")
	valid, err = VerifyForwarderAddress(wrongAddr, deployer, userEmail, chainID, initCode)
	if err != nil {
		t.Fatalf("VerifyForwarderAddress() failed: %v", err)
	}
	if valid {
		t.Errorf("VerifyForwarderAddress() returned true for incorrect address")
	}
}
