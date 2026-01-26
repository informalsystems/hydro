package evm

import (
	"crypto/sha256"
	"fmt"

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/crypto"
)

// ArachnidFactoryAddress is the deterministic deployment proxy deployed on all EVM chains
// See: https://github.com/Arachnid/deterministic-deployment-proxy
var ArachnidFactoryAddress = common.HexToAddress("0x4e59b44847b379578588920cA78FbF26c0B4956C")

// ComputeForwarderAddress computes the CREATE2 address for a forwarder contract
// using Arachnid's Deterministic Deployment Proxy
//
// CREATE2 formula: address = keccak256(0xff ++ factoryAddress ++ salt ++ keccak256(initCode))[12:]
//
// Parameters:
//   - userEmail: User's email address (used to generate deterministic salt)
//   - chainID: Chain ID where contract will be deployed (used in salt for uniqueness)
//   - initCode: The complete bytecode including constructor parameters
//
// Salt generation: sha256(userEmail + ":" + chainID)
// This ensures each user gets a unique forwarder address per chain
func ComputeForwarderAddress(
	userEmail string,
	chainID string,
	initCode []byte,
) (common.Address, error) {
	if userEmail == "" {
		return common.Address{}, fmt.Errorf("user email cannot be empty")
	}
	if chainID == "" {
		return common.Address{}, fmt.Errorf("chain ID cannot be empty")
	}
	if len(initCode) == 0 {
		return common.Address{}, fmt.Errorf("init code cannot be empty")
	}

	// Generate deterministic salt from userEmail + chainID
	salt := GenerateSalt(userEmail, chainID)

	// Hash the init code (bytecode + constructor args)
	initCodeHash := crypto.Keccak256Hash(initCode)

	// CREATE2 formula: keccak256(0xff ++ factoryAddress ++ salt ++ keccak256(initCode))
	// Build the data: 1 byte (0xff) + 20 bytes (address) + 32 bytes (salt) + 32 bytes (initCodeHash)
	data := make([]byte, 1+20+32+32)
	data[0] = 0xff
	copy(data[1:21], ArachnidFactoryAddress.Bytes())
	copy(data[21:53], salt[:])
	copy(data[53:85], initCodeHash.Bytes())

	// Hash and take last 20 bytes as address
	hash := crypto.Keccak256(data)
	address := common.BytesToAddress(hash[12:])

	return address, nil
}

// GenerateSalt generates a deterministic salt for CREATE2 from user email and chain ID
func GenerateSalt(userEmail, chainID string) [32]byte {
	saltInput := fmt.Sprintf("%s:%s", userEmail, chainID)
	return sha256.Sum256([]byte(saltInput))
}

// VerifyForwarderAddress verifies that a given address matches the expected CREATE2 address
func VerifyForwarderAddress(
	expectedAddress common.Address,
	userEmail string,
	chainID string,
	initCode []byte,
) (bool, error) {
	computedAddress, err := ComputeForwarderAddress(userEmail, chainID, initCode)
	if err != nil {
		return false, err
	}
	return expectedAddress == computedAddress, nil
}
