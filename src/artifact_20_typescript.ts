import Fastify from 'fastify';
import { Keypair } from '@solana/web3.js';
import { Config } from './config';
import { SolanaSettlementListener } from './services/solanaListener';

const app = Fastify({ logger: true });
const settlementListener = new SolanaSettlementListener();

// Healthcheck endpoint
app.get('/healthz', async () => {
  return { status: 'healthy', timestamp: new Date().toISOString(), protocol: 'MozaikPay-Engine' };
});

// Create Invoice endpoint (Used by frontend checkout)
app.post('/api/v1/invoices/create', async (request, reply) => {
  const body = request.body as {
    merchantId: string;
    amountUsdc: number;
    customerEmail: string;
  };

  if (!body.amountUsdc || !body.customerEmail) {
    return reply.status(400).send({ error: 'Missing required invoice parameters' });
  }

  // Generate ephemeral reference Keypair for Solana Pay deterministic reconciliation
  const ephemeralReference = Keypair.generate();
  const invoiceIdHex = Buffer.from(Keypair.generate().publicKey.toBytes().slice(0, 16)).toString('hex');

  const registeredInvoice = {
    invoiceIdHex,
    referencePubkey: ephemeralReference.publicKey.toBase58(),
    expectedAmount: body.amountUsdc,
    recipientEmail: body.customerEmail,
    status: 'PENDING' as const
  };

  settlementListener.registerPendingInvoice(registeredInvoice);

  return reply.status(201).send({
    success: true,
    invoiceId: invoiceIdHex,
    reference: ephemeralReference.publicKey.toBase58(),
    recipientEmail: body.customerEmail,
    amountUsdc: body.amountUsdc,
    solanaPayUrl: `solana:${Config.PROGRAM_ID}?amount=${body.amountUsdc}&reference=${ephemeralReference.publicKey.toBase58()}&label=MozaikPay%20Checkout`
  });
});

// Verification check webhook / poller trigger
app.post('/api/v1/invoices/verify', async (request, reply) => {
  const body = request.body as { reference: string };
  if (!body.reference) {
    return reply.status(400).send({ error: 'Missing reference public key' });
  }

  const settled = await settlementListener.pollReferenceAccount(body.reference);
  return reply.send({ verified: settled });
});

const start = async () => {
  try {
    await app.listen({ port: parseInt(Config.PORT), host: '0.0.0.0' });
    console.log(`MozaikEngine Enterprise Gateway running on port ${Config.PORT}`);
  } catch (err) {
    app.log.error(err);
    process.exit(1);
  }
};

if (process.env.NODE_ENV !== 'test') {
  start();
}

export { app, settlementListener };