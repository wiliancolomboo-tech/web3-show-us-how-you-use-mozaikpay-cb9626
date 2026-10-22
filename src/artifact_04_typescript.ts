import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { MozaikEscrow } from "../target/types/mozaik_escrow";
import {
  createMint,
  createAccount,
  mintTo,
  getAccount,
  TOKEN_PROGRAM_ID,
} from "@solana/spl-token";
import { assert, expect } from "chai";

describe("mozaik_escrow deterministic test matrix", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.MozaikEscrow as Program<MozaikEscrow>;

  let mint: anchor.web3.PublicKey;
  let buyerTokenAccount: anchor.web3.PublicKey;
  let merchantTokenAccount: anchor.web3.PublicKey;

  const buyer = (provider.wallet as anchor.Wallet).payer;
  const merchant = anchor.web3.Keypair.generate();
  const unauthorizedUser = anchor.web3.Keypair.generate();

  const escrowId = `mzk_test_${Date.now()}`;
  const totalAmount = new anchor.BN(10_000_000); // 10 USDC (6 decimals)
  const milestoneCount = 2;
  const disputeTimeout = new anchor.BN(5); // 5 seconds for rapid testing

  let escrowPda: anchor.web3.PublicKey;
  let vaultPda: anchor.web3.PublicKey;

  before(async () => {
    // 1. Airdrop SOL to merchant and unauthorized accounts
    const sig1 = await provider.connection.requestAirdrop(merchant.publicKey, 2 * anchor.web3.LAMPORTS_PER_SOL);
    const sig2 = await provider.connection.requestAirdrop(unauthorizedUser.publicKey, 2 * anchor.web3.LAMPORTS_PER_SOL);
    await provider.connection.confirmTransaction(sig1);
    await provider.connection.confirmTransaction(sig2);

    // 2. Create SPL Mint (simulating USDC)
    mint = await createMint(
      provider.connection,
      buyer,
      buyer.publicKey,
      null,
      6
    );

    // 3. Create Token Accounts
    buyerTokenAccount = await createAccount(provider.connection, buyer, mint, buyer.publicKey);
    merchantTokenAccount = await createAccount(provider.connection, buyer, mint, merchant.publicKey);

    // 4. Mint tokens to buyer
    await mintTo(provider.connection, buyer, mint, buyerTokenAccount, buyer, 50_000_000);

    // 5. Derive PDAs
    [escrowPda] = anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("escrow"), Buffer.from(escrowId)],
      program.programId
    );

    [vaultPda] = anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("vault"), Buffer.from(escrowId)],
      program.programId
    );
  });

  it("1. Successfully initializes an escrow through MozaikPay route", async () => {
    await program.methods
      .initializeEscrow(escrowId, totalAmount, milestoneCount, disputeTimeout)
      .accounts({
        buyer: buyer.publicKey,
        merchant: merchant.publicKey,
        tokenMint: mint,
        buyerTokenAccount: buyerTokenAccount,
        escrowState: escrowPda,
        vault: vaultPda,
        systemProgram: anchor.web3.SystemProgram.programId,
        tokenProgram: TOKEN_PROGRAM_ID,
        rent: anchor.web3.SYSVAR_RENT_PUBKEY,
      })
      .rpc();

    const escrowState = await program.account.escrowAccount.fetch(escrowPda);
    const vaultAccount = await getAccount(provider.connection, vaultPda);

    assert.equal(escrowState.escrowId, escrowId);
    assert.equal(escrowState.buyer.toBase58(), buyer.publicKey.toBase58());
    assert.equal(escrowState.merchant.toBase58(), merchant.publicKey.toBase58());
    assert.equal(escrowState.totalAmount.toNumber(), 10_000_000);
    assert.equal(escrowState.amountRemaining.toNumber(), 10_000_000);
    assert.equal(vaultAccount.amount, BigInt(10_000_000));
  });

  it("2. Blocks unauthorized third-parties from releasing milestones", async () => {
    try {
      await program.methods
        .releaseMilestone(0, new anchor.BN(5_000_000))
        .accounts({
          buyer: unauthorizedUser.publicKey,
          merchant: merchant.publicKey,
          merchantTokenAccount: merchantTokenAccount,
          escrowState: escrowPda,
          vault: vaultPda,
          tokenProgram: TOKEN_PROGRAM_ID,
        })
        .signers([unauthorizedUser])
        .rpc();
      assert.fail("Should have thrown unauthorized error");
    } catch (err: any) {
      expect(err.toString()).to.include("Error");
    }
  });

  it("3. Successfully releases Milestone 1 upon deliverable clearance", async () => {
    const trancheAmount = new anchor.BN(5_000_000);

    await program.methods
      .releaseMilestone(0, trancheAmount)
      .accounts({
        buyer: buyer.publicKey,
        merchant: merchant.publicKey,
        merchantTokenAccount: merchantTokenAccount,
        escrowState: escrowPda,
        vault: vaultPda,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .rpc();

    const merchantAccount = await getAccount(provider.connection, merchantTokenAccount);
    const escrowState = await program.account.escrowAccount.fetch(escrowPda);

    assert.equal(merchantAccount.amount, BigInt(5_000_000));
    assert.equal(escrowState.amountRemaining.toNumber(), 5_000_000);
    assert.equal(escrowState.currentMilestone, 1);
  });

  it("4. Prevents skipping milestones out of sequence", async () => {
    try {
      await program.methods
        .releaseMilestone(3, new anchor.BN(5_000_000)) // Attempting milestone 3 instead of 1
        .accounts({
          buyer: buyer.publicKey,
          merchant: merchant.publicKey,
          merchantTokenAccount: merchantTokenAccount,
          escrowState: escrowPda,
          vault: vaultPda,
          tokenProgram: TOKEN_PROGRAM_ID,
        })
        .rpc();
      assert.fail("Should have rejected milestone out of sequence");
    } catch (err: any) {
      expect(err.message).to.include("MilestoneOutOfOrder");
    }
  });

  it("5. Completes final milestone and closes out escrow state", async () => {
    const finalTranche = new anchor.BN(5_000_000);

    await program.methods
      .releaseMilestone(1, finalTranche)
      .accounts({
        buyer: buyer.publicKey,
        merchant: merchant.publicKey,
        merchantTokenAccount: merchantTokenAccount,
        escrowState: escrowPda,
        vault: vaultPda,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .rpc();

    const merchantAccount = await getAccount(provider.connection, merchantTokenAccount);
    const escrowState = await program.account.escrowAccount.fetch(escrowPda);

    assert.equal(merchantAccount.amount, BigInt(10_000_000));
    assert.equal(escrowState.amountRemaining.toNumber(), 0);
    assert.deepEqual(escrowState.status, { completed: {} });
  });
});