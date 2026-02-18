"""Tests for client, wallet, and transaction construction.

These tests exercise the public API surface without hitting a real NOVA
node. Network-dependent methods are covered by integration tests (not
included here).
"""

from __future__ import annotations

import pytest

from nova_sdk.client import NovaClient
from nova_sdk.identity import create_nova_id, generate_keypair
from nova_sdk.transaction import (
    TransactionBuilder,
    compute_transaction_id,
    sign_transaction,
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
# NovaClient construction
# ---------------------------------------------------------------------------


class TestClientConstruction:
    """Verify NovaClient initialises correctly without I/O."""

    def test_basic_url(self) -> None:
        client = NovaClient("http://localhost:9070")
        assert client._node_url == "http://localhost:9070"

    def test_trailing_slash_stripped(self) -> None:
        client = NovaClient("http://localhost:9070/")
        assert client._node_url == "http://localhost:9070"

    def test_custom_timeout(self) -> None:
        client = NovaClient("http://localhost:9070", timeout=5.0)
        assert client._timeout == 5.0

    def test_request_id_increments(self) -> None:
        client = NovaClient("http://localhost:9070")
        id1 = client._next_id()
        id2 = client._next_id()
        assert id2 == id1 + 1


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
        assert len(id1) == 64  # SHA-256 hex digest


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
