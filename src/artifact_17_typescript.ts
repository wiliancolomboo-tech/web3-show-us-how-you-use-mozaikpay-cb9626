import { Resend } from 'resend';
import { Config } from '../config';

export interface InvoiceReceiptPayload {
  recipientEmail: string;
  customerPubkey: string;
  invoiceIdHex: string;
  amountUsdc: number;
  signature: string;
  timestamp: string;
}

export class EmailNotificationService {
  private resend: Resend;

  constructor() {
    this.resend = new Resend(Config.RESEND_API_KEY);
  }

  async sendInvoiceReceipt(payload: InvoiceReceiptPayload): Promise<boolean> {
    try {
      if (Config.APP_ENV === 'test') {
        console.log(`[TEST-MODE] Mock email sent to ${payload.recipientEmail} for invoice ${payload.invoiceIdHex}`);
        return true;
      }

      const response = await this.resend.emails.send({
        from: 'MozaikPay Settlements <billing@mozaikengine.io>',
        to: payload.recipientEmail,
        subject: `Payment Confirmed: Invoice #${payload.invoiceIdHex.substring(0, 8)}`,
        html: `
          <div style="font-family: sans-serif; max-width: 600px; margin: auto; padding: 24px; border: 1px solid #e2e8f0; border-radius: 8px;">
            <h2 style="color: #0f172a;">MozaikPay Instant Settlement Receipt</h2>
            <p>Your payment on Solana was verified and settled successfully.</p>
            <table style="width: 100%; text-align: left; border-collapse: collapse; margin-top: 16px;">
              <tr style="border-bottom: 1px solid #e2e8f0; padding: 8px 0;"><th style="padding: 8px;">Invoice ID</th><td style="padding: 8px; font-family: monospace;">${payload.invoiceIdHex}</td></tr>
              <tr style="border-bottom: 1px solid #e2e8f0; padding: 8px 0;"><th style="padding: 8px;">Customer</th><td style="padding: 8px; font-family: monospace;">${payload.customerPubkey}</td></tr>
              <tr style="border-bottom: 1px solid #e2e8f0; padding: 8px 0;"><th style="padding: 8px;">Amount Paid</th><td style="padding: 8px;"><strong>$${payload.amountUsdc.toFixed(2)} USDC</strong></td></tr>
              <tr style="border-bottom: 1px solid #e2e8f0; padding: 8px 0;"><th style="padding: 8px;">Solana Tx Signature</th><td style="padding: 8px; font-family: monospace; word-break: break-all;">${payload.signature}</td></tr>
              <tr style="border-bottom: 1px solid #e2e8f0; padding: 8px 0;"><th style="padding: 8px;">Time</th><td style="padding: 8px;">${payload.timestamp}</td></tr>
            </table>
            <div style="margin-top: 24px; font-size: 12px; color: #64748b;">
              Settled via MozaikEngine on Solana. Sub-second finality. Zero chargebacks.
            </div>
          </div>
        `
      });

      return !!response.data;
    } catch (err) {
      console.error('[EmailService] Failed to send receipt:', err);
      return false;
    }
  }
}