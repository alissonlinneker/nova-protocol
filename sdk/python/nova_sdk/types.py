"""Core types for the NOVA Protocol SDK.

All public-facing data structures are defined here as Pydantic v2 models.
Wire format follows the NOVA specification: amounts are unsigned integers
in the smallest currency unit, keys and signatures are hex-encoded on the
wire, and addresses use bech32 encoding with the ``nova`` human-readable
prefix.
"""

from __future__ import annotations

import re
import time
from enum import Enum
from typing import Annotated, Any

from pydantic import (
    BaseModel,
    ConfigDict,
    Field,
    GetCoreSchemaHandler,
    ValidationInfo,
    field_serializer,
    field_validator,
    model_validator,
)
from pydantic_core import CoreSchema, core_schema


# ---------------------------------------------------------------------------
# Bech32 helpers (minimal, self-contained)
# ---------------------------------------------------------------------------

_BECH32_CHARSET = "qpzry9x8gf2tvdw0s3jn54khce6mua7l"
_BECH32_HRP = "nova"


def _bech32_polymod(values: list[int]) -> int:
    gen = [0x3B6A57B2, 0x26508E6D, 0x1EA119FA, 0x3D4233DD, 0x2A1462B3]
    chk = 1
    for v in values:
        top = chk >> 25
        chk = ((chk & 0x1FFFFFF) << 5) ^ v
        for i in range(5):
            chk ^= gen[i] if ((top >> i) & 1) else 0
    return chk


def _bech32_hrp_expand(hrp: str) -> list[int]:
    return [ord(c) >> 5 for c in hrp] + [0] + [ord(c) & 31 for c in hrp]


def _bech32_create_checksum(hrp: str, data: list[int]) -> list[int]:
    values = _bech32_hrp_expand(hrp) + data
    polymod = _bech32_polymod(values + [0, 0, 0, 0, 0, 0]) ^ 1
    return [(polymod >> 5 * (5 - i)) & 31 for i in range(6)]


def _bech32_verify_checksum(hrp: str, data: list[int]) -> bool:
    return _bech32_polymod(_bech32_hrp_expand(hrp) + data) == 1


def _convertbits(data: bytes | list[int], frombits: int, tobits: int, pad: bool = True) -> list[int]:
    acc = 0
    bits = 0
    ret: list[int] = []
    maxv = (1 << tobits) - 1
    for value in data:
        if value < 0 or (value >> frombits):
            raise ValueError(f"invalid value for convertbits: {value}")
        acc = (acc << frombits) | value
        bits += frombits
        while bits >= tobits:
            bits -= tobits
            ret.append((acc >> bits) & maxv)
    if pad:
        if bits:
            ret.append((acc << (tobits - bits)) & maxv)
    elif bits >= frombits or ((acc << (tobits - bits)) & maxv):
        raise ValueError("non-zero padding bits")
    return ret


def bech32_encode(hrp: str, data: bytes) -> str:
    """Encode *data* (arbitrary bytes) into a bech32 string with the given HRP."""
    values = _convertbits(data, 8, 5)
    checksum = _bech32_create_checksum(hrp, values)
    combined = values + checksum
    return hrp + "1" + "".join(_BECH32_CHARSET[d] for d in combined)


def bech32_decode(bech: str) -> tuple[str, bytes]:
    """Decode a bech32 string, returning ``(hrp, data_bytes)``."""
    if bech != bech.lower() and bech != bech.upper():
        raise ValueError("mixed-case bech32 string")
    bech = bech.lower()
    pos = bech.rfind("1")
    if pos < 1 or pos + 7 > len(bech):
        raise ValueError("invalid bech32 separator position")
    hrp = bech[:pos]
    data_part = [_BECH32_CHARSET.index(c) for c in bech[pos + 1 :]]
    if not _bech32_verify_checksum(hrp, data_part):
        raise ValueError("invalid bech32 checksum")
    decoded = _convertbits(data_part[:-6], 5, 8, pad=False)
    return hrp, bytes(decoded)


