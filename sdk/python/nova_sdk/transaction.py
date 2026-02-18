"""Transaction construction, signing, and verification.

Provides a fluent builder API for constructing NOVA transactions and
utility functions for signing and verifying them. The binary wire format
matches the Rust protocol crate's ``Transaction::signable_bytes()`` so
that transactions built in any SDK can be verified by the Rust validator.
"""

from __future__ import annotations

import hashlib
import struct
import time
from typing import Self

from nova_sdk.identity import sign_message, verify_signature, parse_nova_id
from nova_sdk.types import (
    Amount,
    NovaId,
    PublicKey,
    Signature,
    SignedTransaction,
    Transaction,
    TransactionType,
)

# ---------------------------------------------------------------------------
# Transaction type wire format mapping
# ---------------------------------------------------------------------------

# The Rust ``TransactionType::Display`` uses PascalCase strings in the
# canonical byte serialization, while the Python enum uses snake_case values
# for JSON ergonomics. This mapping bridges the two.
_TX_TYPE_WIRE: dict[TransactionType, str] = {
    TransactionType.TRANSFER: "Transfer",
    TransactionType.CREDIT_REQUEST: "CreditRequest",
    TransactionType.CREDIT_SETTLEMENT: "CreditSettlement",
    TransactionType.TOKEN_MINT: "TokenMint",
    TransactionType.TOKEN_BURN: "TokenBurn",
}


# ---------------------------------------------------------------------------
# Canonical signable bytes — matches Rust Transaction::signable_bytes()
# ---------------------------------------------------------------------------


def signable_bytes(tx: Transaction) -> bytes:
    """Produce the canonical binary representation used for signing and ID
    computation.

    This must match ``Transaction::signable_bytes()`` in the Rust protocol
    crate byte-for-byte.

    Layout::

        version        — 2 bytes, little-endian u16
        tx_type        — UTF-8 PascalCase string + 0x00 separator
        sender         — UTF-8 address string + 0x00 separator
        receiver       — UTF-8 address string + 0x00 separator
        amount.value   — 8 bytes, little-endian u64
        amount.currency— UTF-8 string + 0x00 separator
        fee            — 8 bytes, little-endian u64
        nonce          — 8 bytes, little-endian u64
        timestamp      — 8 bytes, little-endian u64
        payload        — if present: 0x01 + 4-byte LE u32 length + raw bytes
                         if absent:  0x00

    Fields excluded: id, signature, sender_public_key, zkp_proof.
    """
    buf = bytearray()

    # Protocol version (2 bytes, LE u16).
    buf += struct.pack("<H", tx.version)

    # Transaction type discriminant (PascalCase) + null separator.
    buf += _TX_TYPE_WIRE[tx.tx_type].encode("utf-8")
    buf += b"\x00"

    # Sender address + null separator.
    buf += str(tx.sender).encode("utf-8")
    buf += b"\x00"

    # Receiver address + null separator.
    buf += str(tx.receiver).encode("utf-8")
    buf += b"\x00"

    # Amount value (8 bytes, LE u64).
    buf += struct.pack("<Q", tx.amount.value)

    # Amount currency string + null separator.
    buf += tx.amount.currency.encode("utf-8")
    buf += b"\x00"

    # Fee (8 bytes, LE u64).
    buf += struct.pack("<Q", tx.fee)

    # Nonce (8 bytes, LE u64).
    buf += struct.pack("<Q", tx.nonce)

    # Timestamp (8 bytes, LE u64).
    buf += struct.pack("<Q", tx.timestamp)

    # Payload: length-prefixed if present, single 0x00 flag if absent.
    if tx.payload:
        buf += b"\x01"
        buf += struct.pack("<I", len(tx.payload))
        buf += tx.payload
    else:
        buf += b"\x00"

    return bytes(buf)


def compute_transaction_id(tx: Transaction) -> str:
    """Compute the canonical transaction ID: ``hex(double_sha256(signable_bytes))``.

    This matches the Rust ``Transaction::compute_id()`` implementation, which
    applies SHA-256 twice to protect against length-extension attacks.
    """
    data = signable_bytes(tx)
    first = hashlib.sha256(data).digest()
    return hashlib.sha256(first).hexdigest()


