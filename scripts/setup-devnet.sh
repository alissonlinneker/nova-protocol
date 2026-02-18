#!/usr/bin/env bash
# =============================================================================
# NOVA Protocol — Local Devnet Setup
# Sets up a local development network with 4 validator nodes.
#
# Usage:
#   ./scripts/setup-devnet.sh           # start devnet
#   ./scripts/setup-devnet.sh --clean   # wipe state and start fresh
# =============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
DATA_DIR="${PROJECT_ROOT}/.devnet"
COMPOSE_FILE="${PROJECT_ROOT}/docker/docker-compose.yml"
NODE_COUNT=4

# -----------------------------------------------------------------------------
# Colors
# -----------------------------------------------------------------------------
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

log()  { echo -e "${CYAN}[devnet]${NC} $*"; }
ok()   { echo -e "${GREEN}[  ok  ]${NC} $*"; }
warn() { echo -e "${YELLOW}[ warn ]${NC} $*"; }
err()  { echo -e "${RED}[error ]${NC} $*" >&2; }

# -----------------------------------------------------------------------------
# Prerequisites
# -----------------------------------------------------------------------------
check_prerequisites() {
    local missing=0
    for cmd in docker jq openssl; do
        if ! command -v "${cmd}" &>/dev/null; then
            err "Required tool '${cmd}' is not installed."
            missing=1
        fi
    done
    if ! docker compose version &>/dev/null; then
        err "Docker Compose v2 is required (docker compose)."
        missing=1
    fi
    if [[ "${missing}" -eq 1 ]]; then
        exit 1
    fi
}

# -----------------------------------------------------------------------------
# Cleanup
# -----------------------------------------------------------------------------
clean() {
    log "Stopping any running devnet containers..."
    docker compose -f "${COMPOSE_FILE}" down -v --remove-orphans 2>/dev/null || true
    log "Removing devnet state directory ${DATA_DIR}..."
    rm -rf "${DATA_DIR}"
    ok "Clean complete."
}

# -----------------------------------------------------------------------------
# Key generation
# -----------------------------------------------------------------------------
generate_validator_keys() {
    log "Generating ${NODE_COUNT} validator key pairs..."
    for i in $(seq 1 "${NODE_COUNT}"); do
        local key_dir="${DATA_DIR}/node-${i}/keys"
        mkdir -p "${key_dir}"

        # Generate Ed25519 private key
        openssl genpkey -algorithm Ed25519 -out "${key_dir}/validator.pem" 2>/dev/null

        # Extract public key (hex-encoded)
        local pubkey
        pubkey=$(openssl pkey -in "${key_dir}/validator.pem" -pubout -outform DER 2>/dev/null \
                 | tail -c 32 | xxd -p -c 64)

        echo "${pubkey}" > "${key_dir}/validator.pub"

        ok "Node ${i}: pubkey=${pubkey:0:16}..."
    done
}

# -----------------------------------------------------------------------------
# Genesis block
# -----------------------------------------------------------------------------
generate_genesis() {
    log "Generating genesis block..."
    "${SCRIPT_DIR}/generate-genesis.sh" --output "${DATA_DIR}/genesis.json" --data-dir "${DATA_DIR}"
    ok "Genesis block written to ${DATA_DIR}/genesis.json"
}

# -----------------------------------------------------------------------------
# Distribute genesis to each node
# -----------------------------------------------------------------------------
distribute_genesis() {
    log "Distributing genesis.json to all nodes..."
    for i in $(seq 1 "${NODE_COUNT}"); do
        cp "${DATA_DIR}/genesis.json" "${DATA_DIR}/node-${i}/genesis.json"
    done
    ok "Genesis distributed."
}

# -----------------------------------------------------------------------------
# Start containers
# -----------------------------------------------------------------------------
start_nodes() {
    log "Building Docker images..."
    docker compose -f "${COMPOSE_FILE}" build --quiet

    log "Starting ${NODE_COUNT} validator nodes + API gateway + explorer..."
    docker compose -f "${COMPOSE_FILE}" up -d
}

# -----------------------------------------------------------------------------
# Wait for healthy nodes
# -----------------------------------------------------------------------------
wait_for_nodes() {
    log "Waiting for nodes to become healthy..."
    local max_retries=60
    local delay=2

    for i in $(seq 1 "${NODE_COUNT}"); do
        local port=$((8079 + i))
        local attempt=0
        while [[ "${attempt}" -lt "${max_retries}" ]]; do
            if curl -sf "http://localhost:${port}/health" &>/dev/null; then
                ok "nova-node-${i} (port ${port}) is healthy."
                break
            fi
            attempt=$((attempt + 1))
            sleep "${delay}"
        done
        if [[ "${attempt}" -ge "${max_retries}" ]]; then
            err "nova-node-${i} failed to become healthy within $((max_retries * delay))s."
            docker compose -f "${COMPOSE_FILE}" logs "nova-node-${i}" --tail=30
            exit 1
        fi
    done

    # Wait for API gateway
    local attempt=0
    while [[ "${attempt}" -lt "${max_retries}" ]]; do
        if curl -sf "http://localhost:8090/health" &>/dev/null; then
            ok "nova-api (port 8090) is healthy."
            break
        fi
        attempt=$((attempt + 1))
        sleep "${delay}"
    done
}

# -----------------------------------------------------------------------------
# Fund test accounts
# -----------------------------------------------------------------------------
fund_accounts() {
    log "Funding test accounts..."
    "${SCRIPT_DIR}/fund-test-accounts.sh" --rpc "http://localhost:8080"
}

# -----------------------------------------------------------------------------
# Status summary
# -----------------------------------------------------------------------------
print_summary() {
    echo ""
    echo "============================================================"
    echo "  NOVA Protocol — Devnet Running"
    echo "============================================================"
    echo ""
    echo "  Validator Nodes:"
    for i in $(seq 1 "${NODE_COUNT}"); do
        local port=$((8079 + i))
        echo "    node-${i}  RPC: http://localhost:${port}  P2P: :$((9089 + i))  Metrics: :$((9099 + i))"
    done
    echo ""
    echo "  Services:"
    echo "    API Gateway:     http://localhost:8090"
    echo "    Block Explorer:  http://localhost:3000"
    echo ""
    echo "  Data Directory:    ${DATA_DIR}"
    echo ""
    echo "  Useful commands:"
    echo "    make docker-logs          # follow container logs"
    echo "    make docker-down          # stop devnet"
    echo "    ./scripts/setup-devnet.sh --clean   # wipe and restart"
    echo ""
    echo "============================================================"
}

# -----------------------------------------------------------------------------
# Main
# -----------------------------------------------------------------------------
main() {
    check_prerequisites

    if [[ "${1:-}" == "--clean" ]]; then
        clean
    fi

    mkdir -p "${DATA_DIR}"

    generate_validator_keys
    generate_genesis
    distribute_genesis
    start_nodes
    wait_for_nodes
    fund_accounts
    print_summary
}

main "$@"
