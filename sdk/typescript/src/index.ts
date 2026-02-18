/**
 * @nova-protocol/sdk — TypeScript SDK for the NOVA open payment protocol.
 *
 * @packageDocumentation
 */

// Re-export everything so consumers can import from the package root:
//
//   import { NovaClient, NovaWallet, TransactionBuilder } from '@nova-protocol/sdk';

export * from './types.js';
export * from './utils.js';
export * from './identity.js';
export * from './transaction.js';
export * from './wallet.js';
export * from './credit.js';
export { NovaClient } from './nova.js';
