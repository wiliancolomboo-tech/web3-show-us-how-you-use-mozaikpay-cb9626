import { Connection, PublicKey } from "@solana/web3.js";
import { Program, AnchorProvider, Idl } from "@coral-xyz/anchor";
import { Resend } from "resend";
import axios from "axios";
import * as dotenv from "dotenv";

dotenv.config();

// Configuration
const SOLANA_RPC_URL = process.env.SOLANA_RPC_URL || "https://api.devnet.solana.com";
const PROGRAM_ID = new PublicKey(process.env.PROGRAM_ID || "MoZaiKPAYNexus11111111111111111111111111111");
const RESEND_API_KEY = process.env.RESEND_API_KEY || "re_dummy_sample_key_placeholder";
const SLACK_WEBHOOK_URL = process.env.SLACK_WEBHOOK_URL || "";
const MERCHANT_NOTIFICATION_EMAIL = process.env.MERCHANT_NOTIFICATION_EMAIL || "billing@enterprise.com";

const resend = new Resend(RESEND_API_KEY);
const processedTxCache = new Set<string>();

interface PaymentSettledPayload {
  merchant: string;
  orderId: string;
  payer: string;
  totalAmount: number;
  merchantAmount: number;
  affiliateAmount: number;
  protocolAmount: number;
  timestamp: number;
  signature: string;
}

export class MozaikPayDaemon {
  private connection: Connection;

  constructor() {
    this.connection = new Connection(SOLANA_RPC_URL, {
      commitment: "confirmed",
      wsEndpoint: SOLANA_RPC_URL.replace("https://", "wss://").replace("http://", "ws://"),
    });
  }

  public async start(): Promise<void> {
    console.log("=====================================================");
    console.log("[MozaikPay Nexus] Enterprise Daemon Starting...");
    console.log(`[Target Network]: ${SOLANA_RPC_URL}`);
    console.log(`[Nexus Program ID]: ${PROGRAM_ID.toBase58()}`);
    console.log("=====================================================");

    this.connection.onLogs(
      PROGRAM_ID,
      async (logs, ctx) => {
        if (logs.err) return;
        const signature = logs.signature;

        if (processedTxCache.has(signature)) {
          return; // Strict Idempotency
        }
        processedTxCache.add(signature);

        try {
          await this.processTransactionLogs(signature, logs.logs);
        } catch (err) {
          console.error(`[Process Error] Signature: ${signature}`, err);
        }
      },
      "confirmed"
    );

    console.log("[Status] Listening for settlement events...");
  }

  private async processTransactionLogs(signature: string, logs: string[]): Promise<void> {
    for (const line of logs) {
      if (line.includes("PaymentSettledEvent")) {
        console.log(`[Event Detected] Payment Settlement: ${signature}`);
        
        // Mock payload extraction representative of deserialized Anchor event
        const payload: PaymentSettledPayload = {
          merchant: "MerchantHQ_10x",
          orderId: `ORD-${Date.now().toString().slice(-6)}`,
          payer: "9xQeWvG816bUx9EPjHmaT23yvVM2ZWbrrpZb9PusVFin",
          totalAmount: 250.0, // $250.00 USDC
          merchantAmount: 212.5,
          affiliateAmount: 25.0,
          protocolAmount: 12.5,
          timestamp: Math.floor(Date.now() / 1000),
          signature,
        };

        await Promise.all([
          this.sendCustomerInvoiceEmail(payload),
          this.postSlackOpsAlert(payload),
        ]);
      }
    }
  }

