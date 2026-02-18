"""Async HTTP client for the NOVA Protocol node API.

:class:`NovaClient` wraps both the REST and JSON-RPC 2.0 endpoints exposed
by a NOVA validator node.  All I/O is async via :mod:`httpx`, and every
public method returns typed Pydantic models rather than raw dicts.

Example::

    async with NovaClient("http://localhost:9070") as client:
        status = await client.get_status()
        print(status.block_height)
"""

from __future__ import annotations

import json
from typing import Any

import httpx
from pydantic import BaseModel

from nova_sdk.types import Transaction


# ---------------------------------------------------------------------------
# Response models
# ---------------------------------------------------------------------------


class StatusResponse(BaseModel):
    """Payload returned by ``GET /status``."""

    version: str
    network: str
    block_height: int
    peer_count: int
    synced: bool


class BlockResponse(BaseModel):
    """Payload returned by ``GET /blocks/:height``."""

    height: int
    hash: str
    parent_hash: str
    proposer: str
    tx_count: int
    timestamp: int
    state_root: str = ""


class TransactionResponse(BaseModel):
    """Payload returned by ``GET /transactions/:hash``."""

    hash: str
    sender: str
    recipient: str
    amount: int
    fee: int
    block_height: int = 0
    status: str = "Pending"
    timestamp: int = 0


class AccountResponse(BaseModel):
    """Payload returned by ``GET /accounts/:address``."""

    address: str
    balance: int
    nonce: int


class SendTransactionResponse(BaseModel):
    """Payload returned by the ``nova_sendTransaction`` RPC method."""

    tx_hash: str
    status: str


# ---------------------------------------------------------------------------
# Exceptions
# ---------------------------------------------------------------------------


class NovaClientError(Exception):
    """Base exception for all client-level errors."""

    def __init__(self, message: str, status_code: int | None = None) -> None:
        self.message = message
        self.status_code = status_code
        super().__init__(message)


class NovaConnectionError(NovaClientError):
    """Raised when the SDK cannot establish a connection to the node."""


class NovaNotFoundError(NovaClientError):
    """Raised when the requested resource does not exist (HTTP 404)."""


class NovaRpcError(NovaClientError):
    """Raised when the node returns a JSON-RPC error response."""

    def __init__(self, message: str, code: int) -> None:
        self.code = code
        super().__init__(message, status_code=None)


# ---------------------------------------------------------------------------
# Client
# ---------------------------------------------------------------------------


