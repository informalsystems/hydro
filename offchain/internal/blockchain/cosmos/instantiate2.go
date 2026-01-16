package cosmos

import (
	"crypto/sha256"
	"encoding/binary"
	"fmt"

	"github.com/btcsuite/btcutil/bech32"
)

// ComputeProxyAddress computes the deterministic address for a CosmWasm contract
// using instantiate2. This allows us to know the contract address before deployment.
//
// CosmWasm instantiate2 formula:
// address = sha256("contract_addr" ++ code_id ++ salt ++ creator_canonical_address)
//
// Parameters:
//   - codeID: The code ID of the stored contract on Neutron
//   - creatorAddress: The bech32 address that will instantiate the contract (operator)
//   - userEmail: User's email address (used to generate deterministic salt)
//
// Returns the computed bech32 address with "neutron" prefix
func ComputeProxyAddress(
	codeID uint64,
	creatorAddress string,
	userEmail string,
) (string, error) {
	if codeID == 0 {
		return "", fmt.Errorf("code ID cannot be zero")
	}
	if creatorAddress == "" {
		return "", fmt.Errorf("creator address cannot be empty")
	}
	if userEmail == "" {
		return "", fmt.Errorf("user email cannot be empty")
	}

	// Generate deterministic salt from userEmail
	// Note: For proxy, we don't include chainID since proxy is shared across all EVM chains
	saltInput := fmt.Sprintf("%s:proxy", userEmail)
	salt := sha256.Sum256([]byte(saltInput))

	// Convert creator address from bech32 to canonical (raw bytes)
	_, creatorCanonicalData, err := bech32.Decode(creatorAddress)
	if err != nil {
		return "", fmt.Errorf("failed to decode creator address: %w", err)
	}

	// Convert from 5-bit encoding to 8-bit
	creatorCanonical, err := bech32.ConvertBits(creatorCanonicalData, 5, 8, false)
	if err != nil {
		return "", fmt.Errorf("failed to convert creator address bits: %w", err)
	}

	// Build data for hashing according to CosmWasm instantiate2 spec
	// Format: "contract_addr" ++ code_id (big-endian uint64) ++ salt (32 bytes) ++ creator_canonical
	data := []byte("contract_addr")

	// Encode code_id as big-endian uint64 (8 bytes)
	codeIDBytes := make([]byte, 8)
	binary.BigEndian.PutUint64(codeIDBytes, codeID)
	data = append(data, codeIDBytes...)

	// Append salt (32 bytes)
	data = append(data, salt[:]...)

	// Append creator canonical address
	data = append(data, creatorCanonical...)

	// Hash the data
	hash := sha256.Sum256(data)

	// Take first 20 bytes for address (standard Cosmos address length)
	addressBytes := hash[:20]

	// Convert to 5-bit encoding for bech32
	addressData, err := bech32.ConvertBits(addressBytes, 8, 5, true)
	if err != nil {
		return "", fmt.Errorf("failed to convert address bits: %w", err)
	}

	// Encode as bech32 with "neutron" prefix
	proxyAddress, err := bech32.Encode("neutron", addressData)
	if err != nil {
		return "", fmt.Errorf("failed to encode bech32 address: %w", err)
	}

	return proxyAddress, nil
}

// GenerateProxySalt generates a deterministic salt for proxy instantiate2
func GenerateProxySalt(userEmail string) [32]byte {
	saltInput := fmt.Sprintf("%s:proxy", userEmail)
	return sha256.Sum256([]byte(saltInput))
}

// VerifyProxyAddress verifies that a given address matches the expected instantiate2 address
func VerifyProxyAddress(
	expectedAddress string,
	codeID uint64,
	creatorAddress string,
	userEmail string,
) (bool, error) {
	computedAddress, err := ComputeProxyAddress(codeID, creatorAddress, userEmail)
	if err != nil {
		return false, err
	}
	return expectedAddress == computedAddress, nil
}
