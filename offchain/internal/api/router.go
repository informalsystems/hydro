package api

import (
	"net/http"
	"time"

	"github.com/gorilla/mux"
	"go.uber.org/zap"
)

// SetupRouter creates and configures the HTTP router
func SetupRouter(handler *Handler, logger *zap.Logger) *mux.Router {
	router := mux.NewRouter()

	// Apply middleware
	router.Use(loggingMiddleware(logger))
	router.Use(corsMiddleware())
	router.Use(recoveryMiddleware(logger))

	// Health check endpoint
	router.HandleFunc("/health", handler.HandleHealth).Methods(http.MethodGet)

	// API v1 routes
	api := router.PathPrefix("/api/v1").Subrouter()

	// Contract addresses
	api.HandleFunc("/contracts/addresses", handler.HandleGetContractAddresses).Methods(http.MethodPost)

	// Fee calculation
	api.HandleFunc("/fees/calculate", handler.HandleCalculateFee).Methods(http.MethodPost)

	// Process status
	api.HandleFunc("/processes/status/{processId}", handler.HandleGetProcessStatus).Methods(http.MethodGet)

	// User processes
	api.HandleFunc("/processes/user/{email}", handler.HandleGetUserProcesses).Methods(http.MethodGet)

	return router
}

// ==================== Middleware ====================

// loggingMiddleware logs HTTP requests
func loggingMiddleware(logger *zap.Logger) mux.MiddlewareFunc {
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			start := time.Now()

			// Wrap response writer to capture status code
			wrapped := &responseWriter{ResponseWriter: w, statusCode: http.StatusOK}

			next.ServeHTTP(wrapped, r)

			logger.Info("HTTP request",
				zap.String("method", r.Method),
				zap.String("path", r.URL.Path),
				zap.Int("status", wrapped.statusCode),
				zap.Duration("duration", time.Since(start)),
				zap.String("remote_addr", r.RemoteAddr),
			)
		})
	}
}

// responseWriter wraps http.ResponseWriter to capture status code
type responseWriter struct {
	http.ResponseWriter
	statusCode int
}

func (rw *responseWriter) WriteHeader(code int) {
	rw.statusCode = code
	rw.ResponseWriter.WriteHeader(code)
}

// corsMiddleware adds CORS headers
func corsMiddleware() mux.MiddlewareFunc {
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			// Allow all origins for now (can be restricted later)
			w.Header().Set("Access-Control-Allow-Origin", "*")
			w.Header().Set("Access-Control-Allow-Methods", "GET, POST, PUT, DELETE, OPTIONS")
			w.Header().Set("Access-Control-Allow-Headers", "Content-Type, Authorization")

			// Handle preflight requests
			if r.Method == http.MethodOptions {
				w.WriteHeader(http.StatusOK)
				return
			}

			next.ServeHTTP(w, r)
		})
	}
}

// recoveryMiddleware recovers from panics and logs them
func recoveryMiddleware(logger *zap.Logger) mux.MiddlewareFunc {
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			defer func() {
				if err := recover(); err != nil {
					logger.Error("Panic recovered",
						zap.Any("error", err),
						zap.String("method", r.Method),
						zap.String("path", r.URL.Path),
					)

					// Send error response
					w.Header().Set("Content-Type", "application/json")
					w.WriteHeader(http.StatusInternalServerError)
					w.Write([]byte(`{"error":"Internal server error","message":"An unexpected error occurred"}`))
				}
			}()

			next.ServeHTTP(w, r)
		})
	}
}
