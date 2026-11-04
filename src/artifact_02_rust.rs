use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};

declare_id!("MzkFLow111111111111111111111111111111111111");

pub const CONFIG_SEED: &[u8] = b"merchant_config";
pub const INVOICE_SEED: &[u8] = b"invoice_receipt";

// Basis points denominator (10,000 = 100%)
pub const BPS_DENOMINATOR: u64 = 10_000;
pub const MAX_AFFILIATE_BPS: u64 = 3_000; // Max 30%
pub const PROTOCOL_FEE_BPS: u64 = 200;    // 2.0%

#[program]
pub mod mozaikflow_settlement {
    use super::*;

    /// Initialize a merchant's payment parameters and split payout accounts.
    pub fn initialize_merchant(
        ctx: Context<InitializeMerchant>,
        affiliate_bps: u64,
    ) -> Result<()> {
        require!(
            affiliate_bps <= MAX_AFFILIATE_BPS,
            MozaikError::InvalidAffiliateBps
        );

        let merchant_config = &mut ctx.accounts.merchant_config;
        merchant_config.merchant_authority = ctx.accounts.merchant_authority.key();
        merchant_config.settlement_mint = ctx.accounts.settlement_mint.key();
        merchant_config.merchant_vault = ctx.accounts.merchant_vault.key();
        merchant_config.affiliate_vault = ctx.accounts.affiliate_vault.key();
        merchant_config.treasury_vault = ctx.accounts.treasury_vault.key();
        merchant_config.affiliate_bps = affiliate_bps;
        merchant_config.protocol_fee_bps = PROTOCOL_FEE_BPS;
        merchant_config.bump = ctx.bumps.merchant_config;
        merchant_config.is_active = true;

        emit!(MerchantConfigInitialized {
            merchant: merchant_config.merchant_authority,
            settlement_mint: merchant_config.settlement_mint,
            affiliate_bps,
            protocol_fee_bps: PROTOCOL_FEE_BPS,
        });

        Ok(())
    }

    /// Process atomic payment, split shares, and log deterministic reference receipt.
    pub fn process_payment(
        ctx: Context<ProcessPayment>,
        invoice_id: [u8; 32],
        expected_amount: u64,
    ) -> Result<()> {
        let config = &ctx.accounts.merchant_config;
        require!(config.is_active, MozaikError::MerchantInactive);
        require!(expected_amount > 0, MozaikError::ZeroPaymentAmount);

        // Calculate split allocations
        let protocol_share = expected_amount
            .checked_mul(config.protocol_fee_bps)
            .ok_or(MozaikError::MathOverflow)?
            .checked_div(BPS_DENOMINATOR)
            .ok_or(MozaikError::MathOverflow)?;

        let affiliate_share = expected_amount
            .checked_mul(config.affiliate_bps)
            .ok_or(MozaikError::MathOverflow)?
            .checked_div(BPS_DENOMINATOR)
            .ok_or(MozaikError::MathOverflow)?;

        let merchant_share = expected_amount
            .checked_sub(protocol_share)
            .ok_or(MozaikError::MathOverflow)?
            .checked_sub(affiliate_share)
            .ok_or(MozaikError::MathOverflow)?;

        // 1. Transfer protocol share
        if protocol_share > 0 {
            token::transfer(
                CpiContext::new(
                    ctx.accounts.token_program.to_account_info(),
                    Transfer {
                        from: ctx.accounts.payer_token_account.to_account_info(),
                        to: ctx.accounts.treasury_vault.to_account_info(),
                        authority: ctx.accounts.payer.to_account_info(),
                    },
                ),
                protocol_share,
            )?;
        }

        // 2. Transfer affiliate share
        if affiliate_share > 0 {
            token::transfer(
                CpiContext::new(
                    ctx.accounts.token_program.to_account_info(),
                    Transfer {
                        from: ctx.accounts.payer_token_account.to_account_info(),
                        to: ctx.accounts.affiliate_vault.to_account_info(),
                        authority: ctx.accounts.payer.to_account_info(),
                    },
                ),
                affiliate_share,
            )?;
        }

        // 3. Transfer merchant net share
        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.payer_token_account.to_account_info(),
                    to: ctx.accounts.merchant_vault.to_account_info(),
                    authority: ctx.accounts.payer.to_account_info(),
                },
            ),
            merchant_share,
        )?;

        // Record receipt state PDA to ensure strict idempotency on-chain
        let receipt = &mut ctx.accounts.invoice_receipt;
        receipt.invoice_id = invoice_id;
        receipt.payer = ctx.accounts.payer.key();
        receipt.reference = ctx.accounts.payment_reference.key();
        receipt.total_amount = expected_amount;
        receipt.merchant_share = merchant_share;
        receipt.affiliate_share = affiliate_share;
        receipt.protocol_share = protocol_share;
        receipt.timestamp = Clock::get()?.unix_timestamp;
        receipt.bump = ctx.bumps.invoice_receipt;

        emit!(PaymentSettled {
            invoice_id,
            payer: ctx.accounts.payer.key(),
            reference: ctx.accounts.payment_reference.key(),
            total_amount: expected_amount,
            merchant_share,
            affiliate_share,
            protocol_share,
            timestamp: receipt.timestamp,
        });

        Ok(())
    }
}

