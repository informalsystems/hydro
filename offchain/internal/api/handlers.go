package api

import (
	"encoding/json"
	"fmt"
	"net/http"
	"strconv"

	"github.com/gorilla/mux"
	"go.uber.org/zap"

	"hydro/offchain/internal/database"
	"hydro/offchain/internal/service"
)

// Handler holds dependencies for HTTP handlers
type Handler struct {
	db              *database.DB
	contractService *service.ContractService
	feeService      *service.FeeService
	logger          *zap.Logger
}

// NewHandler creates a new API handler
func NewHandler(
	db *database.DB,
	contractService *service.ContractService,
	feeService *service.FeeService,
	logger *zap.Logger,
) *Handler {
	return &Handler{
		db:              db,
		contractService: contractService,
		feeService:      feeService,
		logger:          logger,
	}
}

// ==================== Health Check ====================

// HandleHealth returns service health status
func (h *Handler) HandleHealth(w http.ResponseWriter, r *http.Request) {
	response := HealthResponse{
		Status:  "ok",
		Version: "1.0.0",
	}
	respondJSON(w, http.StatusOK, response)
}

// ==================== Contract Addresses ====================

// HandleGetContractAddresses handles POST /api/v1/contracts/addresses
// Gets or creates contract addresses for a user on specified chains
func (h *Handler) HandleGetContractAddresses(w http.ResponseWriter, r *http.Request) {
	var req GetContractAddressesRequest
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		h.logger.Error("Failed to decode request", zap.Error(err))
		respondError(w, http.StatusBadRequest, "Invalid request body", err)
		return
	}

	// Validate request
	if req.Email == "" {
		respondError(w, http.StatusBadRequest, "Email is required", nil)
		return
	}
	if len(req.ChainIDs) == 0 {
		respondError(w, http.StatusBadRequest, "At least one chain_id is required", nil)
		return
	}

	h.logger.Info("Getting contract addresses",
		zap.String("email", req.Email),
		zap.Strings("chain_ids", req.ChainIDs))

	// Get or create contract addresses
	addresses, err := h.contractService.GetOrCreateContractAddresses(r.Context(), req.Email, req.ChainIDs)
	if err != nil {
		h.logger.Error("Failed to get contract addresses",
			zap.String("email", req.Email),
			zap.Error(err))
		respondError(w, http.StatusInternalServerError, "Failed to get contract addresses", err)
		return
	}

	// Build response
	contracts := make(map[string]ChainContracts)
	for chainID, addr := range addresses {
		contracts[chainID] = ChainContracts{
			Forwarder: addr.Forwarder,
			Proxy:     addr.Proxy,
		}
	}

	response := GetContractAddressesResponse{
		Email:     req.Email,
		Contracts: contracts,
	}

	respondJSON(w, http.StatusOK, response)
}

// ==================== Fee Calculation ====================

// HandleCalculateFee handles POST /api/v1/fees/calculate
// Calculates bridge fee for a given chain and amount
func (h *Handler) HandleCalculateFee(w http.ResponseWriter, r *http.Request) {
	var req CalculateFeeRequest
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		h.logger.Error("Failed to decode request", zap.Error(err))
		respondError(w, http.StatusBadRequest, "Invalid request body", err)
		return
	}

	// Validate request
	if req.ChainID == "" {
		respondError(w, http.StatusBadRequest, "chain_id is required", nil)
		return
	}
	if req.AmountUSDC == "" {
		respondError(w, http.StatusBadRequest, "amount_usdc is required", nil)
		return
	}

	// Parse amount
	amountUSDC, err := strconv.ParseInt(req.AmountUSDC, 10, 64)
	if err != nil {
		respondError(w, http.StatusBadRequest, "Invalid amount_usdc: must be an integer", err)
		return
	}
	if amountUSDC <= 0 {
		respondError(w, http.StatusBadRequest, "amount_usdc must be positive", nil)
		return
	}

	h.logger.Debug("Calculating fee",
		zap.String("chain_id", req.ChainID),
		zap.Int64("amount_usdc", amountUSDC))

	// Calculate fee
	feeCalc, err := h.feeService.CalculateBridgeFee(req.ChainID, amountUSDC)
	if err != nil {
		h.logger.Error("Failed to calculate fee",
			zap.String("chain_id", req.ChainID),
			zap.Error(err))
		respondError(w, http.StatusInternalServerError, "Failed to calculate fee", err)
		return
	}

	response := CalculateFeeResponse{
		BridgeFeeUSDC:  fmt.Sprintf("%d", feeCalc.BridgeFeeUSDC),
		MinDepositUSDC: fmt.Sprintf("%d", feeCalc.MinDepositUSDC),
	}

	respondJSON(w, http.StatusOK, response)
}

