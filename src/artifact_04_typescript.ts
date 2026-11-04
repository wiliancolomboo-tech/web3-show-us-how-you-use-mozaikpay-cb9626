import { Connection, PublicKey, Keypair } from "@solana/web3.js";
import axios from "axios";
import * as dotenv from "dotenv";

dotenv.config();

export interface SplitPaymentEventData {
  orderIdHex: string;
  payer: string;
  merchantConfig: string;
  grossAmount: number;
  merchantAmount: number;
  platformFee: number;
  taxAmount: number;
  timestamp: number;
  signature: string;
}

export class MozaikFlowEnterpriseOrchestrator {
  private connection: Connection;
  private resendApiKey: string;
  private slackWebhookUrl: string;
  private processedSignatures: Set<string>;

  constructor() {
    const rpcUrl = process.env.SOLANA_RPC_URL || "https://api.devnet.solana.com";
    this.connection = new Connection(rpcUrl, "confirmed");
    this.resendApiKey = process.env.RESEND_API_KEY || "";
    this.slackWebhookUrl = process.env.SLACK_WEBHOOK_URL || "";
    this.processedSignatures = new Set<string>();
  }

  /**
   * Dispatches branded customer receipts via Resend API
   */
  public async sendCustomerReceipt(
    recipientEmail: string,
    event: SplitPaymentEventData
  ): Promise<boolean> {
    if (!this.resendApiKey) {
      console.warn("[WARN] RESEND_API_KEY not configured. Skipping receipt email.");
      return false;
    }

    const payload = {
      from: "MozaikFlow Billing <billing@mozaikflow.io>",
      to: [recipientEmail],
      subject: `Payment Confirmed: Order #${event.orderIdHex.slice(0, 10)}`,
      html: `
        <div style="font-family: Arial, sans-serif; max-width: 600px; margin: 0 auto; padding: 20px; border: 1px solid #e2e8f0; border-radius: 8px;">
          <h2 style="color: #0f172a;">MozaikPay Settlement Receipt</h2>
          <p>Thank you for your purchase. Your transaction was atomically settled on Solana.</p>
          <hr style="border: none; border-top: 1px solid #e2e8f0;" />
          <table style="width: 100%; border-collapse: collapse;">
            <tr><td><strong>Order ID:</strong></td><td><code>${event.orderIdHex}</code></td></tr>
            <tr><td><strong>Gross Paid:</strong></td><td>$${(event.grossAmount / 1e6).toFixed(2)} USDC</td></tr>
            <tr><td><strong>Payer:</strong></td><td><code>${event.payer}</code></td></tr>
            <tr><td><strong>Settled At:</strong></td><td>${new Date(event.timestamp * 1000).toUTCString()}</td></tr>
            <tr><td><strong>Solana Signature:</strong></td><td><a href="https://solscan.io/tx/${event.signature}?cluster=devnet" target="_blank">View on Solscan</a></td></tr>
          </table>
          <p style="font-size: 12px; color: #64748b; margin-top: 20px;">Powered by MozaikPay Protocol.</p>
        </div>
      `,
    };

    try {
      const response = await axios.post("https://api.resend.com/emails", payload, {
        headers: {
          Authorization: `Bearer ${this.resendApiKey}`,
          "Content-Type": "application/json",
        },
      });
      console.log(`[INFO] Resend Receipt Sent successfully. Email ID: ${response.data.id}`);
      return true;
    } catch (err: any) {
      console.error("[ERROR] Failed to send email via Resend:", err.response?.data || err.message);
      return false;
    }
  }

  /**
   * Dispatches operational financial alerts to Slack / Discord
   */
  public async sendSlackAlert(event: SplitPaymentEventData): Promise<boolean> {
    if (!this.slackWebhookUrl) {
      console.warn("[WARN] SLACK_WEBHOOK_URL not configured. Skipping alert.");
      return false;
    }

    const payload = {
      text: `🚀 *MozaikPay Payment Settled* - Order \`${event.orderIdHex.slice(0, 8)}\``,
      blocks: [
        {
          type: "header",
          text: {
            type: "plain_text",
            text: "💰 MozaikPay Settlement Executed",
          },
        },
        {
          type: "section",
          fields: [
            { type: "mrkdwn", text: `*Gross Volume:*\n$${(event.grossAmount / 1e6).toFixed(2)}` },
            { type: "mrkdwn", text: `*Merchant Net:*\n$${(event.merchantAmount / 1e6).toFixed(2)}` },
            { type: "mrkdwn", text: `*Platform Fee:*\n$${(event.platformFee / 1e6).toFixed(2)}` },
            { type: "mrkdwn", text: `*Tax Reserve:*\n$${(event.taxAmount / 1e6).toFixed(2)}` },
          ],
        },
        {
          type: "section",
          text: {
            type: "mrkdwn",
            text: `*Signature:* <https://solscan.io/tx/${event.signature}?cluster=devnet|${event.signature.slice(0, 16)}...>`,
          },
        },
      ],
    };

    try {
      await axios.post(this.slackWebhookUrl, payload);
      console.log("[INFO] Slack notification dispatched successfully.");
      return true;
    } catch (err: any) {
      console.error("[ERROR] Failed to send Slack alert:", err.response?.data || err.message);
      return false;
    }
  }

  /**
   * Idempotently process incoming payment confirmation events
   */
  public async handlePaymentSettled(
    event: SplitPaymentEventData,
    customerEmail?: string
  ): Promise<void> {
    if (this.processedSignatures.has(event.signature)) {
      console.log(`[SKIP] Duplicate signature encountered: ${event.signature}`);
      return;
    }

    this.processedSignatures.add(event.signature);
    console.log(`[PROCESS] Ingesting Payment for Order: ${event.orderIdHex}`);

    await Promise.all([
      this.sendSlackAlert(event),
      customerEmail ? this.sendCustomerReceipt(customerEmail, event) : Promise.resolve(false),
    ]);
  }
}

// -----------------------------------------------------------------------------
// Interactive CLI Driver
// -----------------------------------------------------------------------------
async function runCli() {
  const orchestrator = new MozaikFlowEnterpriseOrchestrator();

  console.log("=== MozaikFlow Enterprise Worker Initialized ===");
  const mockEvent: SplitPaymentEventData = {
    orderIdHex: "0x4a9df36b9012cdfae51249b0129bcdef124890abcdef1234567890abcdef1234",
    payer: Keypair.generate().publicKey.toBase58(),
    merchantConfig: Keypair.generate().publicKey.toBase58(),
    grossAmount: 100_000_000, // 100.00 USDC
    merchantAmount: 85_000_000, // 85.00 USDC (85%)
    platformFee: 10_000_000,    // 10.00 USDC (10%)
    taxAmount: 5_000_000,       // 5.00 USDC (5%)
    timestamp: Math.floor(Date.now() / 1000),
    signature: "5VERv8NMvQbJepNz9Tr6aV7yNn51U5oY6z3p4v38sK4uWbV2Yp8vJbL7pB5C5nK4uWbV2Yp8vJbL7pB5C5nK",
  };

  await orchestrator.handlePaymentSettled(mockEvent, "buyer-receipt@example.com");
}

if (require.main === module) {
  runCli().catch(console.error);
}