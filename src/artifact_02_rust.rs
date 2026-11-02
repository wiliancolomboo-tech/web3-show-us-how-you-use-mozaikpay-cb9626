// programs/mozaikflow-settlement/src/lib.rs
use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, TransferChecked};

declare_id!("MoZaiKPAY1111111111111111111111111111111111");

#[program]
pub mod mozaikflow_settlement {
    use super::*;

    /// Initialize a merchant configuration and billing terms
    pub fn initialize_merchant(
        ctx: Context<InitializeMerchant>,
        merchant_name: String,
        basis_points_tax: u16,
    ) -> Result<()> {
        require!(merchant_name.len() <= 64, MozaikError::StringTooLong);
        require!(basis_points_tax <= 3000, MozaikError::InvalidTaxBasisPoints); // Max 30%

        let config = &mut ctx.accounts.merchant_config;
        config.merchant_authority = ctx.accounts.authority.key();
        config.settlement_vault = ctx.accounts.settlement_vault.key();
        config.tax_vault = ctx.accounts.tax_vault.key();
        config.accepted_mint = ctx.accounts.accepted_mint.key();
        config.basis_points_tax = basis_points_tax;
        config.merchant_name = merchant_name;
        config.bump = ctx.bumps.merchant_config;

        emit!(MerchantConfiguredEvent {
            merchant: ctx.accounts.authority.key(),
            accepted_mint: ctx.accounts.accepted_mint.key(),
            tax_rate_bps: basis_points_tax,
        });

        Ok(())
    }

    /// Create an on-chain MozaikPay-compatible Invoice PDA
    pub fn create_invoice(
        ctx: Context<CreateInvoice>,
        invoice_id: String,
        amount_required: u64,
        expiry_timestamp: i64,
        reference_key: Pubkey,
    ) -> Result<()> {
        require!(invoice_id.len() <= 32, MozaikError::StringTooLong);
        require!(amount_required > 0, MozaikError::ZeroPaymentAmount);
        
        let clock = Clock::get()?;
        require!(
            expiry_timestamp > clock.unix_timestamp,
            MozaikError::InvalidExpiryTimestamp
        );

        let invoice = &mut ctx.accounts.invoice;
        invoice.merchant = ctx.accounts.merchant_config.key();
        invoice.invoice_id = invoice_id.clone();
        invoice.amount_required = amount_required;
        invoice.amount_paid = 0;
        invoice.expiry_timestamp = expiry_timestamp;
        invoice.reference_key = reference_key;
        invoice.status = InvoiceStatus::Pending;
        invoice.bump = ctx.bumps.invoice;

        emit!(InvoiceCreatedEvent {
            invoice_id,
            merchant: ctx.accounts.merchant_config.key(),
            amount: amount_required,
            reference: reference_key,
        });

        Ok(())
    }

