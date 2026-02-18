"""High-level wallet abstraction for NOVA Protocol.

:class:`NovaWallet` bundles keypair management, address derivation, and
transaction signing behind a clean interface. It is the recommended entry
point for applications that need to hold keys and broadcast transactions.
"""

from __future__ import annotations

from typing import TYPE_CHECKING

import httpx

from nova_sdk.identity import (
    create_nova_id,
    generate_keypair,
    keypair_from_seed,
    sign_message,
)
from nova_sdk.transaction import (
    TransactionBuilder,
    sign_transaction,
)
from nova_sdk.types import (
    NovaId,
    PublicKey,
    SignedTransaction,
    Transaction,
    TransactionType,
)


class NovaWallet:
    """In-memory NOVA wallet holding a single Ed25519 keypair.

    Wallets are created via the :meth:`create` or :meth:`from_seed` class
    methods. The secret key is held in memory and never serialised
    automatically — callers must manage persistence and encryption.
    """

    __slots__ = ("_secret_key", "_public_key", "_address")

    def __init__(self, secret_key: bytes, public_key: bytes) -> None:
        self._secret_key = secret_key
        self._public_key = public_key
        self._address = create_nova_id(public_key)

    # ----- constructors ----------------------------------------------------

    @classmethod
    def create(cls) -> "NovaWallet":
        """Generate a new wallet with a random keypair."""
        sk, pk = generate_keypair()
        return cls(sk, pk)

    @classmethod
    def from_seed(cls, seed: bytes) -> "NovaWallet":
        """Derive a wallet deterministically from a 32-byte seed."""
        sk, pk = keypair_from_seed(seed)
        return cls(sk, pk)

    # ----- properties ------------------------------------------------------

    @property
    def address(self) -> str:
        """The bech32-encoded NOVA address."""
        return self._address

    @property
    def public_key(self) -> bytes:
        """The raw 32-byte Ed25519 public key."""
        return self._public_key

    # ----- signing ---------------------------------------------------------

    def sign(self, message: bytes) -> bytes:
        """Sign arbitrary bytes with this wallet's secret key.

        Returns:
            The 64-byte Ed25519 signature.
        """
        return sign_message(self._secret_key, message)

    def build_transfer(
        self,
        to: str,
        amount: int,
        currency: str,
        *,
        nonce: int = 0,
        fee: int = 0,
    ) -> SignedTransaction:
        """Build and sign a transfer transaction.

        Args:
            to: Receiver's NOVA address.
            amount: Value in the smallest currency unit.
            currency: Currency ticker (e.g. ``"NOVA"``).
            nonce: Sender nonce. Defaults to 0; callers should fetch the
                current nonce from the network.
            fee: Transaction fee. Defaults to 0.

        Returns:
            A fully signed :class:`SignedTransaction`.
        """
        tx = (
            TransactionBuilder()
            .type(TransactionType.TRANSFER)
            .sender(self._address)
            .receiver(to)
            .amount(amount, currency)
            .fee(fee)
            .nonce(nonce)
            .build()
        )
        return sign_transaction(tx, self._secret_key)

    # ----- network queries -------------------------------------------------

    async def get_balance(self, node_url: str, token_id: str | None = None) -> int:
        """Query this wallet's balance from a NOVA node.

        Args:
            node_url: Base URL of the NOVA JSON-RPC node.
            token_id: Optional token identifier. When ``None`` the native
                NOVA balance is returned.

        Returns:
            The balance in the smallest currency unit.
        """
        params: dict = {"address": self._address}
        if token_id is not None:
            params["token_id"] = token_id

        async with httpx.AsyncClient(base_url=node_url, timeout=10.0) as client:
            resp = await client.post(
                "/rpc",
                json={"jsonrpc": "2.0", "method": "nova_getBalance", "params": params, "id": 1},
            )
            resp.raise_for_status()
            data = resp.json()

        if "error" in data:
            raise RuntimeError(f"RPC error: {data['error']}")
        return int(data["result"]["balance"])

    async def get_transaction_history(self, node_url: str) -> list[Transaction]:
        """Fetch recent transactions involving this wallet.

        Args:
            node_url: Base URL of the NOVA JSON-RPC node.

        Returns:
            A list of :class:`Transaction` objects, most recent first.
        """
        async with httpx.AsyncClient(base_url=node_url, timeout=10.0) as client:
            resp = await client.post(
                "/rpc",
                json={
                    "jsonrpc": "2.0",
                    "method": "nova_getTransactionHistory",
                    "params": {"address": self._address},
                    "id": 1,
                },
            )
            resp.raise_for_status()
            data = resp.json()

        if "error" in data:
            raise RuntimeError(f"RPC error: {data['error']}")
        return [Transaction.model_validate(tx) for tx in data["result"]["transactions"]]