# ---------------------------------------------------------------------------
# Annotated scalar types
# ---------------------------------------------------------------------------

_NOVA_ID_RE = re.compile(r"^nova1[qpzry9x8gf2tvdw0s3jn54khce6mua7l]{38,}$")


class NovaId(str):
    """A bech32-encoded NOVA address with ``nova`` HRP.

    Subclasses ``str`` so it serialises natively as a JSON string while
    still enforcing format on creation.
    """

    @classmethod
    def __get_pydantic_core_schema__(
        cls, _source_type: Any, handler: GetCoreSchemaHandler
    ) -> CoreSchema:
        return core_schema.no_info_plain_validator_function(cls._validate)

    @classmethod
    def _validate(cls, v: str) -> "NovaId":
        if not isinstance(v, str):
            raise ValueError("NovaId must be a string")
        v = v.lower()
        if not v.startswith("nova1"):
            raise ValueError("NovaId must start with 'nova1'")
        try:
            hrp, _ = bech32_decode(v)
        except Exception as exc:
            raise ValueError(f"invalid bech32 encoding: {exc}") from exc
        if hrp != _BECH32_HRP:
            raise ValueError(f"expected HRP '{_BECH32_HRP}', got '{hrp}'")
        return cls(v)

    def to_public_key_bytes(self) -> bytes:
        """Extract the raw 32-byte public key from the address."""
        _, data = bech32_decode(self)
        return data


class PublicKey(bytes):
    """32-byte Ed25519 public key with hex serialisation."""

    @classmethod
    def __get_pydantic_core_schema__(
        cls, _source_type: Any, handler: GetCoreSchemaHandler
    ) -> CoreSchema:
        return core_schema.no_info_plain_validator_function(cls._validate)

    @classmethod
    def _validate(cls, v: bytes | str) -> "PublicKey":
        if isinstance(v, str):
            v = bytes.fromhex(v)
        if len(v) != 32:
            raise ValueError(f"public key must be 32 bytes, got {len(v)}")
        return cls(v)

    def hex(self) -> str:  # noqa: A003  — shadows builtin deliberately
        return super().hex()


class Signature(bytes):
    """64-byte Ed25519 signature with hex serialisation."""

    @classmethod
    def __get_pydantic_core_schema__(
        cls, _source_type: Any, handler: GetCoreSchemaHandler
    ) -> CoreSchema:
        return core_schema.no_info_plain_validator_function(cls._validate)

    @classmethod
    def _validate(cls, v: bytes | str) -> "Signature":
        if isinstance(v, str):
            v = bytes.fromhex(v)
        if len(v) != 64:
            raise ValueError(f"signature must be 64 bytes, got {len(v)}")
        return cls(v)

    def hex(self) -> str:  # noqa: A003
        return super().hex()


# ---------------------------------------------------------------------------
# Enums
# ---------------------------------------------------------------------------


class TransactionType(str, Enum):
    """Supported transaction types in the NOVA protocol."""

    TRANSFER = "transfer"
    CREDIT_REQUEST = "credit_request"
    CREDIT_SETTLEMENT = "credit_settlement"
    TOKEN_MINT = "token_mint"
    TOKEN_BURN = "token_burn"


class TransactionStatus(str, Enum):
    """Lifecycle status of a transaction."""

    PENDING = "pending"
    CONFIRMED = "confirmed"
    FAILED = "failed"
    EXPIRED = "expired"


# ---------------------------------------------------------------------------
# Core models
# ---------------------------------------------------------------------------


class Amount(BaseModel):
    """Monetary amount in the smallest indivisible unit of a currency."""

    model_config = ConfigDict(frozen=True)

    value: Annotated[int, Field(ge=0, description="Amount in the smallest currency unit")]
    currency: Annotated[str, Field(min_length=1, max_length=12, description="Currency ticker")]

    @field_validator("currency")
    @classmethod
    def _normalise_currency(cls, v: str) -> str:
        return v.upper()


