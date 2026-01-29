package api

import (
	"bytes"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"

	"go.uber.org/zap"

	"hydro/offchain/internal/config"
	"hydro/offchain/internal/service"
)

func TestHandleHealth(t *testing.T) {
	logger := zap.NewNop()
	handler := NewHandler(nil, nil, nil, logger)

	req := httptest.NewRequest(http.MethodGet, "/health", nil)
	w := httptest.NewRecorder()

	handler.HandleHealth(w, req)

	if w.Code != http.StatusOK {
		t.Errorf("expected status %d, got %d", http.StatusOK, w.Code)
	}

	var response HealthResponse
	if err := json.NewDecoder(w.Body).Decode(&response); err != nil {
		t.Fatalf("failed to decode response: %v", err)
	}

	if response.Status != "ok" {
		t.Errorf("expected status 'ok', got '%s'", response.Status)
	}

	if response.Version != "1.0.0" {
		t.Errorf("expected version '1.0.0', got '%s'", response.Version)
	}
}

func TestHandleCalculateFee(t *testing.T) {
	logger := zap.NewNop()
	cfg := &config.Config{
		Chains: map[string]config.ChainConfig{
			"1": {
				OperationalFeeBps: 50,         // 0.5%
				MinOperationalFee: 1_000_000,  // 1 USDC
				MinDepositAmount:  10_000_000, // 10 USDC
			},
		},
	}
	feeService := service.NewFeeService(cfg, logger)
	handler := NewHandler(nil, nil, feeService, logger)

	tests := []struct {
		name           string
		request        CalculateFeeRequest
		expectedStatus int
		expectedFee    string
		expectedMinDep string
		expectError    bool
	}{
		{
			name: "valid request - min fee applies",
			request: CalculateFeeRequest{
				ChainID:    "1",
				AmountUSDC: "100000000", // 100 USDC
			},
			expectedStatus: http.StatusOK,
			expectedFee:    "1000000",   // 1 USDC (min fee applies since 0.5% of 100 = 0.5 < 1)
			expectedMinDep: "10000000",  // 10 USDC
		},
		{
			name: "valid request - minimum fee",
			request: CalculateFeeRequest{
				ChainID:    "1",
				AmountUSDC: "10000000", // 10 USDC
			},
			expectedStatus: http.StatusOK,
			expectedFee:    "1000000",   // 1 USDC (min fee)
			expectedMinDep: "10000000",  // 10 USDC
		},
		{
			name: "missing chain_id",
			request: CalculateFeeRequest{
				ChainID:    "",
				AmountUSDC: "100000000",
			},
			expectedStatus: http.StatusBadRequest,
			expectError:    true,
		},
		{
			name: "missing amount",
			request: CalculateFeeRequest{
				ChainID:    "1",
				AmountUSDC: "",
			},
			expectedStatus: http.StatusBadRequest,
			expectError:    true,
		},
		{
			name: "invalid amount - not a number",
			request: CalculateFeeRequest{
				ChainID:    "1",
				AmountUSDC: "invalid",
			},
			expectedStatus: http.StatusBadRequest,
			expectError:    true,
		},
		{
			name: "invalid amount - negative",
			request: CalculateFeeRequest{
				ChainID:    "1",
				AmountUSDC: "-100",
			},
			expectedStatus: http.StatusBadRequest,
			expectError:    true,
		},
		{
			name: "invalid amount - zero",
			request: CalculateFeeRequest{
				ChainID:    "1",
				AmountUSDC: "0",
			},
			expectedStatus: http.StatusBadRequest,
			expectError:    true,
		},
		{
			name: "unknown chain",
			request: CalculateFeeRequest{
				ChainID:    "999",
				AmountUSDC: "100000000",
			},
			expectedStatus: http.StatusInternalServerError,
			expectError:    true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			body, _ := json.Marshal(tt.request)
			req := httptest.NewRequest(http.MethodPost, "/api/v1/fees/calculate", bytes.NewReader(body))
			req.Header.Set("Content-Type", "application/json")
			w := httptest.NewRecorder()

			handler.HandleCalculateFee(w, req)

			if w.Code != tt.expectedStatus {
				t.Errorf("expected status %d, got %d", tt.expectedStatus, w.Code)
			}

			if tt.expectError {
				var errResp ErrorResponse
				if err := json.NewDecoder(w.Body).Decode(&errResp); err != nil {
					t.Fatalf("failed to decode error response: %v", err)
				}
				if errResp.Error == "" {
					t.Error("expected error message in response")
				}
				return
			}

			var response CalculateFeeResponse
			if err := json.NewDecoder(w.Body).Decode(&response); err != nil {
				t.Fatalf("failed to decode response: %v", err)
			}

			if response.BridgeFeeUSDC != tt.expectedFee {
				t.Errorf("expected fee %s, got %s", tt.expectedFee, response.BridgeFeeUSDC)
			}

			if response.MinDepositUSDC != tt.expectedMinDep {
				t.Errorf("expected min deposit %s, got %s", tt.expectedMinDep, response.MinDepositUSDC)
			}
		})
	}
}

