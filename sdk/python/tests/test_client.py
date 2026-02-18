"""Tests for client, wallet, and transaction construction.

These tests exercise the public API surface without hitting a real NOVA
node. HTTP-level client methods are verified via httpx.MockTransport.
"""

from __future__ import annotations

import json
import struct

import httpx
import pytest

from nova_sdk.client import (
    AccountResponse,
    BlockResponse,
    NovaClient,
    NovaClientError,
    NovaConnectionError,
    NovaNotFoundError,
    NovaRpcError,
    SendTransactionResponse,
    StatusResponse,
    TransactionResponse,
)
from nova_sdk.identity import create_nova_id, generate_keypair, keypair_from_seed
from nova_sdk.transaction import (
    TransactionBuilder,
    compute_transaction_id,
    sign_transaction,
    signable_bytes,
    verify_transaction,
)
from nova_sdk.types import (
    Amount,
    NovaId,
    PublicKey,
    Signature,
    TransactionType,
)
from nova_sdk.wallet import NovaWallet


# ---------------------------------------------------------------------------
# Mock transport helpers
# ---------------------------------------------------------------------------

# Sample JSON payloads used by the HTTP tests.

_STATUS_JSON = {
    "version": "0.1.0",
    "network": "devnet",
    "block_height": 42,
    "peer_count": 5,
    "synced": True,
    "timestamp": "2024-01-01T00:00:00Z",
}

_BLOCK_JSON = {
    "height": 10,
    "hash": "ab" * 32,
    "parent_hash": "cd" * 32,
    "proposer": "nova1validator",
    "tx_count": 3,
    "timestamp": 1700000000,
}

_TX_JSON = {
    "hash": "ee" * 32,
    "sender": "nova1alice",
    "recipient": "nova1bob",
    "amount": 500_000,
    "fee": 100,
    "block_height": 10,
    "status": "confirmed",
    "timestamp": 1700000000,
}

_ACCOUNT_JSON = {
    "address": "nova1alice",
    "balance": 1_000_000,
    "nonce": 7,
    "tx_count": 7,
}

_SEND_TX_RESULT = {
    "tx_hash": "ff" * 32,
    "status": "pending",
}


def _make_rpc_response(result: object = None, error: object = None, req_id: int = 1) -> dict:
    resp: dict = {"jsonrpc": "2.0", "id": req_id}
    if error is not None:
        resp["error"] = error
    else:
        resp["result"] = result
    return resp


def _rest_handler(request: httpx.Request) -> httpx.Response:
    """Mock transport handler for REST endpoints."""
    path = request.url.path

    if path == "/health":
        return httpx.Response(200, json={"status": "ok"})

    if path == "/status":
        return httpx.Response(200, json=_STATUS_JSON)

    if path.startswith("/blocks/"):
        height = path.split("/")[-1]
        if height == "999":
            return httpx.Response(404, json={"error": "Block not found at height 999"})
        return httpx.Response(200, json=_BLOCK_JSON)

    if path.startswith("/transactions/"):
        tx_hash = path.split("/")[-1]
        if tx_hash == "missing":
            return httpx.Response(404, json={"error": "Transaction not found: missing"})
        return httpx.Response(200, json=_TX_JSON)

    if path.startswith("/accounts/"):
        return httpx.Response(200, json=_ACCOUNT_JSON)

    if path == "/rpc" and request.method == "POST":
        body = json.loads(request.content)
        method = body.get("method", "")
        req_id = body.get("id", 1)

        if method == "nova_blockHeight":
            return httpx.Response(200, json=_make_rpc_response(result=42, req_id=req_id))

        if method == "nova_peerCount":
            return httpx.Response(200, json=_make_rpc_response(result=5, req_id=req_id))

        if method == "nova_networkId":
            return httpx.Response(200, json=_make_rpc_response(result="devnet", req_id=req_id))

        if method == "nova_version":
            return httpx.Response(200, json=_make_rpc_response(result="0.1.0", req_id=req_id))

        if method == "nova_getBalance":
            return httpx.Response(
                200,
                json=_make_rpc_response(result={"balance": 1_000_000}, req_id=req_id),
            )

        if method == "nova_sendTransaction":
            return httpx.Response(
                200,
                json=_make_rpc_response(result=_SEND_TX_RESULT, req_id=req_id),
            )

        if method == "nova_errorMethod":
            return httpx.Response(
                200,
                json=_make_rpc_response(
                    error={"code": -32001, "message": "Something went wrong"},
                    req_id=req_id,
                ),
            )

        return httpx.Response(
            200,
            json=_make_rpc_response(
                error={"code": -32601, "message": f"Method not found: {method}"},
                req_id=req_id,
            ),
        )

    return httpx.Response(404, json={"error": "not found"})


