use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};

declare_id!("MoZkPay111111111111111111111111111111111111");

pub const BASIS_POINTS_DIVISOR: u64 = 10_000;
pub const MAX_REVENUE_RECIPIENTS: usize = 5;

#[program]
pub mod mozaik_flow {
    use super::*;

    /// Initialize a merchant's payment and split configuration.
    pub fn initialize_merchant_config(
        ctx: Context<InitializeMerchantConfig>,
        merchant_id: String,
        platform_fee_bps: u16,
        tax_reserve_bps: u16,
    ) -> Result<()> {
        require!(merchant_id.len() <= 32, MozaikPayError::InvalidMerchantId);
        require!(
            (platform_fee_bps + tax_reserve_bps) < BASIS_POINTS_DIVISOR as u16,
            MozaikPayError::ExcessiveFeeSplit
        );

        let config = &mut ctx.accounts.merchant_config;
        config.authority = ctx.accounts.authority.key();
        config.merchant_id = merchant_id;
        config.platform_fee_bps = platform_fee_bps;
        config.tax_reserve_bps = tax_reserve_bps;
        config.platform_vault = ctx.accounts.platform_vault.key();
        config.tax_vault = ctx.accounts.tax_vault.key();
        config.merchant_vault = ctx.accounts.merchant_vault.key();
        config.is_paused = false;
        config.bump = ctx.bumps.merchant_config;

        msg!(
            "Merchant config initialized for: {} with platform fee: {} bps",
            config.merchant_id,
            platform_fee_bps
        );
        Ok(())
    }

    /// Process atomic multi-party revenue split for an incoming merchant order.
    pub fn process_split_payment(
        ctx: Context<ProcessSplitPayment>,
        order_id: [u8; 32],
        gross_amount: u64,
    ) -> Result<()> {
        let config = &ctx.accounts.merchant_config;
        require!(!config.is_paused, MozaikPayError::ContractPaused);
        require!(gross_amount > 0, MozaikPayError::ZeroGrossAmount);

        // Calculate deterministic revenue shares
        let platform_share = (gross_amount as u128)
            .checked_mul(config.platform_fee_bps as u128)
            .ok_or(MozaikPayError::MathOverflow)?
            .checked_div(BASIS_POINTS_DIVISOR as u128)
            .ok_or(MozaikPayError::MathOverflow)? as u64;

        let tax_share = (gross_amount as u128)
            .checked_mul(config.tax_reserve_bps as u128)
            .ok_or(MozaikPayError::MathOverflow)?
            .checked_div(BASIS_POINTS_DIVISOR as u128)
            .ok_or(MozaikPayError::MathOverflow)? as u64;

        let merchant_share = gross_amount
            .checked_sub(platform_share)
            .ok_or(MozaikPayError::MathOverflow)?
            .checked_sub(tax_share)
            .ok_or(MozaikPayError::MathOverflow)?;

        // Execute 1: Transfer Platform Share
        if platform_share > 0 {
            token::transfer(
                CpiContext::new(
                    ctx.accounts.token_program.to_account_info(),
                    Transfer {
                        from: ctx.accounts.customer_token_account.to_account_info(),
                        to: ctx.accounts.platform_vault.to_account_info(),
                        authority: ctx.accounts.payer.to_account_info(),
                    },
                ),
                platform_share,
            )?;
        }

        // Execute 2: Transfer Tax / Reserve Share
        if tax_share > 0 {
            token::transfer(
                CpiContext::new(
                    ctx.accounts.token_program.to_account_info(),
                    Transfer {
                        from: ctx.accounts.customer_token_account.to_account_info(),
                        to: ctx.accounts.tax_vault.to_account_info(),
                        authority: ctx.accounts.payer.to_account_info(),
                    },
                ),
                tax_share,
            )?;
        }

        // Execute 3: Transfer Remaining Merchant Share
        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.customer_token_account.to_account_info(),
                    to: ctx.accounts.merchant_vault.to_account_info(),
                    authority: ctx.accounts.payer.to_account_info(),
                },
            ),
            merchant_share,
        )?;

        // Record receipt order marker to prevent replay
        let receipt = &mut ctx.accounts.payment_receipt;
        receipt.order_id = order_id;
        receipt.payer = ctx.accounts.payer.key();
        receipt.token_mint = ctx.accounts.mint.key();
        receipt.gross_amount = gross_amount;
        receipt.merchant_amount = merchant_share;
        receipt.platform_fee = platform_share;
        receipt.tax_amount = tax_share;
        receipt.timestamp = Clock::get()?.unix_timestamp;
        receipt.bump = ctx.bumps.payment_receipt;

        emit!(PaymentSettledEvent {
            order_id,
            payer: ctx.accounts.payer.key(),
            merchant_config: config.key(),
            gross_amount,
            merchant_amount: merchant_share,
            platform_fee: platform_share,
            tax_amount: tax_share,
            timestamp: receipt.timestamp,
        });

        Ok(())
    }
}

