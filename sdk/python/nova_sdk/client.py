"""Async client for the NOVA Protocol JSON-RPC API.

:class:`NovaClient` provides typed methods for every public RPC endpoint
exposed by a NOVA node. All I/O uses :mod:`httpx` so the client is fully
async and compatible with ``asyncio``.
"""

from __future__ import annotations

import asyncio
import time
from typing import Any

import httpx

from nova_sdk.types import (
    AccountState,
    Block,
    SignedTransaction,
    Transaction,
    TransactionReceipt,
    TransactionStatus,
    ValidatorInfo,
)


# ---------------------------------------------------------------------------
# Exceptions
# ---------------------------------------------------------------------------


class NovaRPCError(Exception):
    """Raised when the NOVA node returns a JSON-RPC error response."""

    def __init__(self, code: int, message: str, data: Any = None) -> None:
        self.code = code
        self.message = message
        self.data = data
        super().__init__(f"RPC error {code}: {message}")


class NovaConnectionError(Exception):
    """Raised when the SDK cannot reach the NOVA node."""


class NovaTimeoutError(Exception):
    """Raised when an operation exceeds its deadline."""


# ---------------------------------------------------------------------------
# Client
# ---------------------------------------------------------------------------


class NovaClient:
    """Async JSON-RPC client for a NOVA node.

    Args:
        node_url: Base URL of the NOVA node (e.g. ``"http://localhost:9070"``).
        timeout: Default request timeout in seconds.

    Example::

        async with NovaClient("http://localhost:9070") as client:
            height = await client.get_block_height()
    """

    def __init__(self, node_url: str, *, timeout: float = 15.0) -> None:
        self._node_url = node_url.rstrip("/")
        self._timeout = timeout
        self._request_id = 0
        self._client: httpx.AsyncClient | None = None

    # ----- lifecycle -------------------------------------------------------

    async def _ensure_client(self) -> httpx.AsyncClient:
        if self._client is None or self._client.is_closed:
            self._client = httpx.AsyncClient(
                base_url=self._node_url,
                timeout=self._timeout,
            )
        return self._client

    async def close(self) -> None:
        """Close the underlying HTTP connection pool."""
        if self._client is not None and not self._client.is_closed:
            await self._client.aclose()

    async def __aenter__(self) -> "NovaClient":
        await self._ensure_client()
        return self

    async def __aexit__(self, *exc: object) -> None:
        await self.close()

    # ----- internal helpers ------------------------------------------------

    def _next_id(self) -> int:
        self._request_id += 1
        return self._request_id

    async def _call(self, method: str, params: dict[str, Any] | None = None) -> Any:
        """Send a JSON-RPC 2.0 request and return the ``result`` field."""
        client = await self._ensure_client()
        payload = {
            "jsonrpc": "2.0",
            "method": method,
            "params": params or {},
            "id": self._next_id(),
        }
        try:
            resp = await client.post("/rpc", json=payload)
            resp.raise_for_status()
        except httpx.ConnectError as exc:
            raise NovaConnectionError(f"cannot reach {self._node_url}: {exc}") from exc
        except httpx.TimeoutException as exc:
            raise NovaTimeoutError(f"request to {self._node_url} timed out") from exc

        body = resp.json()
        if "error" in body and body["error"] is not None:
            err = body["error"]
            raise NovaRPCError(
                code=err.get("code", -1),
                message=err.get("message", "unknown error"),
                data=err.get("data"),
            )
        return body.get("result")

    # ----- public API ------------------------------------------------------

    async def get_block_height(self) -> int:
        """Return the latest confirmed block height."""
        result = await self._call("nova_blockHeight")
        return int(result["height"])

    async def get_block(self, height: int) -> Block:
        """Fetch a full block by height."""
        result = await self._call("nova_getBlock", {"height": height})
        return Block.model_validate(result)

    async def get_transaction(self, tx_hash: str) -> Transaction:
        """Fetch a transaction by its hash."""
        result = await self._call("nova_getTransaction", {"hash": tx_hash})
        return Transaction.model_validate(result)

    async def get_account_state(self, address: str) -> AccountState:
        """Fetch nonce and balances for an account."""
        result = await self._call("nova_getAccountState", {"address": address})
        return AccountState.model_validate(result)

    async def send_transaction(self, signed_tx: SignedTransaction) -> str:
        """Broadcast a signed transaction and return its hash.

        Args:
            signed_tx: The signed transaction to broadcast.

        Returns:
            The hex-encoded transaction hash.
        """
        result = await self._call(
            "nova_sendTransaction",
            signed_tx.model_dump(mode="json", by_alias=True),
        )
        return str(result["tx_hash"])

    async def get_balance(self, address: str, token_id: str | None = None) -> int:
        """Query the balance of an address.

        Args:
            address: A bech32-encoded NOVA address.
            token_id: Optional token identifier; ``None`` for native NOVA.

        Returns:
            The balance in the smallest currency unit.
        """
        params: dict[str, Any] = {"address": address}
        if token_id is not None:
            params["token_id"] = token_id
        result = await self._call("nova_getBalance", params)
        return int(result["balance"])

    async def get_validators(self) -> list[ValidatorInfo]:
        """Return the current validator set."""
        result = await self._call("nova_getValidators")
        return [ValidatorInfo.model_validate(v) for v in result["validators"]]

    async def estimate_fee(self, tx: Transaction) -> int:
        """Estimate the network fee for a transaction.

        Args:
            tx: The unsigned transaction to estimate.

        Returns:
            Estimated fee in the smallest native token unit.
        """
        result = await self._call(
            "nova_estimateFee",
            tx.model_dump(mode="json", by_alias=True),
        )
        return int(result["fee"])

    async def wait_for_confirmation(
        self,
        tx_hash: str,
        *,
        timeout: float = 30.0,
        poll_interval: float = 1.0,
    ) -> TransactionReceipt:
        """Poll the node until a transaction is confirmed or the timeout expires.

        Args:
            tx_hash: Transaction hash to watch.
            timeout: Maximum seconds to wait.
            poll_interval: Seconds between poll attempts.

        Returns:
            The :class:`TransactionReceipt` once the transaction reaches a
            terminal status.

        Raises:
            NovaTimeoutError: If *timeout* is exceeded.
        """
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            try:
                result = await self._call("nova_getTransactionReceipt", {"hash": tx_hash})
            except NovaRPCError:
                # Transaction may not be indexed yet; keep polling.
                await asyncio.sleep(poll_interval)
                continue

            if result is None:
                await asyncio.sleep(poll_interval)
                continue

            receipt = TransactionReceipt.model_validate(result)
            if receipt.status in (
                TransactionStatus.CONFIRMED,
                TransactionStatus.FAILED,
                TransactionStatus.EXPIRED,
            ):
                return receipt

            await asyncio.sleep(poll_interval)

        raise NovaTimeoutError(
            f"transaction {tx_hash} not confirmed within {timeout}s"
        )