class Transaction(BaseModel):
    """An unsigned NOVA transaction."""

    model_config = ConfigDict(populate_by_name=True)

    version: Annotated[int, Field(ge=1, default=1, description="Protocol version")]
    tx_type: TransactionType = Field(alias="type")
    sender: NovaId
    receiver: NovaId
    amount: Amount
    fee: Annotated[int, Field(ge=0, default=0)]
    nonce: Annotated[int, Field(ge=0)]
    timestamp: Annotated[int, Field(default_factory=lambda: int(time.time()))]
    payload: bytes = b""

    @field_serializer("payload")
    @classmethod
    def _serialize_payload(cls, v: bytes, _info: Any) -> str:
        return v.hex()

    @field_validator("payload", mode="before")
    @classmethod
    def _parse_payload(cls, v: Any) -> bytes:
        if isinstance(v, str):
            return bytes.fromhex(v)
        if isinstance(v, bytes):
            return v
        raise ValueError("payload must be bytes or hex string")


class SignedTransaction(BaseModel):
    """A transaction with its Ed25519 signature and signer public key."""

    model_config = ConfigDict(populate_by_name=True)

    transaction: Transaction
    signature: Signature
    public_key: PublicKey

    @field_serializer("signature")
    @classmethod
    def _serialize_signature(cls, v: Signature, _info: Any) -> str:
        return v.hex()

    @field_serializer("public_key")
    @classmethod
    def _serialize_public_key(cls, v: PublicKey, _info: Any) -> str:
        return v.hex()


class TransactionReceipt(BaseModel):
    """Confirmation receipt returned after a transaction is included in a block."""

    tx_hash: str
    block_height: int
    block_hash: str
    status: TransactionStatus
    timestamp: int
    gas_used: Annotated[int, Field(ge=0, default=0)]


class BlockHeader(BaseModel):
    """Header of a NOVA block."""

    height: Annotated[int, Field(ge=0)]
    hash: str
    previous_hash: str
    timestamp: int
    validator: NovaId
    merkle_root: str
    tx_count: Annotated[int, Field(ge=0)]


class Block(BaseModel):
    """A full NOVA block including header and transactions."""

    header: BlockHeader
    transactions: list[SignedTransaction] = Field(default_factory=list)


class AccountState(BaseModel):
    """On-chain state for a single account."""

    address: NovaId
    nonce: Annotated[int, Field(ge=0)]
    balances: dict[str, int] = Field(default_factory=dict)


class CreditOffer(BaseModel):
    """A credit line offered to a borrower."""

    offer_id: str
    lender: NovaId
    borrower: NovaId
    currency: str
    limit: Annotated[int, Field(ge=0)]
    interest_bps: Annotated[int, Field(ge=0, description="Annual interest in basis points")]
    expires_at: int


class CreditScore(BaseModel):
    """Protocol-level credit score for an identity."""

    address: NovaId
    score: Annotated[int, Field(ge=0, le=1000)]
    last_updated: int
    total_credit_lines: Annotated[int, Field(ge=0)] = 0
    total_borrowed: Annotated[int, Field(ge=0)] = 0
    total_repaid: Annotated[int, Field(ge=0)] = 0


class WalletState(BaseModel):
    """Aggregate wallet information returned by the node."""

    address: NovaId
    public_key: PublicKey
    nonce: Annotated[int, Field(ge=0)]
    balances: dict[str, int] = Field(default_factory=dict)
    pending_tx_count: Annotated[int, Field(ge=0)] = 0

    @field_serializer("public_key")
    @classmethod
    def _serialize_pk(cls, v: PublicKey, _info: Any) -> str:
        return v.hex()


class ValidatorInfo(BaseModel):
    """Public information about a network validator."""

    address: NovaId
    public_key: PublicKey
    stake: Annotated[int, Field(ge=0)]
    commission_bps: Annotated[int, Field(ge=0)]
    is_active: bool
    last_block_signed: Annotated[int, Field(ge=0)] = 0

    @field_serializer("public_key")
    @classmethod
    def _serialize_pk(cls, v: PublicKey, _info: Any) -> str:
        return v.hex()