// -----------------------------------------------------------------------------
// ACCOUNT VALIDATION STRUCTS
// -----------------------------------------------------------------------------

#[derive(Accounts)]
#[instruction(affiliate_bps: u64)]
pub struct InitializeMerchant<'info> {
    #[account(mut)]
    pub merchant_authority: Signer<'info>,

    pub settlement_mint: Account<'info, Mint>,

    #[account(
        init,
        payer = merchant_authority,
        space = 8 + MerchantConfig::INIT_SPACE,
        seeds = [CONFIG_SEED, merchant_authority.key().as_ref(), settlement_mint.key().as_ref()],
        bump
    )]
    pub merchant_config: Account<'info, MerchantConfig>,

    #[account(
        constraint = merchant_vault.mint == settlement_mint.key() @ MozaikError::InvalidMintMismatch
    )]
    pub merchant_vault: Account<'info, TokenAccount>,

    #[account(
        constraint = affiliate_vault.mint == settlement_mint.key() @ MozaikError::InvalidMintMismatch
    )]
    pub affiliate_vault: Account<'info, TokenAccount>,

    #[account(
        constraint = treasury_vault.mint == settlement_mint.key() @ MozaikError::InvalidMintMismatch
    )]
    pub treasury_vault: Account<'info, TokenAccount>,

    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
#[instruction(invoice_id: [u8; 32])]
pub struct ProcessPayment<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(
        mut,
        constraint = payer_token_account.owner == payer.key() @ MozaikError::PayerMismatch,
        constraint = payer_token_account.mint == merchant_config.settlement_mint @ MozaikError::InvalidMintMismatch
    )]
    pub payer_token_account: Account<'info, TokenAccount>,

    #[account(
        seeds = [CONFIG_SEED, merchant_config.merchant_authority.as_ref(), merchant_config.settlement_mint.as_ref()],
        bump = merchant_config.bump
    )]
    pub merchant_config: Account<'info, MerchantConfig>,

    #[account(
        mut,
        address = merchant_config.merchant_vault @ MozaikError::UnauthorizedVault
    )]
    pub merchant_vault: Account<'info, TokenAccount>,

    #[account(
        mut,
        address = merchant_config.affiliate_vault @ MozaikError::UnauthorizedVault
    )]
    pub affiliate_vault: Account<'info, TokenAccount>,

    #[account(
        mut,
        address = merchant_config.treasury_vault @ MozaikError::UnauthorizedVault
    )]
    pub treasury_vault: Account<'info, TokenAccount>,

    /// CHECK: MozaikPay Solana Pay reference pubkey. Must be passed as non-signer for index tracking.
    pub payment_reference: AccountInfo<'info>,

    #[account(
        init,
        payer = payer,
        space = 8 + InvoiceReceipt::INIT_SPACE,
        seeds = [INVOICE_SEED, invoice_id.as_ref()],
        bump
    )]
    pub invoice_receipt: Account<'info, InvoiceReceipt>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

// -----------------------------------------------------------------------------
// STATE DEFINITIONS
// -----------------------------------------------------------------------------

#[account]
#[derive(InitSpace)]
pub struct MerchantConfig {
    pub merchant_authority: Pubkey,
    pub settlement_mint: Pubkey,
    pub merchant_vault: Pubkey,
    pub affiliate_vault: Pubkey,
    pub treasury_vault: Pubkey,
    pub affiliate_bps: u64,
    pub protocol_fee_bps: u64,
    pub bump: u8,
    pub is_active: bool,
}

#[account]
#[derive(InitSpace)]
pub struct InvoiceReceipt {
    pub invoice_id: [u8; 32],
    pub payer: Pubkey,
    pub reference: Pubkey,
    pub total_amount: u64,
    pub merchant_share: u64,
    pub affiliate_share: u64,
    pub protocol_share: u64,
    pub timestamp: i64,
    pub bump: u8,
}

// -----------------------------------------------------------------------------
// EVENTS & ERRORS
// -----------------------------------------------------------------------------

#[event]
pub struct MerchantConfigInitialized {
    pub merchant: Pubkey,
    pub settlement_mint: Pubkey,
    pub affiliate_bps: u64,
    pub protocol_fee_bps: u64,
}

#[event]
pub struct PaymentSettled {
    pub invoice_id: [u8; 32],
    pub payer: Pubkey,
    pub reference: Pubkey,
    pub total_amount: u64,
    pub merchant_share: u64,
    pub affiliate_share: u64,
    pub protocol_share: u64,
    pub timestamp: i64,
}

#[error_code]
pub enum MozaikError {
    #[msg("Affiliate basis points exceeds allowed threshold of 3000 bps")]
    InvalidAffiliateBps,
    #[msg("Merchant configuration is currently deactivated")]
    MerchantInactive,
    #[msg("Payment amount must be greater than zero")]
    ZeroPaymentAmount,
    #[msg("Token account mint does not match merchant settlement mint")]
    InvalidMintMismatch,
    #[msg("Specified payer does not own the payer token account")]
    PayerMismatch,
    #[msg("Provided vault does not match the merchant registered vault address")]
    UnauthorizedVault,
    #[msg("Arithmetic overflow occurred during revenue split calculation")]
    MathOverflow,
}