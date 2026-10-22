import { Connection, PublicKey, Keypair, clusterApiUrl } from "@solana/web3.js";
import { Program, AnchorProvider, Wallet, Idl, BN } from "@coral-xyz/anchor";
import { getAssociatedTokenAddressSync } from "@solana/spl-token";
import axios from "axios";
import dotenv from "dotenv";

dotenv.config();

// Environment Configuration Validation
interface AppConfig {
  solanaRpcUrl: string;
  solanaWsUrl: string;
  programId: string;
  resendApiKey: string;
  slackWebhookUrl: string;
  merchantNotificationEmail: string;
  payerKeypairSecret: string;
}

const config: AppConfig = {
  solanaRpcUrl: process.env.SOLANA_RPC_URL || clusterApiUrl("devnet"),
  solanaWsUrl: process.env.SOLANA_WS_URL || "wss://api.devnet.solana.com",
  programId: process.env.PROGRAM_ID || "MzkPay1111111111111111111111111111111111111",
  resendApiKey: process.env.RESEND_API_KEY || "re_test_mock_key_12345",
  slackWebhookUrl: process.env.SLACK_WEBHOOK_URL || "https://hooks.slack.com/services/MOCK/TOKEN",
  merchantNotificationEmail: process.env.MERCHANT_EMAIL || "treasury@acmecorp.global",
  payerKeypairSecret: process.env.PAYER_KEYPAIR_JSON || "[]",
};

// MozaikPay Dynamic Payment Link Generator
export class MozaikPayAdapter {
  private baseCheckoutUrl = "https://checkout.mozaikpay.com/pay";

  public generateCheckoutSession(params: {
    escrowId: string;
    amountUSDC: number;
    merchantPubkey: string;
    callbackUrl: string;
  }): { checkoutUrl: string; paymentReference: string } {
    const paymentReference = `mzk_ref_${Date.now()}_${Math.random().toString(36).substring(7)}`;
    const query = new URLSearchParams({
      ref: paymentReference,
      escrow_id: params.escrowId,
      amount: params.amountUSDC.toFixed(2),
      currency: "USDC",
      merchant: params.merchantPubkey,
      return_url: params.callbackUrl,
    });

    return {
      checkoutUrl: `${this.baseCheckoutUrl}?${query.toString()}`,
      paymentReference,
    };
  }
}

// Enterprise Notification Service
export class EnterpriseNotifier {
  private resendKey: string;
  private slackWebhook: string;

  constructor(resendKey: string, slackWebhook: string) {
    this.resendKey = resendKey;
    this.slackWebhook = slackWebhook;
  }

  public async notifySlack(title: string, fields: Record<string, string>): Promise<void> {
    const blocks = [
      {
        type: "header",
        text: {
          type: "plain_text",
          text: `⚡ MozaikPay Settlement: ${title}`,
          emoji: true,
        },
      },
      {
        type: "section",
        fields: Object.entries(fields).map(([k, v]) => ({
          type: "mrkdwn",
          text: `*${k}:*\n\`${v}\``,
        })),
      },
      {
        type: "context",
        elements: [
          {
            type: "mrkdwn",
            text: `*Network:* Solana Devnet/Mainnet | *Engine:* MozaikFlow Core`,
          },
        ],
      },
    ];

    try {
      if (this.slackWebhook.includes("hooks.slack.com")) {
        await axios.post(this.slackWebhook, { blocks });
        console.log(`[Slack] Successfully posted alert: ${title}`);
      } else {
        console.log(`[Slack Mock] Alert generated for: ${title}`);
      }
    } catch (err: any) {
      console.error(`[Slack Error] Failed to send message: ${err.message}`);
    }
  }