  public async sendCustomerInvoiceEmail(data: PaymentSettledPayload): Promise<void> {
    try {
      const emailHtml = `
        <div style="font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; max-width: 600px; margin: 0 auto; padding: 24px; border: 1px solid #eaeaea; border-radius: 12px; background: #ffffff;">
          <h2 style="color: #111; margin-top: 0;">Payment Receipt Confirmation</h2>
          <p style="color: #555;">Thank you for your payment. Your Solana settlement has been verified and settled via <strong>MozaikPay Nexus</strong>.</p>
          
          <table style="width: 100%; border-collapse: collapse; margin: 20px 0;">
            <tr style="border-bottom: 1px solid #f0f0f0;">
              <td style="padding: 8px 0; color: #777;">Order ID</td>
              <td style="padding: 8px 0; font-weight: bold; text-align: right;">${data.orderId}</td>
            </tr>
            <tr style="border-bottom: 1px solid #f0f0f0;">
              <td style="padding: 8px 0; color: #777;">Amount Settled</td>
              <td style="padding: 8px 0; font-weight: bold; text-align: right;">$${data.totalAmount.toFixed(2)} USDC</td>
            </tr>
            <tr style="border-bottom: 1px solid #f0f0f0;">
              <td style="padding: 8px 0; color: #777;">Payer Account</td>
              <td style="padding: 8px 0; font-family: monospace; text-align: right;">${data.payer.slice(0, 6)}...${data.payer.slice(-4)}</td>
            </tr>
            <tr style="border-bottom: 1px solid #f0f0f0;">
              <td style="padding: 8px 0; color: #777;">Solana Tx Hash</td>
              <td style="padding: 8px 0; font-family: monospace; text-align: right;"><a href="https://solscan.io/tx/${data.signature}?cluster=devnet" target="_blank">${data.signature.slice(0, 8)}...</a></td>
            </tr>
          </table>

          <div style="background: #f7fafc; padding: 12px; border-radius: 6px; font-size: 13px; color: #4a5568;">
            <strong>Automated Settlement Split:</strong><br/>
            Merchant: $${data.merchantAmount.toFixed(2)} | Affiliate: $${data.affiliateAmount.toFixed(2)} | Protocol Reserve: $${data.protocolAmount.toFixed(2)}
          </div>
          
          <p style="font-size: 11px; color: #999; margin-top: 24px;">Powered by MozaikPay Nexus Protocol • Instant Zero-Loss Finality</p>
        </div>
      `;

      if (process.env.NODE_ENV === "production" && RESEND_API_KEY.startsWith("re_")) {
        await resend.emails.send({
          from: "MozaikPay Nexus <invoicing@mozaikpay-nexus.io>",
          to: [MERCHANT_NOTIFICATION_EMAIL],
          subject: `[Receipt] MozaikPay Payment Settled: ${data.orderId}`,
          html: emailHtml,
        });
        console.log(`[Resend Invoicing] Dispatched invoice for order ${data.orderId}`);
      } else {
        console.log(`[Resend Mock Mode] Verified invoice payload for order ${data.orderId}`);
      }
    } catch (err) {
      console.error("[Resend Error] Failed to send email invoice:", err);
    }
  }

  public async postSlackOpsAlert(data: PaymentSettledPayload): Promise<void> {
    if (!SLACK_WEBHOOK_URL) {
      console.log(`[Slack Mock] Alert: Settled $${data.totalAmount} for ${data.orderId}`);
      return;
    }

    try {
      await axios.post(SLACK_WEBHOOK_URL, {
        blocks: [
          {
            type: "header",
            text: {
              type: "plain_text",
              text: "⚡ MozaikPay Nexus: Settlement Confirmed",
              emoji: true,
            },
          },
          {
            type: "section",
            fields: [
              { type: "mrkdwn", text: `*Order ID:*\n\`${data.orderId}\`` },
              { type: "mrkdwn", text: `*Total Settled:*\n$${data.totalAmount.toFixed(2)} USDC` },
              { type: "mrkdwn", text: `*Merchant Net:*\n$${data.merchantAmount.toFixed(2)}` },
              { type: "mrkdwn", text: `*Affiliate Split:*\n$${data.affiliateAmount.toFixed(2)}` },
            ],
          },
          {
            type: "context",
            elements: [
              {
                type: "mrkdwn",
                text: `Tx Signature: <https://solscan.io/tx/${data.signature}?cluster=devnet|${data.signature.slice(0, 12)}...>`,
              },
            ],
          },
        ],
      });
      console.log(`[Slack Ops] Posted notification for ${data.orderId}`);
    } catch (err) {
      console.error("[Slack Error] Failed to post Slack ops notification:", err);
    }
  }
}

if (require.main === module) {
  const daemon = new MozaikPayDaemon();
  daemon.start().catch(console.error);
}