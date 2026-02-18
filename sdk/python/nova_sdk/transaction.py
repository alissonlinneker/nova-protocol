"""Transaction construction, signing, and verification.

Provides a fluent builder API for constructing NOVA transactions and
utility functions for signing and verifying them. All serialisation
uses canonical JSON (sorted keys, no whitespace) to ensure deterministic
hashing.
"""

from __future__ import annotations

import hashlib
import json
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


def _canonical_bytes(tx: Transaction) -> bytes:
    """Serialise a transaction to a deterministic byte representation.

    The canonical form is UTF-8 encoded JSON with keys sorted
    alphabetically and no extraneous whitespace.
    """
    data = tx.model_dump(mode="json", by_alias=True)
    return json.dumps(data, sort_keys=True, separators=(",", ":")).encode("utf-8")


def compute_transaction_id(tx: Transaction) -> str:
    """Compute the SHA-256 transaction ID for *tx*.

    The ID is the hex-encoded SHA-256 digest of the canonical JSON
    representation. This is the value used as the on-chain transaction
    hash.
    """
    return hashlib.sha256(_canonical_bytes(tx)).hexdigest()


def sign_transaction(tx: Transaction, secret_key: bytes) -> SignedTransaction:
    """Sign a transaction and return the wrapped :class:`SignedTransaction`.

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

    message = _canonical_bytes(tx)
    sig = sign_message(secret_key, message)

    return SignedTransaction(
        transaction=tx,
        signature=Signature(sig),
        public_key=PublicKey(pub),
    )


def verify_transaction(signed_tx: SignedTransaction) -> bool:
    """Verify the signature on a :class:`SignedTransaction`.

    Checks that:
    1. The signature is valid for the embedded transaction.
    2. The public key in the signed transaction matches the sender address.

    Returns:
        ``True`` if both checks pass.
    """
    message = _canonical_bytes(signed_tx.transaction)

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
        self._tx_type: TransactionType | None = None
        self._sender: str | None = None
        self._receiver: str | None = None
        self._amount_value: int | None = None
        self._amount_currency: str | None = None
        self._fee: int = 0
        self._nonce: int | None = None
        self._payload: bytes = b""
        self._timestamp: int | None = None

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
        """Set an explicit timestamp (defaults to ``time.time()`` at build time)."""
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
            tx_type=self._tx_type,  # type: ignore[arg-type]
            sender=NovaId(self._sender),  # type: ignore[arg-type]
            receiver=NovaId(self._receiver),  # type: ignore[arg-type]
            amount=Amount(value=self._amount_value, currency=self._amount_currency),  # type: ignore[arg-type]
            fee=self._fee,
            nonce=self._nonce,  # type: ignore[arg-type]
            timestamp=self._timestamp if self._timestamp is not None else int(time.time()),
            payload=self._payload,
        )
