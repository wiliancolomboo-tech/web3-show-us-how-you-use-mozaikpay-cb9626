import * as anchor from '@coral-xyz/anchor';
import { Program } from '@coral-xyz/anchor';
import { MozaikEnterprisePay } from '../target/types/mozaik_enterprise_pay';
import {
  createMint,
  createAccount,
  mintTo,
  getAccount,
  TOKEN_PROGRAM_ID,
} from '@solana/spl-token';
import { PublicKey, Keypair, SystemProgram } from '@solana/web3.js';
import { expect } from 'chai';

describe('MozaikPulse Enterprise Test Matrix', () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.MozaikEnterprisePay as Program<MozaikEnterprisePay>;

  const admin = Keypair.generate();
  const payer = Keypair.generate();
  const unauthorizedUser = Keypair.generate();

  let settlementMint: PublicKey;
  let merchantVault: PublicKey;
  let taxVault: PublicKey;
  let platformVault: PublicKey;
  let payerTokenAccount: PublicKey;
  let merchantConfigPda: PublicKey;

  const MERCHANT_BPS = new anchor.BN(8500); // 85.00%
  const TAX_BPS = new anchor.BN(1000);      // 10.00%
  const PLATFORM_BPS = new anchor.BN(500);   //  5.00%

  before(async () => {
    // Airdrop SOL to test actors
    for (const actor of [admin, payer, unauthorizedUser]) {
      const sig = await provider.connection.requestAirdrop(actor.publicKey, 10 * anchor.web3.LAMPORTS_PER_SOL);
      await provider.connection.confirmTransaction(sig);
    }

    // Create settlement mint (USDC simulator with 6 decimals)
    settlementMint = await createMint(
      provider.connection,
      admin,
      admin.publicKey,
      null,
      6
    );

    // Create respective Token Accounts
    merchantVault = await createAccount(provider.connection, admin, settlementMint, admin.publicKey);
    taxVault = await createAccount(provider.connection, admin, settlementMint, admin.publicKey);
    platformVault = await createAccount(provider.connection, admin, settlementMint, admin.publicKey);
    payerTokenAccount = await createAccount(provider.connection, payer, settlementMint, payer.publicKey);

    // Mint $10,000 USDC to payer
    await mintTo(
      provider.connection,
      admin,
      settlementMint,
      payerTokenAccount,
      admin,
      10_000 * 1_000_000
    );

    [merchantConfigPda] = PublicKey.findProgramAddressSync(
      [Buffer.from('merchant_config'), admin.publicKey.toBuffer()],
      program.programId
    );
  });

  it('1. Successfully initializes merchant configuration with valid 10,000 BPS splits', async () => {
    await program.methods
      .initializeMerchantConfig(MERCHANT_BPS, TAX_BPS, PLATFORM_BPS)
      .accounts({
        admin: admin.publicKey,
        merchantConfig: merchantConfigPda,
        settlementMint,
        merchantVault,
        taxVault,
        platformVault,
        systemProgram: SystemProgram.programId,
        rent: anchor.web3.SYSVAR_RENT_PUBKEY,
      })
      .signers([admin])
      .rpc();

    const configAccount = await program.account.merchantConfig.fetch(merchantConfigPda);
    expect(configAccount.admin.toBase58()).to.equal(admin.publicKey.toBase58());
    expect(configAccount.merchantBps.toNumber()).to.equal(8500);
    expect(configAccount.taxBps.toNumber()).to.equal(1000);
    expect(configAccount.platformBps.toNumber()).to.equal(500);
  });

  it('2. Fails to initialize merchant configuration when split BPS sum != 10,000', async () => {
    const invalidAdmin = Keypair.generate();
    const sig = await provider.connection.requestAirdrop(invalidAdmin.publicKey, 2 * anchor.web3.LAMPORTS_PER_SOL);
    await provider.connection.confirmTransaction(sig);

    const [invalidConfigPda] = PublicKey.findProgramAddressSync(
      [Buffer.from('merchant_config'), invalidAdmin.publicKey.toBuffer()],
      program.programId
    );

    try {
      await program.methods
        .initializeMerchantConfig(new anchor.BN(8000), new anchor.BN(1000), new anchor.BN(500)) // Sum = 9500
        .accounts({
          admin: invalidAdmin.publicKey,
          merchantConfig: invalidConfigPda,
          settlementMint,
          merchantVault,
          taxVault,
          platformVault,
          systemProgram: SystemProgram.programId,
          rent: anchor.web3.SYSVAR_RENT_PUBKEY,
        })
        .signers([invalidAdmin])
        .rpc();
      expect.fail('Expected initializeMerchantConfig to fail with InvalidSplitBasisPoints');
    } catch (err: any) {
      expect(err.error.errorCode.code).to.equal('InvalidSplitBasisPoints');
    }
  });

  it('3. Successfully generates an invoice PDA with exact duration expiry', async () => {
    const invoiceId = 'INV-2025-001';
    const amount = new anchor.BN(1_000 * 1_000_000); // 1,000 USDC
    const duration = new anchor.BN(3600); // 1 hour

    const [invoicePda] = PublicKey.findProgramAddressSync(
      [Buffer.from('invoice'), merchantConfigPda.toBuffer(), Buffer.from(invoiceId)],
      program.programId
    );

    await program.methods
      .createInvoice(invoiceId, amount, duration)
      .accounts({
        admin: admin.publicKey,
        merchantConfig: merchantConfigPda,
        invoice: invoicePda,
        systemProgram: SystemProgram.programId,
      })
      .signers([admin])
      .rpc();

    const invoiceAccount = await program.account.invoice.fetch(invoicePda);
    expect(invoiceAccount.invoiceId).to.equal(invoiceId);
    expect(invoiceAccount.amount.toNumber()).to.equal(1_000_000_000);
    expect(invoiceAccount.amountPaid.toNumber()).to.equal(0);
    expect(invoiceAccount.status).to.deep.equal({ pending: {} });
  });

  it('4. Executes atomic 3-way split payment on invoice settlement', async () => {
    const invoiceId = 'INV-2025-001';
    const [invoicePda] = PublicKey.findProgramAddressSync(
      [Buffer.from('invoice'), merchantConfigPda.toBuffer(), Buffer.from(invoiceId)],
      program.programId
    );

    await program.methods
      .settleInvoice()
      .accounts({
        payer: payer.publicKey,
        merchantConfig: merchantConfigPda,
        invoice: invoicePda,
        payerTokenAccount,
        merchantVault,
        taxVault,
        platformVault,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .signers([payer])
      .rpc();

    // Verify On-Chain Balances
    const merchantVaultAcc = await getAccount(provider.connection, merchantVault);
    const taxVaultAcc = await getAccount(provider.connection, taxVault);
    const platformVaultAcc = await getAccount(provider.connection, platformVault);

    // 1,000 USDC Total:
    // Merchant: 85% = 850 USDC
    // Tax:      10% = 100 USDC
    // Platform:  5% =  50 USDC
    expect(Number(merchantVaultAcc.amount)).to.equal(850 * 1_000_000);
    expect(Number(taxVaultAcc.amount)).to.equal(100 * 1_000_000);
    expect(Number(platformVaultAcc.amount)).to.equal(50 * 1_000_000);

    const invoiceAccount = await program.account.invoice.fetch(invoicePda);
    expect(invoiceAccount.status).to.deep.equal({ settled: {} });
    expect(invoiceAccount.payer.toBase58()).to.equal(payer.publicKey.toBase58());
  });

  it('5. Rejects settlement of an already settled invoice (Double-Spend Protection)', async () => {
    const invoiceId = 'INV-2025-001';
    const [invoicePda] = PublicKey.findProgramAddressSync(
      [Buffer.from('invoice'), merchantConfigPda.toBuffer(), Buffer.from(invoiceId)],
      program.programId
    );

    try {
      await program.methods
        .settleInvoice()
        .accounts({
          payer: payer.publicKey,
          merchantConfig: merchantConfigPda,
          invoice: invoicePda,
          payerTokenAccount,
          merchantVault,
          taxVault,
          platformVault,
          tokenProgram: TOKEN_PROGRAM_ID,
        })
        .signers([payer])
        .rpc();
      expect.fail('Expected settleInvoice to fail with InvoiceAlreadySettledOrCancelled');
    } catch (err: any) {
      expect(err.error.errorCode.code).to.equal('InvoiceAlreadySettledOrCancelled');
    }
  });

  it('6. Rejects cancellation attempts from non-admin actors', async () => {
    const invoiceId = 'INV-2025-002';
    const amount = new anchor.BN(500 * 1_000_000);
    const duration = new anchor.BN(3600);

    const [invoicePda] = PublicKey.findProgramAddressSync(
      [Buffer.from('invoice'), merchantConfigPda.toBuffer(), Buffer.from(invoiceId)],
      program.programId
    );

    await program.methods
      .createInvoice(invoiceId, amount, duration)
      .accounts({
        admin: admin.publicKey,
        merchantConfig: merchantConfigPda,
        invoice: invoicePda,
        systemProgram: SystemProgram.programId,
      })
      .signers([admin])
      .rpc();

    try {
      await program.methods
        .cancelInvoice()
        .accounts({
          admin: unauthorizedUser.publicKey,
          merchantConfig: merchantConfigPda,
          invoice: invoicePda,
        })
        .signers([unauthorizedUser])
        .rpc();
      expect.fail('Expected cancelInvoice to fail with UnauthorizedMerchantAdmin');
    } catch (err: any) {
      expect(err.error.errorCode.code).to.equal('UnauthorizedMerchantAdmin');
    }
  });
});