// -----------------------------------------------------------------------------
// Accounts
// -----------------------------------------------------------------------------

#[derive(Accounts)]
#[instruction(merchant_id: String)]
pub struct InitializeMerchantConfig<'info> {
    #[account(
        init,
        payer = authority,
        space = 8 + MerchantConfig::INIT_SPACE,
        seeds = [b"merchant_config", authority.key().as_ref(), merchant_id.as_bytes()],
        bump
    )]
    pub merchant_config: Account<'info, MerchantConfig>,
    #[account(mut)]
    pub authority: Signer<'info>,
    /// CHECK: Validated as recipient token account in split processing
    pub platform_vault: AccountInfo<'info>,
    /// CHECK: Validated as recipient token account in split processing
    pub tax_vault: AccountInfo<'info>,
    /// CHECK: Validated as recipient token account in split processing
    pub merchant_vault: AccountInfo<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(order_id: [u8; 32])]
pub struct ProcessSplitPayment<'info> {
    #[account(
        mut,
        seeds = [b"merchant_config", merchant_config.authority.as_ref(), merchant_config.merchant_id.as_bytes()],
        bump = merchant_config.bump
    )]
    pub merchant_config: Account<'info, MerchantConfig>,

    #[account(
        init,
        payer = payer,
        space = 8 + PaymentReceipt::INIT_SPACE,
        seeds = [b"payment_receipt", merchant_config.key().as_ref(), order_id.as_ref()],
        bump
    )]
    pub payment_receipt: Account<'info, PaymentReceipt>,

    #[account(mut)]
    pub payer: Signer<'info>,
    pub mint: Account<'info, Mint>,

    #[account(
        mut,
        constraint = customer_token_account.mint == mint.key() @ MozaikPayError::InvalidTokenMint,
        constraint = customer_token_account.owner == payer.key() @ MozaikPayError::InvalidTokenOwner
    )]
    pub customer_token_account: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = merchant_vault.key() == merchant_config.merchant_vault @ MozaikPayError::InvalidVaultRecipient
    )]
    pub merchant_vault: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = platform_vault.key() == merchant_config.platform_vault @ MozaikPayError::InvalidVaultRecipient
    )]
    pub platform_vault: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = tax_vault.key() == merchant_config.tax_vault @ MozaikPayError::InvalidVaultRecipient
    )]
    pub tax_vault: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

// -----------------------------------------------------------------------------
// State Structures
// -----------------------------------------------------------------------------

#[account]
#[derive(InitSpace)]
pub struct MerchantConfig {
    pub authority: Pubkey,
    #[max_len(32)]
    pub merchant_id: String,
    pub platform_fee_bps: u16,
    pub tax_reserve_bps: u16,
    pub platform_vault: Pubkey,
    pub tax_vault: Pubkey,
    pub merchant_vault: Pubkey,
    pub is_paused: bool,
    pub bump: u8,
}

#[account]
#[derive(InitSpace)]
pub struct PaymentReceipt {
    pub order_id: [u8; 32],
    pub payer: Pubkey,
    pub token_mint: Pubkey,
    pub gross_amount: u64,
    pub merchant_amount: u64,
    pub platform_fee: u64,
    pub tax_amount: u64,
    pub timestamp: i64,
    pub bump: u8,
}

// -----------------------------------------------------------------------------
// Events & Errors
// -----------------------------------------------------------------------------

#[event]
pub struct PaymentSettledEvent {
    pub order_id: [u8; 32],
    pub payer: Pubkey,
    pub merchant_config: Pubkey,
    pub gross_amount: u64,
    pub merchant_amount: u64,
    pub platform_fee: u64,
    pub tax_amount: u64,
    pub timestamp: i64,
}

#[error_code]
pub enum MozaikPayError {
    #[msg("Merchant ID length must be between 1 and 32 characters.")]
    InvalidMerchantId,
    #[msg("Total fee splits exceed 100% (10,000 BPS).")]
    ExcessiveFeeSplit,
    #[msg("Contract operations are paused by authority.")]
    ContractPaused,
    #[msg("Gross amount must be strictly greater than zero.")]
    ZeroGrossAmount,
    #[msg("Arithmetic overflow detected during split calculation.")]
    MathOverflow,
    #[msg("Specified token account does not match expected mint.")]
    InvalidTokenMint,
    #[msg("Customer token account is not owned by the payer signer.")]
    InvalidTokenOwner,
    #[msg("Designated vault does not match the merchant configuration.")]
    InvalidVaultRecipient,
}