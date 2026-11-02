use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};

declare_id!("MoZ1kPayEnterpriseEngine1111111111111111111");

pub const BASIS_POINTS_DIVISOR: u64 = 10_000;
pub const MAX_INVOICE_ID_LEN: usize = 32;

#[program]
pub mod mozaik_enterprise_pay {
    use super::*;

    /// Initializes a merchant's configuration, settlement split profiles, and governance authorities.
    pub fn initialize_merchant_config(
        ctx: Context<InitializeMerchantConfig>,
        merchant_bps: u64,
        tax_bps: u64,
        platform_bps: u64,
    ) -> Result<()> {
        require!(
            merchant_bps
                .checked_add(tax_bps)
                .and_then(|sum| sum.checked_add(platform_bps))
                == Some(BASIS_POINTS_DIVISOR),
            MozaikPayError::InvalidSplitBasisPoints
        );

        let config = &mut ctx.accounts.merchant_config;
        config.admin = ctx.accounts.admin.key();
        config.merchant_vault = ctx.accounts.merchant_vault.key();
        config.tax_vault = ctx.accounts.tax_vault.key();
        config.platform_vault = ctx.accounts.platform_vault.key();
        config.settlement_mint = ctx.accounts.settlement_mint.key();
        config.merchant_bps = merchant_bps;
        config.tax_bps = tax_bps;
        config.platform_bps = platform_bps;
        config.bump = ctx.bumps.merchant_config;

        emit!(MerchantConfigInitializedEvent {
            admin: config.admin,
            settlement_mint: config.settlement_mint,
            merchant_bps,
            tax_bps,
            platform_bps,
            timestamp: Clock::get()?.unix_timestamp,
        });

        Ok(())
    }

    /// Creates a deterministic, auditable invoice PDA tied to an enterprise order ID.
    pub fn create_invoice(
        ctx: Context<CreateInvoice>,
        invoice_id: String,
        amount: u64,
        validity_duration_seconds: i64,
    ) -> Result<()> {
        require!(
            !invoice_id.is_empty() && invoice_id.len() <= MAX_INVOICE_ID_LEN,
            MozaikPayError::InvalidInvoiceId
        );
        require!(amount > 0, MozaikPayError::ZeroAmountDisallowed);
        require!(
            validity_duration_seconds >= 60 && validity_duration_seconds <= 604_800,
            MozaikPayError::InvalidValidityDuration
        );

        let clock = Clock::get()?;
        let expires_at = clock
            .unix_timestamp
            .checked_add(validity_duration_seconds)
            .ok_or(MozaikPayError::ArithmeticOverflow)?;

        let invoice = &mut ctx.accounts.invoice;
        invoice.merchant_config = ctx.accounts.merchant_config.key();
        invoice.invoice_id = invoice_id.clone();
        invoice.amount = amount;
        invoice.amount_paid = 0;
        invoice.payer = Pubkey::default();
        invoice.created_at = clock.unix_timestamp;
        invoice.expires_at = expires_at;
        invoice.status = InvoiceStatus::Pending;
        invoice.bump = ctx.bumps.invoice;

        emit!(InvoiceCreatedEvent {
            invoice_pda: invoice.key(),
            merchant_config: ctx.accounts.merchant_config.key(),
            invoice_id,
            amount,
            expires_at,
        });

        Ok(())
    }

    /// Executes the payment and performs real-time atomic splits across Merchant, Tax, and Platform accounts.
    pub fn settle_invoice(ctx: Context<SettleInvoice>) -> Result<()> {
        let clock = Clock::get()?;
        let invoice = &mut ctx.accounts.invoice;
        let config = &ctx.accounts.merchant_config;

        require!(
            invoice.status == InvoiceStatus::Pending,
            MozaikPayError::InvoiceAlreadySettledOrCancelled
        );
        require!(
            clock.unix_timestamp <= invoice.expires_at,
            MozaikPayError::InvoiceExpired
        );

        let total_amount = invoice.amount;

        // Compute split fractions with checked arithmetic
        let platform_amount = total_amount
            .checked_mul(config.platform_bps)
            .ok_or(MozaikPayError::ArithmeticOverflow)?
            .checked_div(BASIS_POINTS_DIVISOR)
            .ok_or(MozaikPayError::ArithmeticOverflow)?;

        let tax_amount = total_amount
            .checked_mul(config.tax_bps)
            .ok_or(MozaikPayError::ArithmeticOverflow)?
            .checked_div(BASIS_POINTS_DIVISOR)
            .ok_or(MozaikPayError::ArithmeticOverflow)?;

        let merchant_amount = total_amount
            .checked_sub(platform_amount)
            .ok_or(MozaikPayError::ArithmeticOverflow)?
            .checked_sub(tax_amount)
            .ok_or(MozaikPayError::ArithmeticOverflow)?;

        // 1. Transfer to Platform Fee Vault
        if platform_amount > 0 {
            token::transfer(
                CpiContext::new(
                    ctx.accounts.token_program.to_account_info(),
                    Transfer {
                        from: ctx.accounts.payer_token_account.to_account_info(),
                        to: ctx.accounts.platform_vault.to_account_info(),
                        authority: ctx.accounts.payer.to_account_info(),
                    },
                ),
                platform_amount,
            )?;
        }

        // 2. Transfer to Tax Escrow Vault
        if tax_amount > 0 {
            token::transfer(
                CpiContext::new(
                    ctx.accounts.token_program.to_account_info(),
                    Transfer {
                        from: ctx.accounts.payer_token_account.to_account_info(),
                        to: ctx.accounts.tax_vault.to_account_info(),
                        authority: ctx.accounts.payer.to_account_info(),
                    },
                ),
                tax_amount,
            )?;
        }

        // 3. Transfer remaining to Merchant Primary Vault
        if merchant_amount > 0 {
            token::transfer(
                CpiContext::new(
                    ctx.accounts.token_program.to_account_info(),
                    Transfer {
                        from: ctx.accounts.payer_token_account.to_account_info(),
                        to: ctx.accounts.merchant_vault.to_account_info(),
                        authority: ctx.accounts.payer.to_account_info(),
                    },
                ),
                merchant_amount,
            )?;
        }

        invoice.amount_paid = total_amount;
        invoice.payer = ctx.accounts.payer.key();
        invoice.status = InvoiceStatus::Settled;

        emit!(InvoiceSettledEvent {
            invoice_pda: invoice.key(),
            invoice_id: invoice.invoice_id.clone(),
            payer: ctx.accounts.payer.key(),
            total_amount,
            merchant_amount,
            tax_amount,
            platform_amount,
            settled_at: clock.unix_timestamp,
        });

        Ok(())
    }