def _build_mock_client(handler=_rest_handler) -> NovaClient:
    """Create a NovaClient backed by a mock transport (no real I/O)."""
    transport = httpx.MockTransport(handler)
    client = NovaClient("http://localhost:9070")
    client._client = httpx.AsyncClient(
        transport=transport,
        base_url="http://localhost:9070",
    )
    return client


# ---------------------------------------------------------------------------
# NovaClient construction
# ---------------------------------------------------------------------------


class TestClientConstruction:
    """Verify NovaClient initialises correctly without I/O."""

    def test_client_construction(self) -> None:
        client = NovaClient("http://localhost:9070")
        assert client._base_url == "http://localhost:9070"

    def test_trailing_slash_stripped(self) -> None:
        client = NovaClient("http://localhost:9070/")
        assert client._base_url == "http://localhost:9070"

    def test_custom_timeout(self) -> None:
        client = NovaClient("http://localhost:9070", timeout=5.0)
        assert client._timeout == 5.0

    def test_custom_retries(self) -> None:
        client = NovaClient("http://localhost:9070", retries=5)
        assert client._retries == 5

    def test_request_id_increments(self) -> None:
        client = NovaClient("http://localhost:9070")
        id1 = client._next_id()
        id2 = client._next_id()
        assert id2 == id1 + 1


# ---------------------------------------------------------------------------
# NovaClient REST endpoint tests
# ---------------------------------------------------------------------------


class TestClientRest:
    """REST endpoint tests with mocked HTTP transport."""

    @pytest.mark.asyncio
    async def test_health_returns_true(self) -> None:
        client = _build_mock_client()
        result = await client.health()
        assert result is True
        await client.close()

    @pytest.mark.asyncio
    async def test_health_returns_false_on_error(self) -> None:
        def error_handler(request: httpx.Request) -> httpx.Response:
            raise httpx.ConnectError("connection refused")

        client = _build_mock_client(handler=error_handler)
        result = await client.health()
        assert result is False
        await client.close()

    @pytest.mark.asyncio
    async def test_get_status(self) -> None:
        client = _build_mock_client()
        status = await client.get_status()
        assert isinstance(status, StatusResponse)
        assert status.version == "0.1.0"
        assert status.network == "devnet"
        assert status.block_height == 42
        assert status.peer_count == 5
        assert status.synced is True
        await client.close()

    @pytest.mark.asyncio
    async def test_get_block(self) -> None:
        client = _build_mock_client()
        block = await client.get_block(10)
        assert isinstance(block, BlockResponse)
        assert block.height == 10
        assert block.tx_count == 3
        assert block.proposer == "nova1validator"
        await client.close()

    @pytest.mark.asyncio
    async def test_get_block_not_found(self) -> None:
        client = _build_mock_client()
        with pytest.raises(NovaNotFoundError) as exc_info:
            await client.get_block(999)
        assert exc_info.value.status_code == 404
        await client.close()

    @pytest.mark.asyncio
    async def test_get_transaction(self) -> None:
        client = _build_mock_client()
        tx = await client.get_transaction("ee" * 32)
        assert isinstance(tx, TransactionResponse)
        assert tx.hash == "ee" * 32
        assert tx.sender == "nova1alice"
        assert tx.recipient == "nova1bob"
        assert tx.amount == 500_000
        assert tx.fee == 100
        await client.close()

    @pytest.mark.asyncio
    async def test_get_account(self) -> None:
        client = _build_mock_client()
        account = await client.get_account("nova1alice")
        assert isinstance(account, AccountResponse)
        assert account.address == "nova1alice"
        assert account.balance == 1_000_000
        assert account.nonce == 7
        await client.close()