    /// Settle the invoice atomically splitting principal to merchant and tax to ops vault
    pub fn settle_invoice(ctx: Context<SettleInvoice>) -> Result<()> {
        let invoice = &mut ctx.accounts.invoice;
        let config = &ctx.accounts.merchant_config;
        let clock = Clock::get()?;

        require!(invoice.status == InvoiceStatus::Pending, MozaikError::InvoiceNotPending);
        require!(clock.unix_timestamp <= invoice.expiry_timestamp, MozaikError::InvoiceExpired);

        let total_amount = invoice.amount_required;
        let tax_amount = (total_amount as u128 * config.basis_points_tax as u128 / 10_000) as u64;
        let merchant_amount = total_amount.checked_sub(tax_amount).ok_or(MozaikError::MathUnderflow)?;

        // 1. Transfer merchant portion
        let cpi_accounts_merchant = TransferChecked {
            from: ctx.accounts.payer_token_account.to_account_info(),
            mint: ctx.accounts.token_mint.to_account_info(),
            to: ctx.accounts.merchant_settlement_vault.to_account_info(),
            authority: ctx.accounts.payer.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx_merchant = CpiContext::new(cpi_program.clone(), cpi_accounts_merchant);
        token::transfer_checked(cpi_ctx_merchant, merchant_amount, ctx.accounts.token_mint.decimals)?;

        // 2. Transfer tax/reserve portion if configured
        if tax_amount > 0 {
            let cpi_accounts_tax = TransferChecked {
                from: ctx.accounts.payer_token_account.to_account_info(),
                mint: ctx.accounts.token_mint.to_account_info(),
                to: ctx.accounts.tax_vault.to_account_info(),
                authority: ctx.accounts.payer.to_account_info(),
            };
            let cpi_ctx_tax = CpiContext::new(cpi_program, cpi_accounts_tax);
            token::transfer_checked(cpi_ctx_tax, tax_amount, ctx.accounts.token_mint.decimals)?;
        }

        // 3. Mutate invoice state
        invoice.amount_paid = total_amount;
        invoice.status = InvoiceStatus::Settled;

        emit!(InvoiceSettledEvent {
            invoice_id: invoice.invoice_id.clone(),
            payer: ctx.accounts.payer.key(),
            merchant_payout: merchant_amount,
            tax_payout: tax_amount,
            settled_at: clock.unix_timestamp,
        });

        Ok(())
    }
}

// -----------------------------------------------------------------------------
// Account Contexts
// -----------------------------------------------------------------------------

#[derive(Accounts)]
#[instruction(merchant_name: String, basis_points_tax: u16)]
pub struct InitializeMerchant<'info> {
    #[account(
        init,
        payer = authority,
        space = 8 + 32 + 32 + 32 + 32 + 2 + (4 + 64) + 1,
        seeds = [b"merchant", authority.key().as_ref()],
        bump
    )]
    pub merchant_config: Account<'info, MerchantConfig>,
    
    pub accepted_mint: Account<'info, Mint>,

    #[account(
        constraint = settlement_vault.mint == accepted_mint.key(),
        constraint = settlement_vault.owner == authority.key()
    )]
    pub settlement_vault: Account<'info, TokenAccount>,

    #[account(
        constraint = tax_vault.mint == accepted_mint.key()
    )]
    pub tax_vault: Account<'info, TokenAccount>,

    #[account(mut)]
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(invoice_id: String)]
pub struct CreateInvoice<'info> {
    #[account(
        mut,
        has_one = merchant_authority @ MozaikError::UnauthorizedMerchant
    )]
    pub merchant_config: Account<'info, MerchantConfig>,
    pub merchant_authority: Signer<'info>,

    #[account(
        init,
        payer = merchant_authority,
        space = 8 + 32 + (4 + 32) + 8 + 8 + 8 + 32 + 1 + 1,
        seeds = [b"invoice", merchant_config.key().as_ref(), invoice_id.as_bytes()],
        bump
    )]
    pub invoice: Account<'info, InvoiceAccount>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct SettleInvoice<'info> {
    #[account(
        mut,
        seeds = [b"invoice", invoice.merchant.as_ref(), invoice.invoice_id.as_bytes()],
        bump = invoice.bump
    )]
    pub invoice: Account<'info, InvoiceAccount>,

    #[account(
        seeds = [b"merchant", merchant_config.merchant_authority.as_ref()],
        bump = merchant_config.bump,
        constraint = invoice.merchant == merchant_config.key()
    )]
    pub merchant_config: Account<'info, MerchantConfig>,

    pub token_mint: Account<'info, Mint>,

    #[account(
        mut,
        constraint = payer_token_account.mint == token_mint.key(),
        constraint = payer_token_account.owner == payer.key()
    )]
    pub payer_token_account: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = merchant_settlement_vault.key() == merchant_config.settlement_vault
    )]
    pub merchant_settlement_vault: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = tax_vault.key() == merchant_config.tax_vault
    )]
    pub tax_vault: Account<'info, TokenAccount>,

    pub payer: Signer<'info>,
    pub token_program: Program<'info, Token>,
}

// -----------------------------------------------------------------------------
// State Definitions & Error Codes
// -----------------------------------------------------------------------------

#[account]
pub struct MerchantConfig {
    pub merchant_authority: Pubkey,
    pub accepted_mint: Pubkey,
    pub settlement_vault: Pubkey,
    pub tax_vault: Pubkey,
    pub basis_points_tax: u16,
    pub merchant_name: String,
    pub bump: u8,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq, Eq)]
pub enum InvoiceStatus {
    Pending,
    Settled,
    Expired,
    Cancelled,
}

#[account]
pub struct InvoiceAccount {
    pub merchant: Pubkey,
    pub invoice_id: String,
    pub amount_required: u64,
    pub amount_paid: u64,
    pub expiry_timestamp: i64,
    pub reference_key: Pubkey,
    pub status: InvoiceStatus,
    pub bump: u8,
}

#[error_code]
pub enum MozaikError {
    #[msg("Input string exceeds designated byte capacity")]
    StringTooLong,
    #[msg("Tax fee basis points cannot exceed 3000 (30%)")]
    InvalidTaxBasisPoints,
    #[msg("Invoice payment amount must be greater than zero")]
    ZeroPaymentAmount,
    #[msg("Expiry timestamp must be positioned in the future")]
    InvalidExpiryTimestamp,
    #[msg("Signer does not maintain authority over this merchant config")]
    UnauthorizedMerchant,
    #[msg("Invoice is not in pending state")]
    InvoiceNotPending,
    #[msg("Invoice has expired past its valid deadline")]
    InvoiceExpired,
    #[msg("Arithmetic underflow during fee split calculation")]
    MathUnderflow,
}

#[event]
pub struct MerchantConfiguredEvent {
    pub merchant: Pubkey,
    pub accepted_mint: Pubkey,
    pub tax_rate_bps: u16,
}

#[event]
pub struct InvoiceCreatedEvent {
    pub invoice_id: String,
    pub merchant: Pubkey,
    pub amount: u64,
    pub reference: Pubkey,
}

#[event]
pub struct InvoiceSettledEvent {
    pub invoice_id: String,
    pub payer: Pubkey,
    pub merchant_payout: u64,
    pub tax_payout: u64,
    pub settled_at: i64,
}