class NovaClient:
    """Async HTTP client for a NOVA Protocol node.

    Supports both the REST endpoints (``/health``, ``/status``, ``/blocks``,
    ``/transactions``, ``/accounts``) and the JSON-RPC 2.0 gateway at
    ``/rpc``.

    Args:
        base_url: Root URL of the NOVA node (e.g. ``"http://localhost:9070"``).
        timeout: Default request timeout in seconds.
        retries: Number of retries on transient failures.

    Example::

        async with NovaClient("http://localhost:9070") as client:
            height = await client.get_block_height()
    """

    def __init__(
        self,
        base_url: str,
        *,
        timeout: float = 30.0,
        retries: int = 3,
    ) -> None:
        self._base_url = base_url.rstrip("/")
        self._timeout = timeout
        self._retries = retries
        self._request_id = 0
        self._client: httpx.AsyncClient | None = None

    # ----- lifecycle -------------------------------------------------------

    async def _ensure_client(self) -> httpx.AsyncClient:
        if self._client is None or self._client.is_closed:
            self._client = httpx.AsyncClient(
                base_url=self._base_url,
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

    async def __aexit__(self, *args: object) -> None:
        await self.close()

    # ----- internal helpers ------------------------------------------------

    def _next_id(self) -> int:
        self._request_id += 1
        return self._request_id

    async def _get(self, path: str) -> httpx.Response:
        """Send a GET request, handling connection errors transparently."""
        client = await self._ensure_client()
        try:
            return await client.get(path)
        except httpx.ConnectError as exc:
            raise NovaConnectionError(
                f"cannot reach {self._base_url}: {exc}",
            ) from exc
        except httpx.TimeoutException as exc:
            raise NovaConnectionError(
                f"request to {self._base_url} timed out",
            ) from exc

    async def _rpc_call(self, method: str, params: list[Any] | None = None) -> Any:
        """Send a JSON-RPC 2.0 request and return the ``result`` field.

        Raises:
            NovaRpcError: If the node returns a JSON-RPC error object.
            NovaConnectionError: If the node is unreachable.
        """
        client = await self._ensure_client()
        payload = {
            "jsonrpc": "2.0",
            "method": method,
            "params": params if params is not None else [],
            "id": self._next_id(),
        }
        try:
            resp = await client.post("/rpc", json=payload)
            resp.raise_for_status()
        except httpx.ConnectError as exc:
            raise NovaConnectionError(
                f"cannot reach {self._base_url}: {exc}",
            ) from exc
        except httpx.TimeoutException as exc:
            raise NovaConnectionError(
                f"request to {self._base_url} timed out",
            ) from exc

        body = resp.json()
        if "error" in body and body["error"] is not None:
            err = body["error"]
            raise NovaRpcError(
                message=err.get("message", "unknown error"),
                code=err.get("code", -1),
            )
        return body.get("result")

    # ----- REST methods ----------------------------------------------------

    async def health(self) -> bool:
        """Check node liveness via ``GET /health``.

        Returns:
            ``True`` if the node responds with 200, ``False`` on any error.
        """
        try:
            resp = await self._get("/health")
            return resp.status_code == 200
        except (NovaClientError, httpx.HTTPError):
            return False

    async def get_status(self) -> StatusResponse:
        """Fetch the node status summary via ``GET /status``."""
        resp = await self._get("/status")
        resp.raise_for_status()
        return StatusResponse.model_validate(resp.json())

    async def get_block(self, height: int) -> BlockResponse:
        """Fetch a block by height via ``GET /blocks/:height``.

        Raises:
            NovaNotFoundError: If no block exists at the given height.
        """
        resp = await self._get(f"/blocks/{height}")
        if resp.status_code == 404:
            raise NovaNotFoundError(
                f"Block not found at height {height}",
                status_code=404,
            )
        resp.raise_for_status()
        return BlockResponse.model_validate(resp.json())

    async def get_transaction(self, hash: str) -> TransactionResponse:
        """Fetch a transaction by hash via ``GET /transactions/:hash``.

        Raises:
            NovaNotFoundError: If no transaction matches the hash.
        """
        resp = await self._get(f"/transactions/{hash}")
        if resp.status_code == 404:
            raise NovaNotFoundError(
                f"Transaction not found: {hash}",
                status_code=404,
            )
        resp.raise_for_status()
        return TransactionResponse.model_validate(resp.json())

    async def get_account(self, address: str) -> AccountResponse:
        """Fetch account state via ``GET /accounts/:address``."""
        resp = await self._get(f"/accounts/{address}")
        resp.raise_for_status()
        return AccountResponse.model_validate(resp.json())

    # ----- JSON-RPC methods ------------------------------------------------

    async def get_block_height(self) -> int:
        """Return the latest confirmed block height (``nova_blockHeight``)."""
        result = await self._rpc_call("nova_blockHeight")
        return int(result)

    async def get_peer_count(self) -> int:
        """Return the number of connected P2P peers (``nova_peerCount``)."""
        result = await self._rpc_call("nova_peerCount")
        return int(result)

    async def get_network_id(self) -> str:
        """Return the network identifier (``nova_networkId``)."""
        result = await self._rpc_call("nova_networkId")
        return str(result)

    async def get_version(self) -> str:
        """Return the node software version (``nova_version``)."""
        result = await self._rpc_call("nova_version")
        return str(result)

    async def get_balance(self, address: str) -> int:
        """Query the native balance for *address* (``nova_getBalance``)."""
        result = await self._rpc_call("nova_getBalance", [address])
        if isinstance(result, dict):
            return int(result["balance"])
        return int(result)

    async def send_transaction(self, tx: Transaction) -> SendTransactionResponse:
        """Broadcast a transaction via ``nova_sendTransaction``.

        Args:
            tx: The :class:`Transaction` to send.

        Returns:
            A :class:`SendTransactionResponse` with the transaction hash and
            initial status.
        """
        tx_data = tx.model_dump(mode="json", by_alias=True)
        result = await self._rpc_call("nova_sendTransaction", [tx_data])
        return SendTransactionResponse.model_validate(result)