# ---------------------------------------------------------------------------
# NovaClient JSON-RPC tests
# ---------------------------------------------------------------------------


class TestClientRpc:
    """JSON-RPC endpoint tests with mocked HTTP transport."""

    @pytest.mark.asyncio
    async def test_get_block_height_rpc(self) -> None:
        client = _build_mock_client()
        height = await client.get_block_height()
        assert height == 42
        await client.close()

    @pytest.mark.asyncio
    async def test_get_peer_count_rpc(self) -> None:
        client = _build_mock_client()
        count = await client.get_peer_count()
        assert count == 5
        await client.close()

    @pytest.mark.asyncio
    async def test_get_network_id_rpc(self) -> None:
        client = _build_mock_client()
        network = await client.get_network_id()
        assert network == "devnet"
        await client.close()

    @pytest.mark.asyncio
    async def test_get_version_rpc(self) -> None:
        client = _build_mock_client()
        version = await client.get_version()
        assert version == "0.1.0"
        await client.close()

    @pytest.mark.asyncio
    async def test_get_balance_rpc(self) -> None:
        client = _build_mock_client()
        balance = await client.get_balance("nova1alice")
        assert balance == 1_000_000
        await client.close()

    @pytest.mark.asyncio
    async def test_send_transaction_rpc(self) -> None:
        client = _build_mock_client()
        sk, pk = generate_keypair()
        sender_addr = create_nova_id(pk)
        _, rpk = generate_keypair()
        receiver_addr = create_nova_id(rpk)

        tx = (
            TransactionBuilder()
            .type(TransactionType.TRANSFER)
            .sender(sender_addr)
            .receiver(receiver_addr)
            .amount(100_000, "NOVA")
            .fee(10)
            .nonce(0)
            .build()
        )
        resp = await client.send_transaction(tx)
        assert isinstance(resp, SendTransactionResponse)
        assert resp.tx_hash == "ff" * 32
        assert resp.status == "pending"
        await client.close()

    @pytest.mark.asyncio
    async def test_rpc_error_handling(self) -> None:
        client = _build_mock_client()
        with pytest.raises(NovaRpcError) as exc_info:
            await client._rpc_call("nova_errorMethod")
        assert exc_info.value.code == -32001
        assert "Something went wrong" in exc_info.value.message
        await client.close()


# ---------------------------------------------------------------------------
# Connection error tests
# ---------------------------------------------------------------------------


class TestClientErrors:
    """Error propagation tests."""

    @pytest.mark.asyncio
    async def test_connection_error(self) -> None:
        def error_handler(request: httpx.Request) -> httpx.Response:
            raise httpx.ConnectError("connection refused")

        client = _build_mock_client(handler=error_handler)
        with pytest.raises(NovaConnectionError):
            await client.get_status()
        await client.close()

    @pytest.mark.asyncio
    async def test_context_manager(self) -> None:
        transport = httpx.MockTransport(_rest_handler)
        client = NovaClient("http://localhost:9070")
        client._client = httpx.AsyncClient(
            transport=transport,
            base_url="http://localhost:9070",
        )
        async with client as c:
            assert c is client
            status = await c.get_status()
            assert status.block_height == 42
        # After exiting the context, the inner client should be closed.
        assert client._client.is_closed


# ---------------------------------------------------------------------------
# Response model validation
# ---------------------------------------------------------------------------