    /// Emergency/Authorized cancellation of an unsettled invoice by the Merchant Admin.
    pub fn cancel_invoice(ctx: Context<CancelInvoice>) -> Result<()> {
        let invoice = &mut ctx.accounts.invoice;
        require!(
            invoice.status == InvoiceStatus::Pending,
            MozaikPayError::InvoiceAlreadySettledOrCancelled
        );

        invoice.status = InvoiceStatus::Cancelled;

        emit!(InvoiceCancelledEvent {
            invoice_pda: invoice.key(),
            invoice_id: invoice.invoice_id.clone(),
            cancelled_at: Clock::get()?.unix_timestamp,
        });

        Ok(())
    }
}

// --------------------------------------------------------------------------------
// Account Structures
// --------------------------------------------------------------------------------

#[derive(Accounts)]
pub struct InitializeMerchantConfig<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    #[account(
        init,
        payer = admin,
        space = 8 + MerchantConfig::INIT_SPACE,
        seeds = [b"merchant_config", admin.key().as_ref()],
        bump
    )]
    pub merchant_config: Account<'info, MerchantConfig>,

    pub settlement_mint: Account<'info, Mint>,

    #[account(
        constraint = merchant_vault.mint == settlement_mint.key() @ MozaikPayError::InvalidMintMismatch
    )]
    pub merchant_vault: Account<'info, TokenAccount>,

    #[account(
        constraint = tax_vault.mint == settlement_mint.key() @ MozaikPayError::InvalidMintMismatch
    )]
    pub tax_vault: Account<'info, TokenAccount>,

    #[account(
        constraint = platform_vault.mint == settlement_mint.key() @ MozaikPayError::InvalidMintMismatch
    )]
    pub platform_vault: Account<'info, TokenAccount>,

    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
#[instruction(invoice_id: String)]
pub struct CreateInvoice<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    #[account(
        has_one = admin @ MozaikPayError::UnauthorizedMerchantAdmin,
        seeds = [b"merchant_config", admin.key().as_ref()],
        bump = merchant_config.bump
    )]
    pub merchant_config: Account<'info, MerchantConfig>,

    #[account(
        init,
        payer = admin,
        space = 8 + Invoice::compute_space(&invoice_id),
        seeds = [b"invoice", merchant_config.key().as_ref(), invoice_id.as_bytes()],
        bump
    )]
    pub invoice: Account<'info, Invoice>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct SettleInvoice<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(
        mut,
        seeds = [
            b"merchant_config", 
            merchant_config.admin.as_ref()
        ],
        bump = merchant_config.bump
    )]
    pub merchant_config: Account<'info, MerchantConfig>,

    #[account(
        mut,
        seeds = [
            b"invoice",
            merchant_config.key().as_ref(),
            invoice.invoice_id.as_bytes()
        ],
        bump = invoice.bump,
        has_one = merchant_config @ MozaikPayError::ConfigInvoiceMismatch
    )]
    pub invoice: Account<'info, Invoice>,

    #[account(
        mut,
        constraint = payer_token_account.owner == payer.key() @ MozaikPayError::UnauthorizedTokenAccount,
        constraint = payer_token_account.mint == merchant_config.settlement_mint @ MozaikPayError::InvalidMintMismatch
    )]
    pub payer_token_account: Account<'info, TokenAccount>,

    #[account(
        mut,
        address = merchant_config.merchant_vault @ MozaikPayError::VaultMismatch
    )]
    pub merchant_vault: Account<'info, TokenAccount>,

    #[account(
        mut,
        address = merchant_config.tax_vault @ MozaikPayError::VaultMismatch
    )]
    pub tax_vault: Account<'info, TokenAccount>,

    #[account(
        mut,
        address = merchant_config.platform_vault @ MozaikPayError::VaultMismatch
    )]
    pub platform_vault: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct CancelInvoice<'info> {
    pub admin: Signer<'info>,

    #[account(
        has_one = admin @ MozaikPayError::UnauthorizedMerchantAdmin,
        seeds = [b"merchant_config", admin.key().as_ref()],
        bump = merchant_config.bump
    )]
    pub merchant_config: Account<'info, MerchantConfig>,

    #[account(
        mut,
        seeds = [
            b"invoice",
            merchant_config.key().as_ref(),
            invoice.invoice_id.as_bytes()
        ],
        bump = invoice.bump,
        has_one = merchant_config @ MozaikPayError::ConfigInvoiceMismatch
    )]
    pub invoice: Account<'info, Invoice>,
}

