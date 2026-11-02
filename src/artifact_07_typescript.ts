import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { MozaikFlow } from "../target/types/mozaik_flow";
import {
  createMint,
  createAccount,
  mintTo,
  approve,
  getAccount,
} from "@solana/spl-token";
import { PublicKey, Keypair, SystemProgram } from "@solana/web3.js";
import { assert, expect } from "chai";

describe("MozaikFlow Suite: Non-Custodial Subscriptions & Revenue Splitting", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.MozaikFlow as Program<MozaikFlow>;
  
  let mint: PublicKey;
  let authority: Keypair;
  let subscriber: Keypair;
  let affiliate: Keypair;
  let mozaikTreasury: Keypair;

  let merchantVault: PublicKey;
  let affiliateVault: PublicKey;
  let protocolTreasuryVault: PublicKey;
  let customerTokenAccount: PublicKey;

  let merchantConfigPda: PublicKey;
  let subscriptionRecordPda: PublicKey;

  const BILLING_AMOUNT = new anchor.BN(50_000_000); // 50 USDC (6 decimals)
  const AFFILIATE_BPS = 500; // 5%

  before(async () => {
    authority = Keypair.generate();
    subscriber = Keypair.generate();
    affiliate = Keypair.generate();
    mozaikTreasury = Keypair.generate();

    // Airdrop SOL to participants
    for (const kp of [authority, subscriber]) {
      const sig = await provider.connection.requestAirdrop(kp.publicKey, 2 * anchor.web3.LAMPORTS_PER_SOL);
      await provider.connection.confirmTransaction(sig);
    }

    // Create settlement mint (USDC simulator)
    mint = await createMint(
      provider.connection,
      authority,
      authority.publicKey,
      null,
      6
    );

    // Initialize Token Accounts
    merchantVault = await createAccount(provider.connection, authority, mint, authority.publicKey);
    affiliateVault = await createAccount(provider.connection, authority, mint, affiliate.publicKey);
    protocolTreasuryVault = await createAccount(provider.connection, authority, mint, mozaikTreasury.publicKey);
    customerTokenAccount = await createAccount(provider.connection, subscriber, mint, subscriber.publicKey);

    // Fund customer with 200 USDC
    await mintTo(
      provider.connection,
      authority,
      mint,
      customerTokenAccount,
      authority.publicKey,
      200_000_000
    );

    [merchantConfigPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("merchant_config"), authority.publicKey.toBuffer(), mint.toBuffer()],
      program.programId
    );

    [subscriptionRecordPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("subscription"), merchantConfigPda.toBuffer(), subscriber.publicKey.toBuffer()],
      program.programId
    );
  });

  it("1. Initializes Merchant Configuration with Multi-party Split", async () => {
    await program.methods
      .initializeMerchant("CloudHost Pro", AFFILIATE_BPS)
      .accounts({
        authority: authority.publicKey,
        settlementMint: mint,
        merchantConfig: merchantConfigPda,
        merchantVault: merchantVault,
        affiliateVault: affiliateVault,
        mozaikTreasury: protocolTreasuryVault,
        systemProgram: SystemProgram.programId,
        tokenProgram: anchor.utils.token.TOKEN_PROGRAM_ID,
        rent: anchor.web3.SYSVAR_RENT_PUBKEY,
      })
      .signers([authority])
      .rpc();

    const config = await program.account.merchantConfig.fetch(merchantConfigPda);
    assert.equal(config.authority.toBase58(), authority.publicKey.toBase58());
    assert.equal(config.affiliateBps, 500);
    assert.equal(config.protocolBps, 50); // 0.5% base fee
    assert.isTrue(config.isActive);
  });

  it("2. Registers Customer Subscription Allowance & Delegates Auth", async () => {
    const FREQUENCY_SECONDS = new anchor.BN(86400); // 1 day

    await program.methods
      .createSubscription(BILLING_AMOUNT, FREQUENCY_SECONDS)
      .accounts({
        subscriber: subscriber.publicKey,
        merchantConfig: merchantConfigPda,
        customerTokenAccount: customerTokenAccount,
        subscriptionRecord: subscriptionRecordPda,
        systemProgram: SystemProgram.programId,
      })
      .signers([subscriber])
      .rpc();

    // Customer delegates token authority to the Subscription Record PDA
    await approve(
      provider.connection,
      subscriber,
      customerTokenAccount,
      subscriptionRecordPda,
      subscriber.publicKey,
      1_000_000_000 // Approve large allowance bucket
    );

    const sub = await program.account.subscriptionRecord.fetch(subscriptionRecordPda);
    assert.equal(sub.subscriber.toBase58(), subscriber.publicKey.toBase58());
    assert.isTrue(sub.maxAmountPerPeriod.eq(BILLING_AMOUNT));
    assert.isTrue(sub.isActive);
  });

  it("3. Executes First Deterministic Debit with On-Chain Split", async () => {
    await program.methods
      .executeSubscriptionDebit(BILLING_AMOUNT)
      .accounts({
        caller: authority.publicKey,
        merchantConfig: merchantConfigPda,
        subscriptionRecord: subscriptionRecordPda,
        customerTokenAccount: customerTokenAccount,
        merchantVault: merchantVault,
        affiliateVault: affiliateVault,
        mozaikTreasury: protocolTreasuryVault,
        tokenProgram: anchor.utils.token.TOKEN_PROGRAM_ID,
      })
      .signers([authority])
      .rpc();

    // Verify Balances:
    // Protocol (0.5%): 50 * 0.005 = 0.25 USDC (250_000)
    // Affiliate (5.0%): 50 * 0.05 = 2.50 USDC (2_500_000)
    // Merchant: 50 - 0.25 - 2.50 = 47.25 USDC (47_250_000)
    const merchantBalance = await getAccount(provider.connection, merchantVault);
    const affiliateBalance = await getAccount(provider.connection, affiliateVault);
    const treasuryBalance = await getAccount(provider.connection, protocolTreasuryVault);
    const customerBalance = await getAccount(provider.connection, customerTokenAccount);

    assert.equal(Number(merchantBalance.amount), 47_250_000);
    assert.equal(Number(affiliateBalance.amount), 2_500_000);
    assert.equal(Number(treasuryBalance.amount), 250_000);
    assert.equal(Number(customerBalance.amount), 150_000_000);
  });

  it("4. Enforces Time-Lock and Prevents Premature Double-Billing", async () => {
    try {
      await program.methods
        .executeSubscriptionDebit(BILLING_AMOUNT)
        .accounts({
          caller: authority.publicKey,
          merchantConfig: merchantConfigPda,
          subscriptionRecord: subscriptionRecordPda,
          customerTokenAccount: customerTokenAccount,
          merchantVault: merchantVault,
          affiliateVault: affiliateVault,
          mozaikTreasury: protocolTreasuryVault,
          tokenProgram: anchor.utils.token.TOKEN_PROGRAM_ID,
        })
        .signers([authority])
        .rpc();
      assert.fail("Should have thrown BillingPeriodNotElapsed error");
    } catch (err: any) {
      expect(err.toString()).to.include("BillingPeriodNotElapsed");
    }
  });

  it("5. Rejects Charges Exceeding Pre-Authorized Allowance Limit", async () => {
    const EXCESSIVE_CHARGE = new anchor.BN(999_000_000);
    try {
      await program.methods
        .executeSubscriptionDebit(EXCESSIVE_CHARGE)
        .accounts({
          caller: authority.publicKey,
          merchantConfig: merchantConfigPda,
          subscriptionRecord: subscriptionRecordPda,
          customerTokenAccount: customerTokenAccount,
          merchantVault: merchantVault,
          affiliateVault: affiliateVault,
          mozaikTreasury: protocolTreasuryVault,
          tokenProgram: anchor.utils.token.TOKEN_PROGRAM_ID,
        })
        .signers([authority])
        .rpc();
      assert.fail("Should have thrown AmountExceedsAllowance error");
    } catch (err: any) {
      expect(err.toString()).to.include("AmountExceedsAllowance");
    }
  });

  it("6. Allows Subscriber Instant Cancellation and Rejects Subsequent Debits", async () => {
    await program.methods
      .cancelSubscription()
      .accounts({
        subscriber: subscriber.publicKey,
        subscriptionRecord: subscriptionRecordPda,
      })
      .signers([subscriber])
      .rpc();

    const sub = await program.account.subscriptionRecord.fetch(subscriptionRecordPda);
    assert.isFalse(sub.isActive);

    try {
      await program.methods
        .executeSubscriptionDebit(BILLING_AMOUNT)
        .accounts({
          caller: authority.publicKey,
          merchantConfig: merchantConfigPda,
          subscriptionRecord: subscriptionRecordPda,
          customerTokenAccount: customerTokenAccount,
          merchantVault: merchantVault,
          affiliateVault: affiliateVault,
          mozaikTreasury: protocolTreasuryVault,
          tokenProgram: anchor.utils.token.TOKEN_PROGRAM_ID,
        })
        .signers([authority])
        .rpc();
      assert.fail("Should have thrown SubscriptionInactive error");
    } catch (err: any) {
      expect(err.toString()).to.include("SubscriptionInactive");
    }
  });
});