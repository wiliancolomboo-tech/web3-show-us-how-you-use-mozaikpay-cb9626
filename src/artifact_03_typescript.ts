// src/orchestrator.ts
import { Connection, Keypair, PublicKey, clusterApiUrl } from "@solana/web3.js";
import { Program, AnchorProvider, Wallet, Idl, BN } from "@coral-xyz/anchor";
import express, { Request, Response } from "express";
import nacl from "tweetnacl";
import axios from "axios";
import { Resend } from "resend";
import dotenv from "dotenv";

dotenv.config();

// Environment bindings
const PORT = process.env.PORT || 4000;
const SOLANA_RPC = process.env.SOLANA_RPC_URL || clusterApiUrl("devnet");
const RESEND_API_KEY = process.env.RESEND_API_KEY || "re_dummy_sample_key_placeholder";
const SLACK_WEBHOOK_URL = process.env.SLACK_WEBHOOK_URL || "https://api.example.com/webhooks/alerts/placeholder";
const PROGRAM_ID = new PublicKey(process.env.PROGRAM_ID || "MoZaiKPAY1111111111111111111111111111111111");

const resend = new Resend(RESEND_API_KEY);
const app = express();
app.use(express.json());

// In-memory idempotency cache (Redis swap-ready)
const processedSignatures = new Set<string>();

export interface MozaikWebhookPayload {
  invoiceId: string;
  signature: string;
  payer: string;
  amount: number;
  mint: string;
  timestamp: number;
  referenceKey: string;
}

/**
 * Validates cryptographic signature authenticity from MozaikPay Engine
 */
export function verifyMozaikSignature(
  payload: MozaikWebhookPayload,
  rawSignatureHex: string,
  merchantPublicKeyHex: string
): boolean {
  try {
    const message = Buffer.from(
      JSON.stringify({
        invoiceId: payload.invoiceId,
        signature: payload.signature,
        payer: payload.payer,
        amount: payload.amount,
        referenceKey: payload.referenceKey,
      })
    );
    const signature = Buffer.from(rawSignatureHex, "hex");
    const pubKey = Buffer.from(merchantPublicKeyHex, "hex");

    return nacl.sign.detached.verify(message, signature, pubKey);
  } catch (err) {
    console.error("Cryptographic signature verification failed:", err);
    return false;
  }
}

/**
 * Enterprise Slack Audit Dispatcher
 */
export async function notifySlackTreasury(payload: MozaikWebhookPayload): Promise<void> {
  const slackBody = {
    blocks: [
      {
        type: "header",
        text: {
          type: "plain_text",
          text: "🚀 MozaikPay Invoice Settled",
          emoji: true,
        },
      },
      {
        type: "section",
        fields: [
          { type: "mrkdwn", text: `*Invoice ID:*\n\`${payload.invoiceId}\`` },
          { type: "mrkdwn", text: `*Amount Settled:*\n${payload.amount / 1e6} USDC` },
          { type: "mrkdwn", text: `*Payer:*\n\`${payload.payer.slice(0, 4)}...${payload.payer.slice(-4)}\`` },
          { type: "mrkdwn", text: `*Solana Tx:*\n<https://explorer.solana.com/tx/${payload.signature}?cluster=devnet|View Explorer>` },
        ],
      },
    ],
  };

  try {
    await axios.post(SLACK_WEBHOOK_URL, slackBody, { timeout: 5000 });
  } catch (err: any) {
    console.warn(`[Telemetry] Slack hook warning (simulated/offline): ${err.message}`);
  }
}

/**
 * Resend Email Receipt Delivery
 */
export async function sendEmailReceipt(
  recipientEmail: string,
  payload: MozaikWebhookPayload
): Promise<void> {
  try {
    await resend.emails.send({
      from: "MozaikFlow Billing <billing@mozaikpay.enterprise>",
      to: recipientEmail,
      subject: `[Settled] Payment Receipt - Invoice ${payload.invoiceId}`,
      html: `
        <div style="font-family: sans-serif; max-width: 600px; margin: 0 auto; padding: 20px; border: 1px solid #e2e8f0; border-radius: 8px;">
          <h2 style="color: #4f46e5;">MozaikPay Instant Settlement Confirmation</h2>
          <p>Your payment has been atomically finalized on Solana.</p>
          <hr style="border: 0; border-top: 1px solid #cbd5e1; margin: 16px 0;" />
          <p><strong>Invoice ID:</strong> ${payload.invoiceId}</p>
          <p><strong>Amount:</strong> $${(payload.amount / 1e6).toFixed(2)} USDC</p>
          <p><strong>Transaction Signature:</strong></p>
          <pre style="background: #f1f5f9; padding: 8px; border-radius: 4px; overflow-x: auto;">${payload.signature}</pre>
          <p style="font-size: 12px; color: #64748b;">Powered by MozaikPay Protocol on Solana.</p>
        </div>
      `,
    });
    console.log(`[Email] Dispatch succeeded for Invoice ${payload.invoiceId} to ${recipientEmail}`);
  } catch (err: any) {
    console.warn(`[Email] Resend API Warning (simulated/fallback): ${err.message}`);
  }
}

// -----------------------------------------------------------------------------
// Webhook & Orchestration API Endpoints
// -----------------------------------------------------------------------------

app.post("/api/v1/mozaik-webhook", async (req: Request, res: Response) => {
  const payload: MozaikWebhookPayload = req.body;
  const signatureHeader = req.headers["x-mozaik-signature"] as string;
  const merchantPubKey = req.headers["x-mozaik-merchant-key"] as string;

  if (!payload || !payload.signature || !payload.invoiceId) {
    return res.status(400).json({ error: "Invalid payload parameters" });
  }

  // Idempotency check
  if (processedSignatures.has(payload.signature)) {
    return res.status(200).json({ status: "already_processed", invoiceId: payload.invoiceId });
  }

  // Cryptographic signature validation (bypassed if test key is set to development)
  if (merchantPubKey && signatureHeader && merchantPubKey !== "bypass_dev") {
    const isValid = verifyMozaikSignature(payload, signatureHeader, merchantPubKey);
    if (!isValid) {
      return res.status(401).json({ error: "Cryptographic signature validation failed" });
    }
  }

  // Mark processed
  processedSignatures.add(payload.signature);

  // Parallel asynchronous event dispatching
  Promise.all([
    notifySlackTreasury(payload),
    sendEmailReceipt("treasury-ops@enterprise-corp.com", payload),
  ]).catch((err) => console.error("[Pipeline Error]", err));

  return res.status(200).json({
    status: "success",
    settled: true,
    invoiceId: payload.invoiceId,
    tx: payload.signature,
  });
});

app.get("/healthz", (_req: Request, res: Response) => {
  res.status(200).json({
    status: "healthy",
    uptime: process.uptime(),
    activeSignatures: processedSignatures.size,
  });
});

if (process.env.NODE_ENV !== "test") {
  app.listen(PORT, () => {
    console.log(`⚡ MozaikFlow Enterprise Daemon online at port ${PORT}`);
  });
}

export default app;