class TestResponseModels:
    """Pydantic model validation for response types."""

    def test_status_response_validation(self) -> None:
        model = StatusResponse.model_validate(_STATUS_JSON)
        assert model.version == "0.1.0"
        assert model.block_height == 42

    def test_block_response_validation(self) -> None:
        model = BlockResponse.model_validate(_BLOCK_JSON)
        assert model.height == 10
        assert model.tx_count == 3

    def test_transaction_response_validation(self) -> None:
        model = TransactionResponse.model_validate(_TX_JSON)
        assert model.hash == "ee" * 32
        assert model.amount == 500_000

    def test_account_response_validation(self) -> None:
        model = AccountResponse.model_validate(_ACCOUNT_JSON)
        assert model.address == "nova1alice"
        assert model.balance == 1_000_000

    def test_send_transaction_response_validation(self) -> None:
        model = SendTransactionResponse.model_validate(_SEND_TX_RESULT)
        assert model.tx_hash == "ff" * 32
        assert model.status == "pending"

    def test_block_response_defaults(self) -> None:
        minimal = {"height": 0, "hash": "a", "parent_hash": "b", "proposer": "c", "tx_count": 0, "timestamp": 0}
        model = BlockResponse.model_validate(minimal)
        assert model.state_root == ""

    def test_transaction_response_defaults(self) -> None:
        minimal = {"hash": "a", "sender": "b", "recipient": "c", "amount": 0, "fee": 0}
        model = TransactionResponse.model_validate(minimal)
        assert model.block_height == 0
        assert model.status == "Pending"
        assert model.timestamp == 0

    def test_error_hierarchy(self) -> None:
        assert issubclass(NovaConnectionError, NovaClientError)
        assert issubclass(NovaNotFoundError, NovaClientError)
        assert issubclass(NovaRpcError, NovaClientError)


# ---------------------------------------------------------------------------
# NovaWallet
# ---------------------------------------------------------------------------


class TestWalletCreation:
    """Wallet lifecycle without network calls."""

    def test_create_produces_valid_address(self) -> None:
        wallet = NovaWallet.create()
        assert wallet.address.startswith("nova1")

    def test_from_seed_is_deterministic(self) -> None:
        seed = b"\x42" * 32
        w1 = NovaWallet.from_seed(seed)
        w2 = NovaWallet.from_seed(seed)
        assert w1.address == w2.address
        assert w1.public_key == w2.public_key

    def test_different_seeds_different_wallets(self) -> None:
        w1 = NovaWallet.from_seed(b"\x01" * 32)
        w2 = NovaWallet.from_seed(b"\x02" * 32)
        assert w1.address != w2.address

    def test_sign_returns_64_bytes(self) -> None:
        wallet = NovaWallet.create()
        sig = wallet.sign(b"hello")
        assert isinstance(sig, bytes)
        assert len(sig) == 64

    def test_public_key_is_32_bytes(self) -> None:
        wallet = NovaWallet.create()
        assert len(wallet.public_key) == 32


# ---------------------------------------------------------------------------
# TransactionBuilder
# ---------------------------------------------------------------------------


class TestTransactionBuilder:
    """Fluent transaction builder API."""

    @pytest.fixture()
    def addresses(self) -> tuple[str, str]:
        _, pk1 = generate_keypair()
        _, pk2 = generate_keypair()
        return create_nova_id(pk1), create_nova_id(pk2)

    def test_build_transfer(self, addresses: tuple[str, str]) -> None:
        sender, receiver = addresses
        tx = (
            TransactionBuilder()
            .type(TransactionType.TRANSFER)
            .sender(sender)
            .receiver(receiver)
            .amount(1_000_000, "NOVA")
            .fee(100)
            .nonce(1)
            .build()
        )
        assert tx.tx_type == TransactionType.TRANSFER
        assert tx.version == 1
        assert tx.sender == sender
        assert tx.receiver == receiver
        assert tx.amount.value == 1_000_000
        assert tx.amount.currency == "NOVA"
        assert tx.fee == 100
        assert tx.nonce == 1

    def test_missing_field_raises(self) -> None:
        with pytest.raises(ValueError, match="missing required fields"):
            TransactionBuilder().type(TransactionType.TRANSFER).build()

    def test_builder_returns_new_instance(self, addresses: tuple[str, str]) -> None:
        sender, receiver = addresses
        builder = TransactionBuilder()
        b2 = builder.type(TransactionType.TRANSFER)
        assert b2 is builder, "fluent methods return self"

    def test_payload_attached(self, addresses: tuple[str, str]) -> None:
        sender, receiver = addresses
        payload = b'{"memo": "test"}'
        tx = (
            TransactionBuilder()
            .type(TransactionType.TRANSFER)
            .sender(sender)
            .receiver(receiver)
            .amount(500, "BRL")
            .nonce(0)
            .payload(payload)
            .build()
        )
        assert tx.payload == payload

    def test_default_fee_is_zero(self, addresses: tuple[str, str]) -> None:
        sender, receiver = addresses
        tx = (
            TransactionBuilder()
            .type(TransactionType.TRANSFER)
            .sender(sender)
            .receiver(receiver)
            .amount(100, "NOVA")
            .nonce(0)
            .build()
        )
        assert tx.fee == 0

    def test_default_version_is_one(self, addresses: tuple[str, str]) -> None:
        sender, receiver = addresses
        tx = (
            TransactionBuilder()
            .type(TransactionType.TRANSFER)
            .sender(sender)
            .receiver(receiver)
            .amount(100, "NOVA")
            .nonce(0)
            .build()
        )
        assert tx.version == 1

    def test_version_override(self, addresses: tuple[str, str]) -> None:
        sender, receiver = addresses
        tx = (
            TransactionBuilder()
            .version(2)
            .type(TransactionType.TRANSFER)
            .sender(sender)
            .receiver(receiver)
            .amount(100, "NOVA")
            .nonce(0)
            .build()
        )
        assert tx.version == 2