// ==================== Process Status ====================

// HandleGetProcessStatus handles GET /api/v1/processes/status/:processId
// Gets the status of a specific process
func (h *Handler) HandleGetProcessStatus(w http.ResponseWriter, r *http.Request) {
	vars := mux.Vars(r)
	processID := vars["processId"]

	if processID == "" {
		respondError(w, http.StatusBadRequest, "process_id is required", nil)
		return
	}

	h.logger.Debug("Getting process status", zap.String("process_id", processID))

	// Get process from database
	process, err := h.db.GetProcessByProcessID(r.Context(), processID)
	if err != nil {
		h.logger.Error("Failed to get process",
			zap.String("process_id", processID),
			zap.Error(err))
		respondError(w, http.StatusInternalServerError, "Failed to get process", err)
		return
	}
	if process == nil {
		respondError(w, http.StatusNotFound, "Process not found", nil)
		return
	}

	// Build response
	var amountUSDC *string
	if process.AmountUSDC != nil {
		amt := fmt.Sprintf("%d", *process.AmountUSDC)
		amountUSDC = &amt
	}

	response := GetProcessStatusResponse{
		ProcessID:  process.ProcessID,
		Status:     process.Status,
		AmountUSDC: amountUSDC,
		TxHashes: TxHashes{
			Bridge:  process.BridgeTxHash,
			Deposit: process.DepositTxHash,
		},
		Error: process.ErrorMessage,
	}

	respondJSON(w, http.StatusOK, response)
}

// ==================== User Processes ====================

// HandleGetUserProcesses handles GET /api/v1/processes/user/:email
// Gets all processes for a user
func (h *Handler) HandleGetUserProcesses(w http.ResponseWriter, r *http.Request) {
	vars := mux.Vars(r)
	email := vars["email"]

	if email == "" {
		respondError(w, http.StatusBadRequest, "email is required", nil)
		return
	}

	// Parse pagination parameters (optional)
	limit := 50 // default
	offset := 0 // default

	if limitStr := r.URL.Query().Get("limit"); limitStr != "" {
		if parsedLimit, err := strconv.Atoi(limitStr); err == nil && parsedLimit > 0 {
			limit = parsedLimit
		}
	}

	if offsetStr := r.URL.Query().Get("offset"); offsetStr != "" {
		if parsedOffset, err := strconv.Atoi(offsetStr); err == nil && parsedOffset >= 0 {
			offset = parsedOffset
		}
	}

	h.logger.Debug("Getting user processes",
		zap.String("email", email),
		zap.Int("limit", limit),
		zap.Int("offset", offset))

	// Get processes from database
	processes, err := h.db.GetProcessesByUser(r.Context(), email, limit, offset)
	if err != nil {
		h.logger.Error("Failed to get user processes",
			zap.String("email", email),
			zap.Error(err))
		respondError(w, http.StatusInternalServerError, "Failed to get processes", err)
		return
	}

	// Build response
	processSummaries := make([]ProcessSummary, 0, len(processes))
	for _, proc := range processes {
		var amountUSDC *string
		if proc.AmountUSDC != nil {
			amt := fmt.Sprintf("%d", *proc.AmountUSDC)
			amountUSDC = &amt
		}

		processSummaries = append(processSummaries, ProcessSummary{
			ProcessID:        proc.ProcessID,
			ChainID:          proc.ChainID,
			ForwarderAddress: proc.ForwarderAddress,
			ProxyAddress:     proc.ProxyAddress,
			Status:           proc.Status,
			AmountUSDC:       amountUSDC,
			TxHashes: TxHashes{
				Bridge:  proc.BridgeTxHash,
				Deposit: proc.DepositTxHash,
			},
			Error: proc.ErrorMessage,
		})
	}

	response := GetUserProcessesResponse{
		Processes: processSummaries,
	}

	respondJSON(w, http.StatusOK, response)
}

// ==================== Helper Functions ====================

// respondJSON sends a JSON response
func respondJSON(w http.ResponseWriter, statusCode int, data interface{}) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(statusCode)
	if err := json.NewEncoder(w).Encode(data); err != nil {
		// Log error but can't send response since headers already written
		fmt.Printf("Failed to encode JSON response: %v\n", err)
	}
}

// respondError sends an error response
func respondError(w http.ResponseWriter, statusCode int, message string, err error) {
	errorMsg := message
	if err != nil {
		errorMsg = fmt.Sprintf("%s: %v", message, err)
	}

	response := ErrorResponse{
		Error:   message,
		Message: errorMsg,
	}

	respondJSON(w, statusCode, response)
}
