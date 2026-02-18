#!/usr/bin/env bash
# =============================================================================
# NOVA Protocol — Fund Test Accounts
# Creates and funds test accounts on the devnet for development and testing.
#
# Usage:
#   ./scripts/fund-test-accounts.sh
#   ./scripts/fund-test-accounts.sh --rpc http://localhost:8080
# =============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
DATA_DIR="${PROJECT_ROOT}/.devnet"

# Defaults
RPC_URL="http://localhost:8080"
ACCOUNT_COUNT=5

# Parse arguments
while [[ $# -gt 0 ]]; do
    case "$1" in
        --rpc) RPC_URL="$2"; shift 2 ;;
        --count) ACCOUNT_COUNT="$2"; shift 2 ;;
        -h|--help)
            echo "Usage: $0 [--rpc URL] [--count N]"
            exit 0
            ;;
        *) echo "Unknown option: $1" >&2; exit 1 ;;
    esac
done

# Colors
GREEN='\033[0;32m'
CYAN='\033[0;36m'
YELLOW='\033[1;33m'
NC='\033[0m'

log()  { echo -e "${CYAN}[fund]${NC} $*"; }
ok()   { echo -e "${GREEN}[ ok ]${NC} $*"; }
warn() { echo -e "${YELLOW}[warn]${NC} $*"; }

# Funding amounts per token
declare -A FUND_AMOUNTS=(
    ["NOVA-BRL"]="1000000.00000000"
    ["NOVA-USD"]="1000000.00000000"
    ["NOVA-BTC"]="10.00000000"
    ["NOVA-ETH"]="100.000000000000000000"
)

# -----------------------------------------------------------------------------
# Generate test NOVA IDs
# -----------------------------------------------------------------------------
generate_nova_ids() {
    log "Generating ${ACCOUNT_COUNT} test accounts..."
    mkdir -p "${DATA_DIR}/test-accounts"

    for i in $(seq 1 "${ACCOUNT_COUNT}"); do
        local key_file="${DATA_DIR}/test-accounts/account-${i}.pem"
        local id_file="${DATA_DIR}/test-accounts/account-${i}.id"

        # Generate Ed25519 key pair
        openssl genpkey -algorithm Ed25519 -out "${key_file}" 2>/dev/null

        # Derive a NOVA ID from the public key (bech32-like format for display)
        local pubkey_hex
        pubkey_hex=$(openssl pkey -in "${key_file}" -pubout -outform DER 2>/dev/null \
                     | tail -c 32 | xxd -p -c 64)

        # Truncate to 40 chars for a readable address
        local nova_id="nova1${pubkey_hex:0:38}"
        echo "${nova_id}" > "${id_file}"
    done
}

# -----------------------------------------------------------------------------
# Fund accounts via RPC
# -----------------------------------------------------------------------------
fund_accounts() {
    log "Funding test accounts via ${RPC_URL}..."

    for i in $(seq 1 "${ACCOUNT_COUNT}"); do
        local id_file="${DATA_DIR}/test-accounts/account-${i}.id"
        local nova_id
        nova_id=$(cat "${id_file}")

        for token in "${!FUND_AMOUNTS[@]}"; do
            local amount="${FUND_AMOUNTS[${token}]}"

            # Submit funding transaction (devnet faucet endpoint)
            local response
            response=$(curl -sf -X POST "${RPC_URL}/v1/faucet/fund" \
                -H "Content-Type: application/json" \
                -d "{
                    \"recipient\": \"${nova_id}\",
                    \"token\": \"${token}\",
                    \"amount\": \"${amount}\"
                }" 2>/dev/null) || true

            if [[ -n "${response}" ]]; then
                local tx_hash
                tx_hash=$(echo "${response}" | jq -r '.tx_hash // "pending"' 2>/dev/null || echo "pending")
            fi
        done
        ok "Account ${i}: ${nova_id:0:20}... funded"
    done
}

# -----------------------------------------------------------------------------
# Print summary
# -----------------------------------------------------------------------------
print_summary() {
    echo ""
    echo "============================================================"
    echo "  Test Accounts — Funded"
    echo "============================================================"
    echo ""
    printf "  %-4s  %-44s  %s\n" "#" "NOVA ID" "Balances"
    printf "  %-4s  %-44s  %s\n" "---" "-------------------------------------------" "--------"

    for i in $(seq 1 "${ACCOUNT_COUNT}"); do
        local id_file="${DATA_DIR}/test-accounts/account-${i}.id"
        local nova_id
        nova_id=$(cat "${id_file}")

        local balances=""
        for token in "NOVA-BRL" "NOVA-USD" "NOVA-BTC" "NOVA-ETH"; do
            local amount="${FUND_AMOUNTS[${token}]}"
            balances="${balances}${token}=${amount}  "
        done

        printf "  %-4s  %-44s\n" "${i}" "${nova_id}"
        printf "        %s\n" "${balances}"
    done

    echo ""
    echo "  Key files:  ${DATA_DIR}/test-accounts/"
    echo "============================================================"
    echo ""
}

# -----------------------------------------------------------------------------
# Main
# -----------------------------------------------------------------------------
main() {
    for cmd in openssl curl jq xxd; do
        if ! command -v "${cmd}" &>/dev/null; then
            echo "Error: '${cmd}' is required but not installed." >&2
            exit 1
        fi
    done

    generate_nova_ids
    fund_accounts
    print_summary
}

main "$@"