# ---------------------------------------------------------------------------
# Signable bytes — canonical binary format
# ---------------------------------------------------------------------------


class TestSignableBytes:
    """Verify the binary layout matches the Rust protocol crate."""

    def test_no_payload_layout(self) -> None:
        _, pk1 = generate_keypair()
        _, pk2 = generate_keypair()
        sender = create_nova_id(pk1)
        receiver = create_nova_id(pk2)

        tx = (
            TransactionBuilder()
            .type(TransactionType.TRANSFER)
            .sender(sender)
            .receiver(receiver)
            .amount(1_000_000, "NOVA")
            .fee(100)
            .nonce(1)
            .timestamp(1_700_000_000_000)
            .build()
        )

        data = signable_bytes(tx)
        offset = 0

        # version: LE u16 = 1
        assert struct.unpack_from("<H", data, offset) == (1,)
        offset += 2

        # tx_type: "Transfer" + 0x00
        type_str = b"Transfer"
        assert data[offset : offset + len(type_str)] == type_str
        offset += len(type_str)
        assert data[offset] == 0x00
        offset += 1

        # sender + 0x00
        sender_bytes = sender.encode("utf-8")
        assert data[offset : offset + len(sender_bytes)] == sender_bytes
        offset += len(sender_bytes)
        assert data[offset] == 0x00
        offset += 1

        # receiver + 0x00
        receiver_bytes = receiver.encode("utf-8")
        assert data[offset : offset + len(receiver_bytes)] == receiver_bytes
        offset += len(receiver_bytes)
        assert data[offset] == 0x00
        offset += 1

        # amount.value: LE u64 = 1_000_000
        assert struct.unpack_from("<Q", data, offset) == (1_000_000,)
        offset += 8

        # currency: "NOVA" + 0x00
        assert data[offset : offset + 4] == b"NOVA"
        offset += 4
        assert data[offset] == 0x00
        offset += 1

        # fee: LE u64 = 100
        assert struct.unpack_from("<Q", data, offset) == (100,)
        offset += 8

        # nonce: LE u64 = 1
        assert struct.unpack_from("<Q", data, offset) == (1,)
        offset += 8

        # timestamp: LE u64 = 1_700_000_000_000
        assert struct.unpack_from("<Q", data, offset) == (1_700_000_000_000,)
        offset += 8

        # no payload flag: 0x00
        assert data[offset] == 0x00
        offset += 1

        assert offset == len(data)

    def test_with_payload_layout(self) -> None:
        _, pk1 = generate_keypair()
        _, pk2 = generate_keypair()
        sender = create_nova_id(pk1)
        receiver = create_nova_id(pk2)

        payload = b"hello"
        tx = (
            TransactionBuilder()
            .type(TransactionType.TRANSFER)
            .sender(sender)
            .receiver(receiver)
            .amount(0, "NOVA")
            .fee(0)
            .nonce(1)
            .timestamp(1_700_000_000_000)
            .payload(payload)
            .build()
        )

        data = signable_bytes(tx)
        # The last bytes should be: 0x01 + LE u32(5) + b"hello"
        payload_section = data[-(1 + 4 + len(payload)) :]
        assert payload_section[0] == 0x01
        assert struct.unpack_from("<I", payload_section, 1) == (len(payload),)
        assert payload_section[5:] == payload