  public async sendEmailInvoice(
    recipient: string,
    escrowId: string,
    amount: string,
    txSignature: string
  ): Promise<void> {
    const emailPayload = {
      from: "billing@mozaikflow.io",
      to: [recipient],
      subject: `[MozaikPay Receipt] Escrow Settlement: ${escrowId}`,
      html: `
        <div style="font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; padding: 24px; color: #1e293b;">
          <h2 style="color: #4f46e5;">MozaikPay Settlement Confirmation</h2>
          <p>An on-chain settlement has been confirmed via the MozaikFlow Engine.</p>
          <table style="width: 100%; border-collapse: collapse; margin-top: 16px;">
            <tr style="border-bottom: 1px solid #e2e8f0;"><td style="padding: 8px 0; font-weight: bold;">Escrow ID</td><td>${escrowId}</td></tr>
            <tr style="border-bottom: 1px solid #e2e8f0;"><td style="padding: 8px 0; font-weight: bold;">Settled Amount</td><td>${amount} USDC</td></tr>
            <tr style="border-bottom: 1px solid #e2e8f0;"><td style="padding: 8px 0; font-weight: bold;">Transaction Signature</td><td><code>${txSignature}</code></td></tr>
          </table>
          <p style="margin-top: 24px;">
            <a href="https://solscan.io/tx/${txSignature}?cluster=devnet" style="background-color: #4f46e5; color: white; padding: 10px 18px; border-radius: 6px; text-decoration: none; font-weight: bold;">View on Explorer</a>
          </p>
        </div>
      `,
    };

    try {
      if (this.resendKey.startsWith("re_") && !this.resendKey.includes("mock")) {
        await axios.post("https://api.resend.com/emails", emailPayload, {
          headers: { Authorization: `Bearer ${this.resendKey}` },
        });
        console.log(`[Resend] Payment receipt dispatched to ${recipient}`);
      } else {
        console.log(`[Resend Mock] Email rendered for ${recipient} (Tx: ${txSignature})`);
      }
    } catch (err: any) {
      console.error(`[Resend Error] Failed to send email: ${err.message}`);
    }
  }
}

// Background Agent Orchestrator
export class MozaikFlowOrchestrator {
  private connection: Connection;
  private notifier: EnterpriseNotifier;
  private programId: PublicKey;

  constructor() {
    this.connection = new Connection(config.solanaRpcUrl, {
      wsEndpoint: config.solanaWsUrl,
      commitment: "confirmed",
    });
    this.notifier = new EnterpriseNotifier(config.resendApiKey, config.slackWebhookUrl);
    this.programId = new PublicKey(config.programId);
  }

  public async startDaemon(): Promise<void> {
    console.log("=====================================================");
    console.log("  🚀 MOZAIKFLOW ENTERPRISE AGENT IS LIVE");
    console.log(`  RPC: ${config.solanaRpcUrl}`);
    console.log(`  Program ID: ${this.programId.toBase58()}`);
    console.log("=====================================================");

    this.connection.onLogs(
      this.programId,
      async (logs, ctx) => {
        if (logs.err) return;

        for (const log of logs.logs) {
          if (log.includes("EscrowFundedEvent")) {
            await this.handleEscrowFunded(logs.signature);
          } else if (log.includes("MilestoneReleasedEvent")) {
            await this.handleMilestoneReleased(logs.signature);
          }
        }
      },
      "confirmed"
    );
  }

  private async handleEscrowFunded(signature: string): Promise<void> {
    console.log(`\n[EVENT DETECTED] EscrowFunded in Tx: ${signature}`);
    await this.notifier.notifySlack("Escrow Vault Funded", {
      Transaction: signature,
      Status: "Locked in On-Chain PDA",
      Gateway: "MozaikPay Smart Router",
    });

    await this.notifier.sendEmailInvoice(
      config.merchantNotificationEmail,
      "ESCROW-DEMO-01",
      "5,000.00",
      signature
    );
  }

  private async handleMilestoneReleased(signature: string): Promise<void> {
    console.log(`\n[EVENT DETECTED] MilestoneReleased in Tx: ${signature}`);
    await this.notifier.notifySlack("Milestone Tranche Released", {
      Transaction: signature,
      Payout: "Settled to Merchant Wallet",
    });
  }
}

// CLI Execution Entrypoint
if (require.main === module) {
  const orchestrator = new MozaikFlowOrchestrator();
  orchestrator.startDaemon().catch((err) => {
    console.error("Fatal Daemon Failure:", err);
    process.exit(1);
  });
}