// --------------------------------------------------------------------------------
// State Objects
// --------------------------------------------------------------------------------

#[account]
#[derive(InitSpace)]
pub struct MerchantConfig {
    pub admin: Pubkey,
    pub merchant_vault: Pubkey,
    pub tax_vault: Pubkey,
    pub platform_vault: Pubkey,
    pub settlement_mint: Pubkey,
    pub merchant_bps: u64,
    pub tax_bps: u64,
    pub platform_bps: u64,
    pub bump: u8,
}

#[account]
pub struct Invoice {
    pub merchant_config: Pubkey,
    pub invoice_id: String,
    pub amount: u64,
    pub amount_paid: u64,
    pub payer: Pubkey,
    pub created_at: i64,
    pub expires_at: i64,
    pub status: InvoiceStatus,
    pub bump: u8,
}

impl Invoice {
    pub fn compute_space(invoice_id: &str) -> usize {
        32 +                      // merchant_config Pubkey
        (4 + invoice_id.len()) +  // String prefix + dynamic UTF-8 bytes
        8 +                       // amount (u64)
        8 +                       // amount_paid (u64)
        32 +                      // payer Pubkey
        8 +                       // created_at (i64)
        8 +                       // expires_at (i64)
        1 +                       // status Enum (1 byte)
        1                         // bump (u8)
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, InitSpace)]
pub enum InvoiceStatus {
    Pending,
    Settled,
    Cancelled,
}

// --------------------------------------------------------------------------------
// Events
// --------------------------------------------------------------------------------

#[event]
pub struct MerchantConfigInitializedEvent {
    pub admin: Pubkey,
    pub settlement_mint: Pubkey,
    pub merchant_bps: u64,
    pub tax_bps: u64,
    pub platform_bps: u64,
    pub timestamp: i64,
}

#[event]
pub struct InvoiceCreatedEvent {
    pub invoice_pda: Pubkey,
    pub merchant_config: Pubkey,
    pub invoice_id: String,
    pub amount: u64,
    pub expires_at: i64,
}

#[event]
pub struct InvoiceSettledEvent {
    pub invoice_pda: Pubkey,
    pub invoice_id: String,
    pub payer: Pubkey,
    pub total_amount: u64,
    pub merchant_amount: u64,
    pub tax_amount: u64,
    pub platform_amount: u64,
    pub settled_at: i64,
}

#[event]
pub struct InvoiceCancelledEvent {
    pub invoice_pda: Pubkey,
    pub invoice_id: String,
    pub cancelled_at: i64,
}

// --------------------------------------------------------------------------------
// Error Codes
// --------------------------------------------------------------------------------

#[error_code]
pub enum MozaikPayError {
    #[msg("Total split basis points must strictly equal 10,000 (100.00%).")]
    InvalidSplitBasisPoints,
    #[msg("Invoice identifier must be non-empty and less than or equal to 32 characters.")]
    InvalidInvoiceId,
    #[msg("Invoice amount must be strictly greater than zero.")]
    ZeroAmountDisallowed,
    #[msg("Validity duration must be between 60 seconds and 604,800 seconds (7 days).")]
    InvalidValidityDuration,
    #[msg("Arithmetic overflow detected during split calculation.")]
    ArithmeticOverflow,
    #[msg("Token account mint does not match the configured settlement mint.")]
    InvalidMintMismatch,
    #[msg("Signer is not authorized as merchant administrator.")]
    UnauthorizedMerchantAdmin,
    #[msg("Token account authority does not match payer signing authority.")]
    UnauthorizedTokenAccount,
    #[msg("Target vault does not match the approved merchant configuration.")]
    VaultMismatch,
    #[msg("Invoice does not belong to the specified merchant configuration.")]
    ConfigInvoiceMismatch,
    #[msg("Invoice is not in pending status; already settled or cancelled.")]
    InvoiceAlreadySettledOrCancelled,
    #[msg("Payment timestamp has exceeded the invoice expiration deadline.")]
    InvoiceExpired,
}