# ---------------------------------------------------------------------------
# Transaction signing and verification
# ---------------------------------------------------------------------------


class TestTransactionSigning:
    """Sign and verify full transactions."""

    def test_sign_and_verify(self) -> None:
        sk, pk = generate_keypair()
        sender_addr = create_nova_id(pk)
        _, pk2 = generate_keypair()
        receiver_addr = create_nova_id(pk2)

        tx = (
            TransactionBuilder()
            .type(TransactionType.TRANSFER)
            .sender(sender_addr)
            .receiver(receiver_addr)
            .amount(500_000, "NOVA")
            .fee(50)
            .nonce(7)
            .build()
        )

        signed = sign_transaction(tx, sk)
        assert len(bytes(signed.signature)) == 64
        assert len(bytes(signed.public_key)) == 32
        assert verify_transaction(signed) is True

    def test_tampered_transaction_fails(self) -> None:
        sk, pk = generate_keypair()
        sender_addr = create_nova_id(pk)
        _, pk2 = generate_keypair()
        receiver_addr = create_nova_id(pk2)

        tx = (
            TransactionBuilder()
            .type(TransactionType.TRANSFER)
            .sender(sender_addr)
            .receiver(receiver_addr)
            .amount(500_000, "NOVA")
            .fee(50)
            .nonce(7)
            .build()
        )
        signed = sign_transaction(tx, sk)

        # Tamper with the amount.
        tampered_tx = tx.model_copy(update={"amount": Amount(value=999_999, currency="NOVA")})
        tampered_signed = signed.model_copy(update={"transaction": tampered_tx})
        assert verify_transaction(tampered_signed) is False

    def test_wrong_signer_fails(self) -> None:
        sk1, pk1 = generate_keypair()
        _, pk2 = generate_keypair()
        sk3, pk3 = generate_keypair()

        sender_addr = create_nova_id(pk1)
        receiver_addr = create_nova_id(pk2)

        tx = (
            TransactionBuilder()
            .type(TransactionType.TRANSFER)
            .sender(sender_addr)
            .receiver(receiver_addr)
            .amount(100, "NOVA")
            .nonce(0)
            .build()
        )

        # Sign with a different key than the sender.
        signed = sign_transaction(tx, sk3)
        assert verify_transaction(signed) is False

    def test_transaction_id_is_deterministic(self) -> None:
        sk, pk = generate_keypair()
        sender_addr = create_nova_id(pk)
        _, pk2 = generate_keypair()
        receiver_addr = create_nova_id(pk2)

        tx = (
            TransactionBuilder()
            .type(TransactionType.TRANSFER)
            .sender(sender_addr)
            .receiver(receiver_addr)
            .amount(100, "NOVA")
            .nonce(0)
            .timestamp(1700000000)
            .build()
        )
        id1 = compute_transaction_id(tx)
        id2 = compute_transaction_id(tx)
        assert id1 == id2
        assert len(id1) == 64  # double-SHA-256 hex digest


# ---------------------------------------------------------------------------
# Cross-language test vector
# ---------------------------------------------------------------------------


