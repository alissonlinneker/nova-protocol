#!/usr/bin/env bash
# =============================================================================
# NOVA Protocol — Genesis Block Generator
# Creates a genesis.json that bootstraps the network's initial state.
#
# Usage:
#   ./scripts/generate-genesis.sh
#   ./scripts/generate-genesis.sh --output /path/to/genesis.json --data-dir /path
# =============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

# Defaults
OUTPUT_FILE="${PROJECT_ROOT}/.devnet/genesis.json"
DATA_DIR="${PROJECT_ROOT}/.devnet"
CHAIN_ID="nova-devnet-1"
NODE_COUNT=4

# Parse arguments
while [[ $# -gt 0 ]]; do
    case "$1" in
        --output)  OUTPUT_FILE="$2"; shift 2 ;;
        --data-dir) DATA_DIR="$2"; shift 2 ;;
        --chain-id) CHAIN_ID="$2"; shift 2 ;;
        --nodes)    NODE_COUNT="$2"; shift 2 ;;
        -h|--help)
            echo "Usage: $0 [--output FILE] [--data-dir DIR] [--chain-id ID] [--nodes N]"
            exit 0
            ;;
        *) echo "Unknown option: $1" >&2; exit 1 ;;
    esac
done

mkdir -p "$(dirname "${OUTPUT_FILE}")"

# -----------------------------------------------------------------------------
# Collect validator public keys
# -----------------------------------------------------------------------------
validators_json="[]"
for i in $(seq 1 "${NODE_COUNT}"); do
    local_key_file="${DATA_DIR}/node-${i}/keys/validator.pub"
    if [[ -f "${local_key_file}" ]]; then
        pubkey=$(cat "${local_key_file}")
    else
        # Generate deterministic placeholder for standalone runs
        pubkey=$(printf "deadbeef%056d" "${i}")
    fi

    validator=$(jq -n \
        --arg id "node-${i}" \
        --arg pubkey "${pubkey}" \
        --argjson stake 1000000 \
        --argjson index "$((i - 1))" \
        '{
            id: $id,
            public_key: $pubkey,
            stake: $stake,
            index: $index
        }')
    validators_json=$(echo "${validators_json}" | jq --argjson v "${validator}" '. + [$v]')
done

# -----------------------------------------------------------------------------
# Initial token supplies
# -----------------------------------------------------------------------------
initial_supplies=$(jq -n '{
    "NOVA-BRL": {
        "name": "NOVA Brazilian Real",
        "symbol": "NOVA-BRL",
        "decimals": 8,
        "total_supply": "10000000000000000",
        "description": "Brazilian Real stablecoin on NOVA Protocol"
    },
    "NOVA-USD": {
        "name": "NOVA US Dollar",
        "symbol": "NOVA-USD",
        "decimals": 8,
        "total_supply": "10000000000000000",
        "description": "US Dollar stablecoin on NOVA Protocol"
    },
    "NOVA-BTC": {
        "name": "NOVA Wrapped Bitcoin",
        "symbol": "NOVA-BTC",
        "decimals": 8,
        "total_supply": "2100000000000000",
        "description": "Wrapped Bitcoin on NOVA Protocol"
    },
    "NOVA-ETH": {
        "name": "NOVA Wrapped Ether",
        "symbol": "NOVA-ETH",
        "decimals": 18,
        "total_supply": "100000000000000000000000000",
        "description": "Wrapped Ether on NOVA Protocol"
    }
}')

# -----------------------------------------------------------------------------
# Protocol parameters
# -----------------------------------------------------------------------------
protocol_params=$(jq -n '{
    block_time_ms: 2000,
    max_block_size_bytes: 2097152,
    max_transactions_per_block: 10000,
    min_validator_stake: 100000,
    epoch_length_blocks: 1000,
    slashing_penalty_percent: 5,
    reward_per_block: 100,
    max_validators: 100,
    zkp_verification_gas: 50000,
    transaction_fee_base: 10
}')

# -----------------------------------------------------------------------------
# Assemble genesis
# -----------------------------------------------------------------------------
genesis_time=$(date -u +"%Y-%m-%dT%H:%M:%SZ")

genesis=$(jq -n \
    --arg chain_id "${CHAIN_ID}" \
    --arg genesis_time "${genesis_time}" \
    --argjson validators "${validators_json}" \
    --argjson initial_supplies "${initial_supplies}" \
    --argjson protocol_params "${protocol_params}" \
    '{
        chain_id: $chain_id,
        genesis_time: $genesis_time,
        initial_height: 0,
        consensus: {
            algorithm: "nova-bft",
            params: {
                timeout_propose_ms: 3000,
                timeout_prevote_ms: 1000,
                timeout_precommit_ms: 1000,
                timeout_commit_ms: 1000
            }
        },
        validators: $validators,
        initial_supplies: $initial_supplies,
        protocol_params: $protocol_params,
        app_hash: "0000000000000000000000000000000000000000000000000000000000000000"
    }')

echo "${genesis}" | jq '.' > "${OUTPUT_FILE}"

echo "[genesis] Genesis block written to ${OUTPUT_FILE}"
echo "[genesis] Chain ID:    ${CHAIN_ID}"
echo "[genesis] Validators:  ${NODE_COUNT}"
echo "[genesis] Time:        ${genesis_time}"