func TestHandleGetContractAddresses_Validation(t *testing.T) {
	logger := zap.NewNop()
	handler := NewHandler(nil, nil, nil, logger)

	tests := []struct {
		name           string
		request        GetContractAddressesRequest
		expectedStatus int
	}{
		{
			name: "missing email",
			request: GetContractAddressesRequest{
				Email:    "",
				ChainIDs: []string{"1"},
			},
			expectedStatus: http.StatusBadRequest,
		},
		{
			name: "missing chain_ids",
			request: GetContractAddressesRequest{
				Email:    "test@example.com",
				ChainIDs: []string{},
			},
			expectedStatus: http.StatusBadRequest,
		},
		{
			name: "nil chain_ids",
			request: GetContractAddressesRequest{
				Email:    "test@example.com",
				ChainIDs: nil,
			},
			expectedStatus: http.StatusBadRequest,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			body, _ := json.Marshal(tt.request)
			req := httptest.NewRequest(http.MethodPost, "/api/v1/contracts/addresses", bytes.NewReader(body))
			req.Header.Set("Content-Type", "application/json")
			w := httptest.NewRecorder()

			handler.HandleGetContractAddresses(w, req)

			if w.Code != tt.expectedStatus {
				t.Errorf("expected status %d, got %d", tt.expectedStatus, w.Code)
			}
		})
	}
}

func TestHandleGetContractAddresses_InvalidJSON(t *testing.T) {
	logger := zap.NewNop()
	handler := NewHandler(nil, nil, nil, logger)

	req := httptest.NewRequest(http.MethodPost, "/api/v1/contracts/addresses", bytes.NewReader([]byte("invalid json")))
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()

	handler.HandleGetContractAddresses(w, req)

	if w.Code != http.StatusBadRequest {
		t.Errorf("expected status %d, got %d", http.StatusBadRequest, w.Code)
	}
}

func TestHandleCalculateFee_InvalidJSON(t *testing.T) {
	logger := zap.NewNop()
	handler := NewHandler(nil, nil, nil, logger)

	req := httptest.NewRequest(http.MethodPost, "/api/v1/fees/calculate", bytes.NewReader([]byte("invalid json")))
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()

	handler.HandleCalculateFee(w, req)

	if w.Code != http.StatusBadRequest {
		t.Errorf("expected status %d, got %d", http.StatusBadRequest, w.Code)
	}
}

func TestRespondJSON(t *testing.T) {
	w := httptest.NewRecorder()

	data := map[string]string{"key": "value"}
	respondJSON(w, http.StatusOK, data)

	if w.Code != http.StatusOK {
		t.Errorf("expected status %d, got %d", http.StatusOK, w.Code)
	}

	if ct := w.Header().Get("Content-Type"); ct != "application/json" {
		t.Errorf("expected content-type 'application/json', got '%s'", ct)
	}

	var result map[string]string
	if err := json.NewDecoder(w.Body).Decode(&result); err != nil {
		t.Fatalf("failed to decode response: %v", err)
	}

	if result["key"] != "value" {
		t.Errorf("expected key 'value', got '%s'", result["key"])
	}
}

func TestRespondError(t *testing.T) {
	tests := []struct {
		name           string
		statusCode     int
		message        string
		err            error
		expectedError  string
	}{
		{
			name:          "error without underlying error",
			statusCode:    http.StatusBadRequest,
			message:       "Bad request",
			err:           nil,
			expectedError: "Bad request",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			w := httptest.NewRecorder()
			respondError(w, tt.statusCode, tt.message, tt.err)

			if w.Code != tt.statusCode {
				t.Errorf("expected status %d, got %d", tt.statusCode, w.Code)
			}

			var errResp ErrorResponse
			if err := json.NewDecoder(w.Body).Decode(&errResp); err != nil {
				t.Fatalf("failed to decode response: %v", err)
			}

			if errResp.Error != tt.expectedError {
				t.Errorf("expected error '%s', got '%s'", tt.expectedError, errResp.Error)
			}
		})
	}
}
