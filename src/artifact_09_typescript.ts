import { Command } from 'commander';
import { PublicKey } from '@solana/web3.js';

const program = new Command();

program
  .name('mozaik-cli')
  .description('MozaikPay Enterprise CLI - Generate Solana Pay POS Invoices & Settle Accounts')
  .version('1.0.0');

program
  .command('create-pos-invoice')
  .description('Generate an enterprise checkout QR payload and Solana Pay deep-link')
  .requiredOption('-i, --id <invoiceId>', 'Unique enterprise Invoice Identifier')
  .requiredOption('-a, --amount <amount>', 'Amount in USDC (e.g. 250.00)')
  .requiredOption('-m, --merchant <pubkey>', 'Merchant Admin Public Key')
  .action((options) => {
    const amountFloat = parseFloat(options.amount);
    const amountAtomic = Math.round(amountFloat * 1_000_000);
    const merchantPubkey = new PublicKey(options.merchant);

    // Compute deterministic PDA
    const programId = new PublicKey(process.env.PROGRAM_ID || 'MoZ1kPayEnterpriseEngine1111111111111111111');
    const [merchantConfig] = PublicKey.findProgramAddressSync(
      [Buffer.from('merchant_config'), merchantPubkey.toBuffer()],
      programId
    );

    const [invoicePda] = PublicKey.findProgramAddressSync(
      [Buffer.from('invoice'), merchantConfig.toBuffer(), Buffer.from(options.id)],
      programId
    );

    // Construct Standard Solana Pay Deep Link for MozaikPay Checkout
    const solanaPayUrl = `solana:https://api.mozaikpay.io/v1/pay?invoice=${invoicePda.toBase58()}&amount=${amountFloat}&label=Enterprise%20Checkout`;

    console.log('========================================================');
    console.log('           MOZAIKPULSE ENTERPRISE CHECKOUT LINK         ');
    console.log('========================================================');
    console.log(`Invoice ID:        ${options.id}`);
    console.log(`Settlement Amount: $${amountFloat.toFixed(2)} USDC (${amountAtomic} raw units)`);
    console.log(`Merchant Config:   ${merchantConfig.toBase58()}`);
    console.log(`Invoice PDA:       ${invoicePda.toBase58()}`);
    console.log('--------------------------------------------------------');
    console.log(`Solana Pay URI:`);
    console.log(`\x1b[36m${solanaPayUrl}\x1b[0],`);
    console.log('--------------------------------------------------------');
    console.log('QR Code Payload generated. Ready for Phantom/Solflare terminal scan.');
  });

program.parse(process.argv);