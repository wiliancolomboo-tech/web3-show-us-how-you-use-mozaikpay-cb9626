import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { MozaikPulse } from "../target/types/mozaik_pulse";
import {
  createMint,
  createAccount,
  mintTo,
  getAccount,
  TOKEN_PROGRAM_ID,
} from "@solana/spl-token";
import { Keypair, PublicKey, SystemProgram } from "@solana/web3.js";
import { expect } from "chai";

describe("MozaikPulse Enterprise Protocol Tests", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.MozaikPulse as Program<MozaikPulse>;

  let mint: PublicKey;
  let payerTokenAccount: PublicKey;
  let vendor1TokenAccount: PublicKey;
  let vendor2TokenAccount: PublicKey;
  let affiliateTokenAccount: PublicKey;

  const payer = (provider.wallet as anchor.Wallet).payer;
  const vendor1 = Keypair.generate();
  const vendor2 = Keypair.generate();
  const affiliate = Keypair.generate();

  const merchantId = "ENTERPRISE-SaaS-001";
  let merchantConfigPda: PublicKey;
  let merchantBump: number;

  before(async () => {
    // 1. Create settlement token mint (e.g. Mock USDC)
    mint = await createMint(
      provider.connection,
      payer,
      payer.publicKey,
      null,
      6
    );

    // 2. Setup SPL Token Accounts
    payerTokenAccount = await createAccount(
      provider.connection,
      payer,
      mint,
      payer.publicKey
    );

    vendor1TokenAccount = await createAccount(
      provider.connection,
      payer,
      mint,
      vendor1.publicKey
    );

    vendor2TokenAccount = await createAccount(
      provider.connection,
      payer,
      mint,
      vendor2.publicKey
    );

    affiliateTokenAccount = await createAccount(
      provider.connection,
      payer,
      mint,
      affiliate.publicKey
    );

    // 3. Mint initial 10,000 USDC tokens to payer
    await mintTo(
      provider.connection,
      payer,
      mint,
      payerTokenAccount,
      payer.publicKey,
      10_000_000_000
    );

    [merchantConfigPda, merchantBump] = PublicKey.findProgramAddressSync(
      [
        Buffer.from("merchant_config"),
        payer.publicKey.toBuffer(),
        Buffer.from(merchantId),
      ],
      program.programId
    );
  });

  it("Initializes Merchant Split Configuration with exact 10,000 BPS", async () => {
    // 50% to Vendor 1, 30% to Vendor 2, 20% to Affiliate
    const splitPercentages = [5000, 3000, 2000];

    const tx = await program.methods
      .initializeMerchantConfig(merchantId, splitPercentages)
      .accounts({
        merchantConfig: merchantConfigPda,
        settlementMint: mint,
        authority: payer.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .remainingAccounts([
        { pubkey: vendor1TokenAccount, isWritable: false, isSigner: false },
        { pubkey: vendor2TokenAccount, isWritable: false, isSigner: false },
        { pubkey: affiliateTokenAccount, isWritable: false, isSigner: false },
      ])
      .rpc();

    expect(tx).to.be.a("string");

    const configAccount = await program.account.merchantConfig.fetch(merchantConfigPda);
    expect(configAccount.merchantId).to.equal(merchantId);
    expect(configAccount.recipientCount).to.equal(3);
    expect(configAccount.splits[0].basisPoints).to.equal(5000);
    expect(configAccount.splits[1].basisPoints).to.equal(3000);
    expect(configAccount.splits[2].basisPoints).to.equal(2000);
  });

  it("Fails initialization if Basis Points do not sum to 10,000", async () => {
    const invalidConfigId = "INVALID-BPS-TEST";
    const [invalidPda] = PublicKey.findProgramAddressSync(
      [
        Buffer.from("merchant_config"),
        payer.publicKey.toBuffer(),
        Buffer.from(invalidConfigId),
      ],
      program.programId
    );

    try {
      await program.methods
        .initializeMerchantConfig(invalidConfigId, [4000, 4000]) // Sums to 8000, not 10000
        .accounts({
          merchantConfig: invalidPda,
          settlementMint: mint,
          authority: payer.publicKey,
          systemProgram: SystemProgram.programId,
        })
        .remainingAccounts([
          { pubkey: vendor1TokenAccount, isWritable: false, isSigner: false },
          { pubkey: vendor2TokenAccount, isWritable: false, isSigner: false },
        ])
        .rpc();
      expect.fail("Expected transaction to fail with InvalidSplitBasisPoints");
    } catch (err: any) {
      expect(err.error.errorCode.code).to.equal("InvalidSplitBasisPoints");
    }
  });

  it("Executes deterministic checkout multi-split with dust absorption", async () => {
    const orderId = "ORDER-TEST-99882";
    const totalAmount = new anchor.BN(1_000_000_001); // 1,000.000001 USDC (tests remainder handling)

    await program.methods
      .executeCheckoutPayment(orderId, totalAmount)
      .accounts({
        merchantConfig: merchantConfigPda,
        payerTokenAccount: payerTokenAccount,
        payer: payer.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .remainingAccounts([
        { pubkey: vendor1TokenAccount, isWritable: true, isSigner: false },
        { pubkey: vendor2TokenAccount, isWritable: true, isSigner: false },
        { pubkey: affiliateTokenAccount, isWritable: true, isSigner: false },
      ])
      .rpc();

    // Verify Split Balances
    const bal1 = await getAccount(provider.connection, vendor1TokenAccount);
    const bal2 = await getAccount(provider.connection, vendor2TokenAccount);
    const balAffiliate = await getAccount(provider.connection, affiliateTokenAccount);

    // Vendor 1 (50%): 500_000_000
    expect(Number(bal1.amount)).to.equal(500_000_000);
    // Vendor 2 (30%): 300_000_000
    expect(Number(bal2.amount)).to.equal(300_000_000);
    // Affiliate (20% + remainder dust): 200_000_001
    expect(Number(balAffiliate.amount)).to.equal(200_000_001);
  });

  it("Creates a linear streaming escrow and enforces per-second curve vesting", async () => {
    const streamId = "PAYROLL-ENG-042";
    const totalDeposit = new anchor.BN(6_000_000_000); // 6,000 USDC
    const now = Math.floor(Date.now() / 1000);
    const startTime = new anchor.BN(now);
    const duration = 10; // 10 seconds for rapid testing
    const endTime = new anchor.BN(now + duration);

    const [streamPda] = PublicKey.findProgramAddressSync(
      [
        Buffer.from("stream_account"),
        payer.publicKey.toBuffer(),
        Buffer.from(streamId),
      ],
      program.programId
    );

    const [vaultPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("stream_vault"), Buffer.from(streamId)],
      program.programId
    );

    const recipient = Keypair.generate();
    const recipientTokenAccount = await createAccount(
      provider.connection,
      payer,
      mint,
      recipient.publicKey
    );

    // Create stream
    await program.methods
      .createLinearStream(streamId, totalDeposit, startTime, endTime)
      .accounts({
        streamAccount: streamPda,
        mint: mint,
        senderTokenAccount: payerTokenAccount,
        vaultTokenAccount: vaultPda,
        recipient: recipient.publicKey,
        sender: payer.publicKey,
        systemProgram: SystemProgram.programId,
        tokenProgram: TOKEN_PROGRAM_ID,
        rent: anchor.web3.SYSVAR_RENT_PUBKEY,
      })
      .rpc();

    // Fetch and check stream state
    const streamState = await program.account.streamAccount.fetch(streamPda);
    expect(streamState.totalDeposit.toNumber()).to.equal(totalDeposit.toNumber());
    expect(streamState.withdrawnAmount.toNumber()).to.equal(0);
    expect(streamState.isActive).to.be.true;

    // Wait 5 seconds (50% vested)
    await new Promise((resolve) => setTimeout(resolve, 5000));

    // Withdraw 1,000 USDC
    const withdrawAmount = new anchor.BN(1_000_000_000);
    await program.methods
      .withdrawFromStream(withdrawAmount)
      .accounts({
        streamAccount: streamPda,
        vaultTokenAccount: vaultPda,
        recipientTokenAccount: recipientTokenAccount,
        recipient: recipient.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .signers([recipient])
      .rpc();

    const recipientBal = await getAccount(provider.connection, recipientTokenAccount);
    expect(Number(recipientBal.amount)).to.equal(1_000_000_000);
  });
});