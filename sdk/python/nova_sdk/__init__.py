"""NOVA Protocol Python SDK.

Provides everything needed to interact with the NOVA network from Python:
identity management, transaction building, wallet operations, and an async
client for the node HTTP and JSON-RPC API.

Quick start::

    from nova_sdk import NovaWallet, NovaClient

    wallet = NovaWallet.create()
    print(wallet.address)

    async with NovaClient("http://localhost:9070") as client:
        height = await client.get_block_height()
"""

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
from nova_sdk.identity import (
    create_nova_id,
    generate_keypair,
    keypair_from_seed,
    parse_nova_id,
    sign_message,
    verify_signature,
)
from nova_sdk.transaction import (
    TransactionBuilder,
    compute_transaction_id,
    sign_transaction,
    verify_transaction,
)
from nova_sdk.types import (
    AccountState,
    Amount,
    Block,
    BlockHeader,
    CreditOffer,
    CreditScore,
    NovaId,
    PublicKey,
    Signature,
    SignedTransaction,
    Transaction,
    TransactionReceipt,
    TransactionStatus,
    TransactionType,
    ValidatorInfo,
    WalletState,
)
from nova_sdk.wallet import NovaWallet

__all__ = [
    # Client
    "NovaClient",
    "NovaClientError",
    "NovaConnectionError",
    "NovaNotFoundError",
    "NovaRpcError",
    # Client response models
    "AccountResponse",
    "BlockResponse",
    "SendTransactionResponse",
    "StatusResponse",
    "TransactionResponse",
    # Identity
    "create_nova_id",
    "generate_keypair",
    "keypair_from_seed",
    "parse_nova_id",
    "sign_message",
    "verify_signature",
    # Transaction
    "TransactionBuilder",
    "compute_transaction_id",
    "sign_transaction",
    "verify_transaction",
    # Types
    "AccountState",
    "Amount",
    "Block",
    "BlockHeader",
    "CreditOffer",
    "CreditScore",
    "NovaId",
    "PublicKey",
    "Signature",
    "SignedTransaction",
    "Transaction",
    "TransactionReceipt",
    "TransactionStatus",
    "TransactionType",
    "ValidatorInfo",
    "WalletState",
    # Wallet
    "NovaWallet",
]

__version__ = "0.1.0"
