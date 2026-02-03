package cosmos

import (
	"crypto/sha256"
	"encoding/binary"
	"fmt"

	"github.com/btcsuite/btcutil/bech32"
)

// ComputeProxyAddress computes the deterministic address for a CosmWasm contract
// using instantiate2. This matches the wasmd implementation for predictable addresses.
//
// CosmWasm instantiate2 formula (from wasmd/x/wasm/keeper/addresses.go):
//  1. Build address key with 8-byte big-endian length prefixes:
//     key = len(checksum) || checksum || len(creator) || creator || len(salt) || salt || len(msg) || msg
//  2. Build module key: "wasm" || 0x00
//  3. Compute: th = sha256("module")
//  4. Compute: address = sha256(th || moduleKey || addressKey)
//
// Parameters:
//   - codeChecksum: The SHA256 checksum of the wasm bytecode (32 bytes)
//   - creatorAddress: The bech32 address that will instantiate the contract (operator)
//   - salt: The salt bytes for deterministic address derivation
//   - msg: The instantiate message bytes (can be nil for FixMsg=false)
//
// Returns the computed bech32 address with "neutron" prefix (32-byte address)
func ComputeProxyAddress(
	codeChecksum []byte,
	creatorAddress string,
	salt []byte,
	msg []byte,
) (string, error) {
	if len(codeChecksum) != 32 {
		return "", fmt.Errorf("code checksum must be 32 bytes, got %d", len(codeChecksum))
	}
	if creatorAddress == "" {
		return "", fmt.Errorf("creator address cannot be empty")
	}
	if len(salt) == 0 {
		return "", fmt.Errorf("salt cannot be empty")
	}

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

	// Build contract address key with 8-byte big-endian length prefixes
	// (from wasmd BuildContractAddressPredictable using UInt64LengthPrefix)
	addressKey := make([]byte, 0, 8+len(codeChecksum)+8+len(creatorCanonical)+8+len(salt)+8+len(msg))
	addressKey = append(addressKey, uint64ToBytes(uint64(len(codeChecksum)))...)
	addressKey = append(addressKey, codeChecksum...)
	addressKey = append(addressKey, uint64ToBytes(uint64(len(creatorCanonical)))...)
	addressKey = append(addressKey, creatorCanonical...)
	addressKey = append(addressKey, uint64ToBytes(uint64(len(salt)))...)
	addressKey = append(addressKey, salt...)
	addressKey = append(addressKey, uint64ToBytes(uint64(len(msg)))...)
	if len(msg) > 0 {
		addressKey = append(addressKey, msg...)
	}

	// Build module key: "wasm" + 0x00 (from cosmos-sdk address.Module)
	moduleKey := append([]byte("wasm"), 0)

	// Compute address using cosmos-sdk address.Module with Hash("module", mKey || addressKey)
	// Step 1: th = sha256("module")
	typeHash := sha256.Sum256([]byte("module"))

	// Step 2: address = sha256(th || moduleKey || addressKey)
	hasher := sha256.New()
	hasher.Write(typeHash[:])
	hasher.Write(moduleKey)
	hasher.Write(addressKey)
	addressBytes := hasher.Sum(nil) // Full 32-byte hash

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

// uint64ToBytes converts a uint64 to 8-byte big-endian encoding
// (matches sdk.Uint64ToBigEndian used in UInt64LengthPrefix)
func uint64ToBytes(v uint64) []byte {
	b := make([]byte, 8)
	binary.BigEndian.PutUint64(b, v)
	return b
}

// GenerateProxySalt generates a deterministic salt for proxy instantiate2
func GenerateProxySalt(userEmail string) [32]byte {
	saltInput := fmt.Sprintf("%s:proxy", userEmail)
	return sha256.Sum256([]byte(saltInput))
}

// VerifyProxyAddress verifies that a given address matches the expected instantiate2 address
func VerifyProxyAddress(
	expectedAddress string,
	codeChecksum []byte,
	creatorAddress string,
	salt []byte,
	msg []byte,
) (bool, error) {
	computedAddress, err := ComputeProxyAddress(codeChecksum, creatorAddress, salt, msg)
	if err != nil {
		return false, err
	}
	return expectedAddress == computedAddress, nil
}
