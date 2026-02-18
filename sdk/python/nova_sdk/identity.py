"""NOVA identity primitives: keypair generation, address encoding, signing.

All cryptographic operations use Ed25519 via PyNaCl (libsodium binding).
Addresses are bech32-encoded with the ``nova`` human-readable prefix,
providing built-in checksum validation and a clear visual namespace.
"""

from __future__ import annotations

import hashlib

from nacl.exceptions import BadSignatureError
from nacl.signing import SigningKey, VerifyKey

from nova_sdk.types import bech32_decode, bech32_encode

_HRP = "nova"


def generate_keypair() -> tuple[bytes, bytes]:
    """Generate a random Ed25519 keypair.

    Returns:
        A ``(secret_key, public_key)`` tuple where *secret_key* is 32 bytes
        of seed material and *public_key* is the 32-byte verifying key.
    """
    sk = SigningKey.generate()
    return bytes(sk), bytes(sk.verify_key)


def keypair_from_seed(seed: bytes) -> tuple[bytes, bytes]:
    """Derive an Ed25519 keypair deterministically from a 32-byte seed.

    Args:
        seed: Exactly 32 bytes of seed material.

    Returns:
        A ``(secret_key, public_key)`` tuple.

    Raises:
        ValueError: If *seed* is not exactly 32 bytes.
    """
    if len(seed) != 32:
        raise ValueError(f"seed must be exactly 32 bytes, got {len(seed)}")
    sk = SigningKey(seed)
    return bytes(sk), bytes(sk.verify_key)


def create_nova_id(public_key: bytes) -> str:
    """Encode a 32-byte Ed25519 public key as a bech32 NOVA address.

    Args:
        public_key: 32-byte Ed25519 verifying key.

    Returns:
        A bech32 string with the ``nova`` HRP (e.g. ``nova1qw508d6...``).

    Raises:
        ValueError: If *public_key* is not 32 bytes.
    """
    if len(public_key) != 32:
        raise ValueError(f"public key must be 32 bytes, got {len(public_key)}")
    return bech32_encode(_HRP, public_key)


def parse_nova_id(address: str) -> bytes:
    """Decode a bech32 NOVA address back to the 32-byte public key.

    Args:
        address: A bech32 string starting with ``nova1``.

    Returns:
        The 32-byte public key embedded in the address.

    Raises:
        ValueError: If the address is malformed or has the wrong HRP.
    """
    hrp, data = bech32_decode(address)
    if hrp != _HRP:
        raise ValueError(f"expected HRP '{_HRP}', got '{hrp}'")
    if len(data) != 32:
        raise ValueError(f"decoded key must be 32 bytes, got {len(data)}")
    return data


def sign_message(secret_key: bytes, message: bytes) -> bytes:
    """Sign *message* with an Ed25519 secret key.

    Args:
        secret_key: The 32-byte signing key (seed).
        message: Arbitrary bytes to sign.

    Returns:
        The 64-byte Ed25519 signature.
    """
    sk = SigningKey(secret_key)
    signed = sk.sign(message)
    return signed.signature


def verify_signature(public_key: bytes, message: bytes, signature: bytes) -> bool:
    """Verify an Ed25519 signature.

    Args:
        public_key: 32-byte verifying key.
        message: The original message bytes.
        signature: The 64-byte signature to verify.

    Returns:
        ``True`` if the signature is valid, ``False`` otherwise.
    """
    try:
        vk = VerifyKey(public_key)
        vk.verify(message, signature)
        return True
    except (BadSignatureError, Exception):
        return False