class TestCrossLanguageVector:
    """Verify that the signable bytes and tx ID match the Rust protocol crate.

    Uses the same hardcoded address strings as the Rust test to ensure the
    binary serialization is identical byte-for-byte. The expected hex values
    are pinned in the Rust ``cross_language_test_vector`` test.
    """

    def test_vector(self) -> None:
        # Use hardcoded address strings (same as Rust and TypeScript tests).
        # These are not valid bech32, so we construct the Transaction model
        # directly with model_construct() to bypass NovaId validation. The
        # signable_bytes function only cares about the raw string value.
        from nova_sdk.types import Transaction as TxModel

        tx = TxModel.model_construct(
            version=1,
            tx_type=TransactionType.TRANSFER,
            sender="nova1sender_test_vector",
            receiver="nova1receiver_test_vector",
            amount=Amount(value=1_000_000, currency="NOVA"),
            fee=100,
            nonce=42,
            timestamp=1_700_000_000_000,
            payload=b"",
        )

        canonical = signable_bytes(tx)
        canonical_hex = canonical.hex()
        tx_id = compute_transaction_id(tx)

        # These must match the values pinned in the Rust cross_language_test_vector test.
        assert canonical_hex == (
            "01005472616e73666572006e6f76613173656e6465725f746573745f766563746f72"
            "006e6f76613172656365697665725f746573745f766563746f720040420f00000000"
            "004e4f56410064000000000000002a000000000000000068e5cf8b01000000"
        )

        assert tx_id == "a8c099ee823f352281802881bf6b55008b4a0f8813808426fe83017e20a5d147"

        print(f"\n--- Cross-language test vector (Python) ---")
        print(f"signable_bytes_hex: {canonical_hex}")
        print(f"tx_id: {tx_id}")

    def test_signing_roundtrip_with_deterministic_keypair(self) -> None:
        """Verify sign + verify round-trips with real bech32 addresses."""
        sender_seed = b"\x01" + b"\x00" * 31
        sk, pk = keypair_from_seed(sender_seed)
        sender_addr = create_nova_id(pk)

        receiver_seed = b"\x02" + b"\x00" * 31
        _, rpk = keypair_from_seed(receiver_seed)
        receiver_addr = create_nova_id(rpk)

        tx = (
            TransactionBuilder()
            .version(1)
            .type(TransactionType.TRANSFER)
            .sender(sender_addr)
            .receiver(receiver_addr)
            .amount(1_000_000, "NOVA")
            .fee(100)
            .nonce(42)
            .timestamp(1_700_000_000_000)
            .build()
        )

        signed = sign_transaction(tx, sk)
        assert verify_transaction(signed) is True


# ---------------------------------------------------------------------------
# Wallet-level transfer building
# ---------------------------------------------------------------------------


class TestWalletTransfer:
    """End-to-end: wallet builds and signs a transfer."""

    def test_build_transfer_verifies(self) -> None:
        sender = NovaWallet.create()
        receiver = NovaWallet.create()

        signed = sender.build_transfer(
            to=receiver.address,
            amount=250_000,
            currency="NOVA",
            nonce=0,
            fee=10,
        )
        assert verify_transaction(signed) is True
        assert signed.transaction.sender == sender.address
        assert signed.transaction.receiver == receiver.address
        assert signed.transaction.amount.value == 250_000


# ---------------------------------------------------------------------------
# Pydantic type validation
# ---------------------------------------------------------------------------


class TestTypeValidation:
    """Pydantic v2 model validation edge cases."""

    def test_nova_id_rejects_invalid(self) -> None:
        with pytest.raises(Exception):
            NovaId._validate("btc1invalidaddress")

    def test_public_key_from_hex(self) -> None:
        _, pk = generate_keypair()
        hex_str = pk.hex()
        restored = PublicKey._validate(hex_str)
        assert bytes(restored) == pk

    def test_public_key_rejects_wrong_length(self) -> None:
        with pytest.raises(ValueError, match="32 bytes"):
            PublicKey._validate(b"\x00" * 16)

    def test_signature_rejects_wrong_length(self) -> None:
        with pytest.raises(ValueError, match="64 bytes"):
            Signature._validate(b"\x00" * 32)

    def test_amount_currency_normalised(self) -> None:
        a = Amount(value=100, currency="nova")
        assert a.currency == "NOVA"

    def test_amount_rejects_negative(self) -> None:
        with pytest.raises(Exception):
            Amount(value=-1, currency="NOVA")
