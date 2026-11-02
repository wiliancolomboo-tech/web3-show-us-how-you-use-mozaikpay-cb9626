import { Connection, PublicKey, ConfirmedSignatureInfo } from '@solana/web3.js';
import { Config } from '../config';
import { EmailNotificationService } from './emailService';
import { SlackNotificationService } from './slackService';

export interface RegisteredInvoice {
  invoiceIdHex: string;
  referencePubkey: string;
  expectedAmount: number;
  recipientEmail: string;
  status: 'PENDING' | 'SETTLED' | 'EXPIRED';
}

export class SolanaSettlementListener {
  private connection: Connection;
  private emailService: EmailNotificationService;
  private slackService: SlackNotificationService;
  private pendingInvoices: Map<string, RegisteredInvoice> = new Map();

  constructor() {
    this.connection = new Connection(Config.SOLANA_RPC_URL, {
      wsEndpoint: Config.SOLANA_WS_URL,
      commitment: 'confirmed'
    });
    this.emailService = new EmailNotificationService();
    this.slackService = new SlackNotificationService();
  }

  public registerPendingInvoice(invoice: RegisteredInvoice): void {
    this.pendingInvoices.set(invoice.referencePubkey, invoice);
    console.log(`[Listener] Registered pending invoice ${invoice.invoiceIdHex} referencing ${invoice.referencePubkey}`);
  }

  public async pollReferenceAccount(referencePubkeyStr: string): Promise<boolean> {
    const invoice = this.pendingInvoices.get(referencePubkeyStr);
    if (!invoice || invoice.status === 'SETTLED') return false;

    const refPubkey = new PublicKey(referencePubkeyStr);
    const signatures: ConfirmedSignatureInfo[] = await this.connection.getSignaturesForAddress(
      refPubkey,
      { limit: 1 },
      'confirmed'
    );

    if (signatures.length === 0) {
      return false;
    }

    const latestTx = signatures[0];
    if (latestTx.err) {
      console.warn(`[Listener] Tx failed for reference ${referencePubkeyStr}`);
      return false;
    }

    invoice.status = 'SETTLED';
    console.log(`[Listener] Payment confirmed! Tx: ${latestTx.signature}`);

    // Trigger enterprise side effects
    await this.emailService.sendInvoiceReceipt({
      recipientEmail: invoice.recipientEmail,
      customerPubkey: 'Confirmed-On-Solana',
      invoiceIdHex: invoice.invoiceIdHex,
      amountUsdc: invoice.expectedAmount,
      signature: latestTx.signature,
      timestamp: new Date().toISOString()
    });

    const merchantCut = invoice.expectedAmount * 0.85;
    const affiliateCut = invoice.expectedAmount * 0.10;

    await this.slackService.notifyPaymentSettled(
      invoice.invoiceIdHex,
      invoice.expectedAmount,
      merchantCut,
      affiliateCut,
      latestTx.signature
    );

    return true;
  }
}