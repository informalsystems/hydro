# syntax=docker/dockerfile:1

# --------------------------------------------------------
# Stage 1: Build neutron-query-relayer
# --------------------------------------------------------
    FROM golang:1.22-bullseye AS builder

    # Set up build arguments
    ARG LDFLAGS
    
    # Prepare the build environment
    WORKDIR /app
    
    # Clone the neutron-query-relayer repository
    RUN apt-get update && apt-get install -y git && \
        git clone --depth 1 https://github.com/neutron-org/neutron-query-relayer.git /app
    
    # Download Go modules
    RUN go mod download
    
    # Build the neutron-query-relayer binary
    RUN go build -ldflags "${LDFLAGS}" -a -o build/neutron_query_relayer ./cmd/neutron_query_relayer/*.go
    
    # --------------------------------------------------------
    # Stage 2: Final image with all dependencies
    # --------------------------------------------------------
    FROM alpine:3.21.0
    
    # Install dependencies
    RUN apk add --no-cache \
        bash \
        curl \
        ca-certificates \
        git \
        jq
    
    # Set the desired version of neutrond
    ARG NEUTROND_VERSION="v5.0.2"
    ARG NEUTROND_BINARY="neutrond-linux-amd64"
    ARG NEUTROND_DOWNLOAD_URL="https://github.com/neutron-org/neutron/releases/download/${NEUTROND_VERSION}/${NEUTROND_BINARY}"
    
    # Download and install the neutrond binary
    RUN curl -L ${NEUTROND_DOWNLOAD_URL} -o /usr/local/bin/neutrond && \
        chmod +x /usr/local/bin/neutrond
    
    # Add the neutron-query-relayer binary from the builder stage
    COPY --from=builder /app/build/neutron_query_relayer /usr/local/bin/neutron_query_relayer
    
    # Add CosmWasm libraries
    ADD https://github.com/CosmWasm/wasmvm/releases/download/v1.5.2/libwasmvm.x86_64.so /lib/
    ADD https://github.com/CosmWasm/wasmvm/releases/download/v1.5.2/libwasmvm.aarch64.so /lib/
    
    # Copy scripts and other artifacts
    COPY tools /usr/local/hydro/tools
    COPY artifacts/ /usr/local/hydro/artifacts
    COPY .seed /usr/local/hydro/.seed
    
    # Set the default working directory
    WORKDIR /usr/local/hydro
    
    # Ensure scripts are executable
    RUN chmod +x tools/relaying.sh tools/deployment/*.sh

    # Import the seed phrase
    RUN neutrond keys add submitter --recover --keyring-backend test --source /usr/local/hydro/.seed

    
    # Expose the neutron-query-relayer port
    EXPOSE 9999
    
    # Default entry point
    ENTRYPOINT ["bash"]
    