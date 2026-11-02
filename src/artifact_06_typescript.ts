import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { MozaikpayNexus } from "../target/types/mozaikpay_nexus";
import {
  TOKEN_PROGRAM_ID,
  createMint,
  createAccount,
  mintTo,
  getAccount,
} from "@solana/spl-token";
import { expect } from "chai";

describe("mozaikpay-nexus enterprise tests", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.MozaikpayNexus as Program<MozaikpayNexus>;

  let paymentMint: anchor.web3.PublicKey;
  let authority: anchor.web3.Keypair;
  let affiliate: anchor.web3.Keypair;
  let protocolFee: anchor.web3.Keypair;
  let payer: anchor.web3.Keypair;

  let merchantPda: anchor.web3.PublicKey;
  let treasuryVault: anchor.web3.PublicKey;
  let affiliateVault: anchor.web3.PublicKey;
  let protocolVault: anchor.web3.PublicKey;
  let payerTokenAccount: anchor.web3.PublicKey;

  const merchantId = "STORE_SOL_001";
  const affiliateBps = 1000; // 10%
  const protocolBps = 500;   // 5%

  before(async () => {
    authority = anchor.web3.Keypair.generate();
    affiliate = anchor.web3.Keypair.generate();
    protocolFee = anchor.web3.Keypair.generate();
    payer = anchor.web3.Keypair.generate();

    // Airdrop SOL
    for (const user of [authority, payer]) {
      const sig = await provider.connection.requestAirdrop(user.publicKey, 2 * anchor.web3.LAMPORTS_PER_SOL);
      await provider.connection.confirmTransaction(sig);
    }

    // Create payment currency mint (USDC surrogate)
    paymentMint = await createMint(
      provider.connection,
      authority,
      authority.publicKey,
      null,
      6
    );

    // Create Associated Token Accounts
    treasuryVault = await createAccount(provider.connection, authority, paymentMint, authority.publicKey);
    affiliateVault = await createAccount(provider.connection, authority, paymentMint, affiliate.publicKey);
    protocolVault = await createAccount(provider.connection, authority, paymentMint, protocolFee.publicKey);
    payerTokenAccount = await createAccount(provider.connection, payer, paymentMint, payer.publicKey);

    // Fund Payer with 10,000 USDC (6 decimals)
    await mintTo(
      provider.connection,
      authority,
      paymentMint,
      payerTokenAccount,
      authority.publicKey,
      10_000_000_000
    );

    [merchantPda] = anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("merchant"), authority.publicKey.toBuffer(), Buffer.from(merchantId)],
      program.programId
    );
  });

  it("1. Successfully registers an enterprise merchant with atomic rev-shares", async () => {
    await program.methods
      .initializeMerchant(merchantId, affiliateBps, protocolBps)
      .accounts({
        merchantAccount: merchantPda,
        authority: authority.publicKey,
        treasuryVault: treasuryVault,
        affiliatePayoutAccount: affiliateVault,
        protocolFeeAccount: protocolVault,
        paymentMint: paymentMint,
        systemProgram: anchor.web3.SystemProgram.programId,
      })
      .signers([authority])
      .rpc();

    const merchant = await program.account.merchantAccount.fetch(merchantPda);
    expect(merchant.merchantId).to.equal(merchantId);
    expect(merchant.affiliateShareBps).to.equal(1000);
    expect(merchant.protocolShareBps).to.equal(500);
    expect(merchant.isActive).to.be.true;
    expect(merchant.totalOrders.toNumber()).to.equal(0);
  });

  it("2. Fails when rev-shares exceed 100% (10,000 bps)", async () => {
    const invalidMerchantId = "STORE_INVALID";
    const [invalidPda] = anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("merchant"), authority.publicKey.toBuffer(), Buffer.from(invalidMerchantId)],
      program.programId
    );

    try {
      await program.methods
        .initializeMerchant(invalidMerchantId, 8000, 2500) // 105% total
        .accounts({
          merchantAccount: invalidPda,
          authority: authority.publicKey,
          treasuryVault: treasuryVault,
          affiliatePayoutAccount: affiliateVault,
          protocolFeeAccount: protocolVault,
          paymentMint: paymentMint,
          systemProgram: anchor.web3.SystemProgram.programId,
        })
        .signers([authority])
        .rpc();
      expect.fail("Should have thrown InvalidBpsConfiguration");
    } catch (err: any) {
      expect(err.error.errorCode.code).to.equal("InvalidBpsConfiguration");
    }
  });

  it("3. Executes exact atomic 3-way split payment settlement", async () => {
    const orderId = "ORD_EXP_999";
    const chargeAmount = new anchor.BN(1_000_000_000); // 1,000 USDC

    await program.methods
      .executeSplitSettlement(orderId, chargeAmount)
      .accounts({
        merchantAccount: merchantPda,
        payer: payer.publicKey,
        payerTokenAccount: payerTokenAccount,
        treasuryVault: treasuryVault,
        affiliatePayoutAccount: affiliateVault,
        protocolFeeAccount: protocolVault,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .signers([payer])
      .rpc();

    const merchantBalance = await getAccount(provider.connection, treasuryVault);
    const affiliateBalance = await getAccount(provider.connection, affiliateVault);
    const protocolBalance = await getAccount(provider.connection, protocolVault);

    // 85% = 850 USDC
    expect(Number(merchantBalance.amount)).to.equal(850_000_000);
    // 10% = 100 USDC
    expect(Number(affiliateBalance.amount)).to.equal(100_000_000);
    // 5% = 50 USDC
    expect(Number(protocolBalance.amount)).to.equal(50_000_000);

    const merchant = await program.account.merchantAccount.fetch(merchantPda);
    expect(merchant.totalOrders.toNumber()).to.equal(1);
    expect(merchant.totalVolume.toNumber()).to.equal(1_000_000_000);
  });

  it("4. Rejects zero or invalid amount checkouts", async () => {
    try {
      await program.methods
        .executeSplitSettlement("ORD_FAIL", new anchor.BN(0))
        .accounts({
          merchantAccount: merchantPda,
          payer: payer.publicKey,
          payerTokenAccount: payerTokenAccount,
          treasuryVault: treasuryVault,
          affiliatePayoutAccount: affiliateVault,
          protocolFeeAccount: protocolVault,
          tokenProgram: TOKEN_PROGRAM_ID,
        })
        .signers([payer])
        .rpc();
      expect.fail("Should have rejected zero amount");
    } catch (err: any) {
      expect(err.error.errorCode.code).to.equal("InvalidAmount");
    }
  });
});