def sign_transaction(tx: Transaction, secret_key: bytes) -> SignedTransaction:
    """Sign a transaction and return the wrapped :class:`SignedTransaction`.

    The signing message is the raw canonical signable bytes (matching the Rust
    validator), NOT a hash of the transaction ID.

    Args:
        tx: The unsigned transaction to sign.
        secret_key: The 32-byte Ed25519 signing key of the sender.

    Returns:
        A :class:`SignedTransaction` containing the original transaction,
        the 64-byte signature, and the signer's public key.
    """
    from nacl.signing import SigningKey

    sk = SigningKey(secret_key)
    pub = bytes(sk.verify_key)

    message = signable_bytes(tx)
    sig = sign_message(secret_key, message)

    return SignedTransaction(
        transaction=tx,
        signature=Signature(sig),
        public_key=PublicKey(pub),
    )


def verify_transaction(signed_tx: SignedTransaction) -> bool:
    """Verify the signature on a :class:`SignedTransaction`.

    Checks that:
    1. The Ed25519 signature is valid for the canonical signable bytes.
    2. The public key in the signed transaction matches the sender address.

    Returns:
        ``True`` if both checks pass.
    """
    message = signable_bytes(signed_tx.transaction)

    # Verify the Ed25519 signature itself.
    if not verify_signature(bytes(signed_tx.public_key), message, bytes(signed_tx.signature)):
        return False

    # Verify that the public key corresponds to the claimed sender.
    sender_pk = parse_nova_id(signed_tx.transaction.sender)
    if sender_pk != bytes(signed_tx.public_key):
        return False

    return True


class TransactionBuilder:
    """Fluent builder for constructing :class:`Transaction` instances.

    Example::

        tx = (
            TransactionBuilder()
            .type(TransactionType.TRANSFER)
            .sender("nova1...")
            .receiver("nova1...")
            .amount(1_000_000, "NOVA")
            .fee(1000)
            .nonce(42)
            .build()
        )
    """

    def __init__(self) -> None:
        self._version: int = 1
        self._tx_type: TransactionType | None = None
        self._sender: str | None = None
        self._receiver: str | None = None
        self._amount_value: int | None = None
        self._amount_currency: str | None = None
        self._fee: int = 0
        self._nonce: int | None = None
        self._payload: bytes = b""
        self._timestamp: int | None = None

    def version(self, v: int) -> Self:
        """Set the protocol version (default: 1)."""
        self._version = v
        return self

    def type(self, tx_type: TransactionType) -> Self:
        """Set the transaction type."""
        self._tx_type = tx_type
        return self

    def sender(self, nova_id: str) -> Self:
        """Set the sender NOVA address."""
        self._sender = nova_id
        return self

    def receiver(self, nova_id: str) -> Self:
        """Set the receiver NOVA address."""
        self._receiver = nova_id
        return self

    def amount(self, value: int, currency: str) -> Self:
        """Set the transfer amount and currency."""
        self._amount_value = value
        self._amount_currency = currency
        return self

    def fee(self, value: int) -> Self:
        """Set the transaction fee."""
        self._fee = value
        return self

    def nonce(self, n: int) -> Self:
        """Set the sender's nonce."""
        self._nonce = n
        return self

    def payload(self, data: bytes) -> Self:
        """Attach an opaque payload to the transaction."""
        self._payload = data
        return self

    def timestamp(self, ts: int) -> Self:
        """Set an explicit timestamp (defaults to ``time.time_ns() // 1_000_000`` at build time)."""
        self._timestamp = ts
        return self

    def build(self) -> Transaction:
        """Validate all fields and return the constructed :class:`Transaction`.

        Raises:
            ValueError: If any required field is missing.
        """
        missing: list[str] = []
        if self._tx_type is None:
            missing.append("type")
        if self._sender is None:
            missing.append("sender")
        if self._receiver is None:
            missing.append("receiver")
        if self._amount_value is None or self._amount_currency is None:
            missing.append("amount")
        if self._nonce is None:
            missing.append("nonce")
        if missing:
            raise ValueError(f"missing required fields: {', '.join(missing)}")

        return Transaction(
            version=self._version,
            tx_type=self._tx_type,  # type: ignore[arg-type]
            sender=NovaId(self._sender),  # type: ignore[arg-type]
            receiver=NovaId(self._receiver),  # type: ignore[arg-type]
            amount=Amount(value=self._amount_value, currency=self._amount_currency),  # type: ignore[arg-type]
            fee=self._fee,
            nonce=self._nonce,  # type: ignore[arg-type]
            timestamp=self._timestamp if self._timestamp is not None else int(time.time()),
            payload=self._payload,
        )
