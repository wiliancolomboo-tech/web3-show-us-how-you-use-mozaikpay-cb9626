import Fastify from 'fastify';
import crypto from 'crypto';
import Redis from 'ioredis';
import dotenv from 'dotenv';
import { MozaikPayWebhookPayloadSchema } from './types';
import { SolanaAuditor } from './services/solanaAuditor';
import { EmailService } from './services/emailService';
import { SlackService } from './services/slackService';

dotenv.config();

const app = Fastify({ logger: true });

const REDIS_HOST = process.env.REDIS_HOST || 'localhost';
const REDIS_PORT = Number(process.env.REDIS_PORT) || 6379;
const redis = new Redis({ host: REDIS_HOST, port: REDIS_PORT });

const SOLANA_RPC = process.env.SOLANA_RPC_URL || 'https://api.mainnet-beta.solana.com';
const auditor = new SolanaAuditor(SOLANA_RPC);

const emailService = new EmailService(
  process.env.RESEND_API_KEY || 're_test_key',
  process.env.SENDER_EMAIL || 'billing@example.com'
);

const slackService = new SlackService(process.env.SLACK_WEBHOOK_URL || 'https://api.example.com/webhooks/alerts/placeholder');
const MOZAIKPAY_WEBHOOK_SECRET = process.env.MOZAIKPAY_WEBHOOK_SECRET || 'dev_secret';

// Health Check Endpoint
app.get('/health', async () => {
  const redisStatus = redis.status === 'ready' ? 'healthy' : 'degraded';
  return { status: 'healthy', redis: redisStatus, timestamp: new Date().toISOString() };
});

// MozaikPay Inbound Webhook
app.post('/api/v1/mozaikpay-webhook', async (request, reply) => {
  const signatureHeader = request.headers['x-mozaikpay-signature'] as string;
  const rawBody = JSON.stringify(request.body);

  // 1. Verify HMAC Signature
  const calculatedHmac = crypto
    .createHmac('sha256', MOZAIKPAY_WEBHOOK_SECRET)
    .update(rawBody)
    .digest('hex');

  if (signatureHeader !== calculatedHmac) {
    app.log.warn('Unauthorized webhook signature mismatch.');
    return reply.status(401).send({ error: 'Signature verification failed' });
  }

  // 2. Validate Schema Payload
  const parsed = MozaikPayWebhookPayloadSchema.safeParse(request.body);
  if (!parsed.success) {
    app.log.error({ error: parsed.error }, 'Malformed webhook payload structure.');
    return reply.status(400).send({ error: 'Invalid payload schema' });
  }

  const payload = parsed.data;
  const { orderId, customerPublicKey, customerEmail, amountUnits, transactionSignature, referencePublicKey, planTier } = payload.data;

  // 3. Distributed Lock & Idempotency Key (Redis)
  const idempotencyKey = `mozaik:processed:${payload.eventId}`;
  const isNewEvent = await redis.set(idempotencyKey, 'LOCKED', 'EX', 86400 * 3, 'NX');

  if (!isNewEvent) {
    app.log.info({ eventId: payload.eventId }, 'Duplicate event ignored via idempotency check.');
    return reply.status(200).send({ status: 'duplicate_event_acknowledged' });
  }

  // 4. Audit On-Chain Solana Finality
  const isValidTx = await auditor.verifyPaymentTransaction(
    transactionSignature,
    customerPublicKey,
    referencePublicKey
  );

  if (!isValidTx) {
    await redis.del(idempotencyKey); // Release lock to allow legitimate retry if RPC was simply lagging
    return reply.status(422).send({ error: 'Transaction validation failed on Solana RPC' });
  }

  // 5. Calculate Credits & Format Decimal Amount (Assuming 6 Decimals for USDC)
  const amountUsdc = (Number(amountUnits) / 1_000_000).toFixed(2);
  const creditsAllocated = planTier === 'enterprise_mesh' ? 1_000_000 : 200_000;

  // 6. Asynchronous Downstream Integrations (Parallelized)
  await Promise.allSettled([
    emailService.sendInvoiceReceipt(customerEmail, orderId, amountUsdc, creditsAllocated, transactionSignature),
    slackService.notifyPaymentSettled(orderId, customerPublicKey, amountUsdc, planTier, transactionSignature),
  ]);

  app.log.info({ orderId, customerPublicKey }, 'MozaikPay settlement fully orchestrated successfully.');
  return reply.status(200).send({ status: 'success', creditsProvisioned: creditsAllocated });
});

const start = async () => {
  try {
    const port = Number(process.env.PORT) || 8080;
    await app.listen({ port, host: '0.0.0.0' });
    console.log(`MozaikFlow Enterprise Relayer listening on port ${port}`);
  } catch (err) {
    app.log.error(err);
    process.exit(1);
  }
};

start();