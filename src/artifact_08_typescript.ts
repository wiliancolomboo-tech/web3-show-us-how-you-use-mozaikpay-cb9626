import { Connection, PublicKey, Keypair } from '@solana/web3.js';
import { Program, AnchorProvider, Wallet } from '@coral-xyz/anchor';
import * as dotenv from 'dotenv';
import { ResendAdapter } from './adapters/resendAdapter';
import { SlackAdapter } from './adapters/slackAdapter';

dotenv.config();

const RPC_URL = process.env.SOLANA_RPC_URL || 'https://api.devnet.solana.com';
const WS_URL = process.env.SOLANA_WS_URL || 'wss://api.devnet.solana.com';
const PROGRAM_ID = new PublicKey(process.env.PROGRAM_ID || 'MoZ1kPayEnterpriseEngine1111111111111111111');
const RESEND_API_KEY = process.env.RESEND_API_KEY || 'mock-resend-key';
const SLACK_WEBHOOK_URL = process.env.SLACK_WEBHOOK_URL || 'mock-slack-url';

// Idempotency cache (TTL: 1 Hour)
const processedTxCache = new Set<string>();

export async function runDaemon() {
  console.log('====================================================');
  console.log('  MOZAIKPULSE ENTERPRISE ORCHESTRATION DAEMON');
  console.log(`  RPC: ${RPC_URL}`);
  console.log(`  Program: ${PROGRAM_ID.toBase58()}`);
  console.log('====================================================');

  const connection = new Connection(RPC_URL, {
    wsEndpoint: WS_URL,
    commitment: 'confirmed',
  });

  const dummyWallet = new Wallet(Keypair.generate());
  const provider = new AnchorProvider(connection, dummyWallet, { commitment: 'confirmed' });

  // Minimal IDL for parsing the InvoiceSettledEvent
  const idl: any = {
    version: "0.1.0",
    name: "mozaik_enterprise_pay",
    events: [
      {
        name: "InvoiceSettledEvent",
        fields: [
          { name: "invoicePda", type: "publicKey", index: false },
          { name: "invoiceId", type: "string", index: false },
          { name: "payer", type: "publicKey", index: false },
          { name: "totalAmount", type: "u64", index: false },
          { name: "merchantAmount", type: "u64", index: false },
          { name: "taxAmount", type: "u64", index: false },
          { name: "platformAmount", type: "u64", index: false },
          { name: "settledAt", type: "i64", index: false }
        ]
      }
    ]
  };

  const program = new Program(idl, PROGRAM_ID, provider);
  const resend = new ResendAdapter(RESEND_API_KEY);
  const slack = new SlackAdapter(SLACK_WEBHOOK_URL);

  console.log('[Daemon] Listening for on-chain InvoiceSettledEvent...');

  program.addEventListener('InvoiceSettledEvent', async (event: any, slot: number, signature: string) => {
    if (processedTxCache.has(signature)) {
      console.log(`[Daemon] Signature ${signature} already handled. Skipping.`);
      return;
    }
    processedTxCache.add(signature);

    console.log(`\n Invoice Settled Detected!`);
    console.log(`   Tx: ${signature}`);
    console.log(`   Invoice ID: ${event.invoiceId}`);
    console.log(`   Total Amount: ${event.totalAmount.toString()} (Base units)`);
    console.log(`   Payer: ${event.payer.toBase58()}`);

    // Enterprise Slack Dispatch
    await slack.sendInvoiceSettledAlert({
      invoiceId: event.invoiceId,
      totalAmount: event.totalAmount.toNumber(),
      merchantShare: event.merchantAmount.toNumber(),
      taxShare: event.taxAmount.toNumber(),
      platformShare: event.platformAmount.toNumber(),
      txSignature: signature,
      payerPubkey: event.payer.toBase58(),
    });

    // Enterprise Customer Email Receipt
    await resend.sendSettlementReceipt({
      to: 'customer-finance@enterprise.com',
      invoiceId: event.invoiceId,
      txSignature: signature,
      totalAmount: event.totalAmount.toNumber(),
      settledAt: new Date(event.settledAt.toNumber() * 1000).toISOString(),
      payerPubkey: event.payer.toBase58(),
    });
  });
}

if (require.main === module) {
  runDaemon().catch((err) => {
    console.error('Fatal Daemon Crash:', err);
    process.exit(1);
  });
}