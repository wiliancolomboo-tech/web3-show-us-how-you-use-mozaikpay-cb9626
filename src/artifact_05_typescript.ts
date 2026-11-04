import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { MozaikFlow } from "../target/types/mozaik_flow";
import {
  createMint,
  createAccount,
  mintTo,
  getAccount,
  TOKEN_PROGRAM_ID,
} from "@solana/spl-token";
import { expect } from "chai";
import crypto from "crypto";

describe("mozaik-flow protocol integration tests", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.MozaikFlow as Program<MozaikFlow>;

  let mint: anchor.web3.PublicKey;
  let customerTokenAccount: anchor.web3.PublicKey;
  let merchantVault: anchor.web3.PublicKey;
  let platformVault: anchor.web3.PublicKey;
  let taxVault: anchor.web3.PublicKey;
  let merchantConfigPda: anchor.web3.PublicKey;

  const authority = provider.wallet;
  const merchantId = "STORE_SOL_001";
  const platformFeeBps = 1000; // 10.00%
  const taxReserveBps = 500;   // 5.00%

  before(async () => {
    // 1. Create Mock USDC Mint
    mint = await createMint(
      provider.connection,
      (authority as any).payer,
      authority.publicKey,
      null,
      6
    );

    // 2. Initialize Recipient Token Accounts
    merchantVault = await createAccount(
      provider.connection,
      (authority as any).payer,
      mint,
      authority.publicKey
    );

    platformVault = await createAccount(
      provider.connection,
      (authority as any).payer,
      mint,
      authority.publicKey
    );

    taxVault = await createAccount(
      provider.connection,
      (authority as any).payer,
      mint,
      authority.publicKey
    );

    // 3. Initialize Customer Token Account with 1,000 USDC
    customerTokenAccount = await createAccount(
      provider.connection,
      (authority as any).payer,
      mint,
      authority.publicKey
    );

    await mintTo(
      provider.connection,
      (authority as any).payer,
      mint,
      customerTokenAccount,
      authority.publicKey,
      1_000_000_000 // 1,000 USDC
    );

    // 4. Derive Merchant Config PDA
    [merchantConfigPda] = anchor.web3.PublicKey.findProgramAddressSync(
      [
        Buffer.from("merchant_config"),
        authority.publicKey.toBuffer(),
        Buffer.from(merchantId),
      ],
      program.programId
    );
  });

  it("Initializes merchant split configuration successfully", async () => {
    await program.methods
      .initializeMerchantConfig(merchantId, platformFeeBps, taxReserveBps)
      .accounts({
        merchantConfig: merchantConfigPda,
        authority: authority.publicKey,
        platformVault: platformVault,
        taxVault: taxVault,
        merchantVault: merchantVault,
        systemProgram: anchor.web3.SystemProgram.programId,
      })
      .rpc();

    const configAccount = await program.account.merchantConfig.fetch(merchantConfigPda);
    expect(configAccount.merchantId).to.equal(merchantId);
    expect(configAccount.platformFeeBps).to.equal(platformFeeBps);
    expect(configAccount.taxReserveBps).to.equal(taxReserveBps);
    expect(configAccount.isPaused).to.be.false;
  });

  it("Executes atomic split payment with mathematical precision", async () => {
    const rawOrderId = crypto.randomBytes(32);
    const orderId = Array.from(rawOrderId);
    const grossAmount = new anchor.BN(100_000_000); // 100 USDC

    const [receiptPda] = anchor.web3.PublicKey.findProgramAddressSync(
      [
        Buffer.from("payment_receipt"),
        merchantConfigPda.toBuffer(),
        Buffer.from(orderId),
      ],
      program.programId
    );

    await program.methods
      .processSplitPayment(orderId, grossAmount)
      .accounts({
        merchantConfig: merchantConfigPda,
        paymentReceipt: receiptPda,
        payer: authority.publicKey,
        mint: mint,
        customerTokenAccount: customerTokenAccount,
        merchantVault: merchantVault,
        platformVault: platformVault,
        taxVault: taxVault,
        tokenProgram: TOKEN_PROGRAM_ID,
        systemProgram: anchor.web3.SystemProgram.programId,
      })
      .rpc();

    // Verify token account balances
    const merchantBalance = await getAccount(provider.connection, merchantVault);
    const platformBalance = await getAccount(provider.connection, platformVault);
    const taxBalance = await getAccount(provider.connection, taxVault);

    // 85% of 100 USDC = 85 USDC
    expect(Number(merchantBalance.amount)).to.equal(85_000_000);
    // 10% of 100 USDC = 10 USDC
    expect(Number(platformBalance.amount)).to.equal(10_000_000);
    // 5% of 100 USDC = 5 USDC
    expect(Number(taxBalance.amount)).to.equal(5_000_000);

    // Verify on-chain payment receipt
    const receipt = await program.account.paymentReceipt.fetch(receiptPda);
    expect(receipt.grossAmount.toNumber()).to.equal(100_000_000);
    expect(receipt.merchantAmount.toNumber()).to.equal(85_000_000);
    expect(receipt.platformFee.toNumber()).to.equal(10_000_000);
    expect(receipt.taxAmount.toNumber()).to.equal(5_000_000);
  });

  it("Fails deterministically when replay attack is attempted with same order_id", async () => {
    const rawOrderId = crypto.randomBytes(32);
    const orderId = Array.from(rawOrderId);
    const grossAmount = new anchor.BN(50_000_000);

    const [receiptPda] = anchor.web3.PublicKey.findProgramAddressSync(
      [
        Buffer.from("payment_receipt"),
        merchantConfigPda.toBuffer(),
        Buffer.from(orderId),
      ],
      program.programId
    );

    // First execution succeeds
    await program.methods
      .processSplitPayment(orderId, grossAmount)
      .accounts({
        merchantConfig: merchantConfigPda,
        paymentReceipt: receiptPda,
        payer: authority.publicKey,
        mint: mint,
        customerTokenAccount: customerTokenAccount,
        merchantVault: merchantVault,
        platformVault: platformVault,
        taxVault: taxVault,
        tokenProgram: TOKEN_PROGRAM_ID,
        systemProgram: anchor.web3.SystemProgram.programId,
      })
      .rpc();

    // Second execution with identical order ID must fail
    try {
      await program.methods
        .processSplitPayment(orderId, grossAmount)
        .accounts({
          merchantConfig: merchantConfigPda,
          paymentReceipt: receiptPda,
          payer: authority.publicKey,
          mint: mint,
          customerTokenAccount: customerTokenAccount,
          merchantVault: merchantVault,
          platformVault: platformVault,
          taxVault: taxVault,
          tokenProgram: TOKEN_PROGRAM_ID,
          systemProgram: anchor.web3.SystemProgram.programId,
        })
        .rpc();
      expect.fail("Expected transaction to fail due to account already in use.");
    } catch (err: any) {
      expect(err.toString()).to.include("already in use");
    }
  });

  it("Rejects initialization when fee splits exceed 100%", async () => {
    const badMerchantId = "BAD_MERCHANT_FEE";
    const [badPda] = anchor.web3.PublicKey.findProgramAddressSync(
      [
        Buffer.from("merchant_config"),
        authority.publicKey.toBuffer(),
        Buffer.from(badMerchantId),
      ],
      program.programId
    );

    try {
      await program.methods
        .initializeMerchantConfig(badMerchantId, 6000, 5000) // 110% total
        .accounts({
          merchantConfig: badPda,
          authority: authority.publicKey,
          platformVault: platformVault,
          taxVault: taxVault,
          merchantVault: merchantVault,
          systemProgram: anchor.web3.SystemProgram.programId,
        })
        .rpc();
      expect.fail("Should have thrown ExcessiveFeeSplit error");
    } catch (err: any) {
      expect(err.toString()).to.include("ExcessiveFeeSplit");
    }
  });
});