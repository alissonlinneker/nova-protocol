"""Tests for nova_sdk.identity — keypair generation, address encoding, signing."""

from __future__ import annotations

import os

from nova_sdk.identity import (
    create_nova_id,
    generate_keypair,
    keypair_from_seed,
    parse_nova_id,
    sign_message,
    verify_signature,
)


class TestKeypairGeneration:
    """Ed25519 keypair generation via PyNaCl."""

    def test_generate_returns_correct_lengths(self) -> None:
        sk, pk = generate_keypair()
        assert len(sk) == 32, "secret key must be 32 bytes"
        assert len(pk) == 32, "public key must be 32 bytes"

    def test_generate_produces_unique_keys(self) -> None:
        _, pk1 = generate_keypair()
        _, pk2 = generate_keypair()
        assert pk1 != pk2, "two random keypairs must differ"

    def test_keypair_types_are_bytes(self) -> None:
        sk, pk = generate_keypair()
        assert isinstance(sk, bytes)
        assert isinstance(pk, bytes)


class TestDeterministicKeypair:
    """Deterministic derivation from a 32-byte seed."""

    def test_same_seed_same_keypair(self) -> None:
        seed = b"\x01" * 32
        sk1, pk1 = keypair_from_seed(seed)
        sk2, pk2 = keypair_from_seed(seed)
        assert sk1 == sk2
        assert pk1 == pk2

    def test_different_seed_different_keypair(self) -> None:
        _, pk1 = keypair_from_seed(b"\x01" * 32)
        _, pk2 = keypair_from_seed(b"\x02" * 32)
        assert pk1 != pk2

    def test_invalid_seed_length_raises(self) -> None:
        import pytest

        with pytest.raises(ValueError, match="32 bytes"):
            keypair_from_seed(b"\x00" * 16)


class TestNovaIdRoundtrip:
    """Bech32 address encoding and decoding."""

    def test_roundtrip(self) -> None:
        _, pk = generate_keypair()
        address = create_nova_id(pk)
        recovered = parse_nova_id(address)
        assert recovered == pk, "roundtrip must recover the original public key"

    def test_address_starts_with_nova1(self) -> None:
        _, pk = generate_keypair()
        address = create_nova_id(pk)
        assert address.startswith("nova1")

    def test_address_is_lowercase(self) -> None:
        _, pk = generate_keypair()
        address = create_nova_id(pk)
        assert address == address.lower()

    def test_deterministic_address(self) -> None:
        seed = b"\xab" * 32
        _, pk = keypair_from_seed(seed)
        addr1 = create_nova_id(pk)
        addr2 = create_nova_id(pk)
        assert addr1 == addr2

    def test_invalid_hrp_raises(self) -> None:
        import pytest

        # Encode with wrong prefix to get a valid bech32 string with wrong HRP.
        from nova_sdk.types import bech32_encode

        bad = bech32_encode("btc", b"\x00" * 32)
        with pytest.raises(ValueError, match="HRP"):
            parse_nova_id(bad)

    def test_invalid_key_length_raises(self) -> None:
        import pytest

        with pytest.raises(ValueError, match="32 bytes"):
            create_nova_id(b"\x00" * 16)


class TestSigningAndVerification:
    """Ed25519 sign / verify via PyNaCl."""

    def test_sign_verify_roundtrip(self) -> None:
        sk, pk = generate_keypair()
        message = b"transfer 100 NOVA to alice"
        sig = sign_message(sk, message)
        assert len(sig) == 64
        assert verify_signature(pk, message, sig) is True

    def test_wrong_message_fails(self) -> None:
        sk, pk = generate_keypair()
        sig = sign_message(sk, b"correct message")
        assert verify_signature(pk, b"wrong message", sig) is False

    def test_wrong_key_fails(self) -> None:
        sk1, _ = generate_keypair()
        _, pk2 = generate_keypair()
        sig = sign_message(sk1, b"hello")
        assert verify_signature(pk2, b"hello", sig) is False

    def test_deterministic_signatures(self) -> None:
        seed = b"\xcc" * 32
        sk, _ = keypair_from_seed(seed)
        msg = b"deterministic"
        sig1 = sign_message(sk, msg)
        sig2 = sign_message(sk, msg)
        assert sig1 == sig2, "Ed25519 signatures are deterministic"

    def test_empty_message(self) -> None:
        sk, pk = generate_keypair()
        sig = sign_message(sk, b"")
        assert verify_signature(pk, b"", sig) is True

    def test_large_message(self) -> None:
        sk, pk = generate_keypair()
        msg = os.urandom(10_000)
        sig = sign_message(sk, msg)
        assert verify_signature(pk, msg, sig) is True
