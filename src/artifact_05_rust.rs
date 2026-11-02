use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};

declare_id!("MzkF1owEnterpriseGateway1111111111111111111");

pub const BPS_DENOMINATOR: u64 = 10_000;
pub const TREASURY_BPS: u64 = 8_000;   // 80.00%
pub const RESERVE_BPS: u64 = 1_500;    // 15.00%
pub const AFFILIATE_BPS: u64 = 500;    //  5.00%

#[program]
pub mod mozaik_flow {
    use super::*;

    /// Initializes a merchant's payment split configuration
    pub fn initialize_merchant_config(
        ctx: Context<InitializeMerchantConfig>,
        merchant_id: String,
    ) -> Result<()> {
        require!(merchant_id.len() <= 32, MozaikFlowError::MerchantIdTooLong);
        
        let config = &mut ctx.accounts.merchant_config;
        config.authority = ctx.accounts.authority.key();
        config.treasury_token_account = ctx.accounts.treasury_token_account.key();
        config.reserve_token_account = ctx.accounts.reserve_token_account.key();
        config.payment_mint = ctx.accounts.payment_mint.key();
        config.merchant_id = merchant_id;
        config.total_processed_volume = 0;
        config.bump = ctx.bumps.merchant_config;

        emit!(MerchantConfigInitialized {
            merchant: config.authority,
            merchant_id: config.merchant_id.clone(),
            payment_mint: config.payment_mint,
        });

        Ok(())
    }

    /// Process a payment originating from MozaikPay checkout with automated 3-way split
    pub fn process_mozaik_payment(
        ctx: Context<ProcessMozaikPayment>,
        order_id: String,
        amount: u64,
        allocated_credits: u64,
    ) -> Result<()> {
        require!(amount > 0, MozaikFlowError::ZeroAmountPayment);
        require!(order_id.len() <= 64, MozaikFlowError::OrderIdTooLong);

        // Calculate deterministic revenue splits
        let treasury_amount = amount
            .checked_mul(TREASURY_BPS)
            .ok_or(MozaikFlowError::MathOverflow)?
            .checked_div(BPS_DENOMINATOR)
            .ok_or(MozaikFlowError::MathOverflow)?;

        let reserve_amount = amount
            .checked_mul(RESERVE_BPS)
            .ok_or(MozaikFlowError::MathOverflow)?
            .checked_div(BPS_DENOMINATOR)
            .ok_or(MozaikFlowError::MathOverflow)?;

        let affiliate_amount = amount
            .checked_sub(treasury_amount)
            .ok_or(MozaikFlowError::MathOverflow)?
            .checked_sub(reserve_amount)
            .ok_or(MozaikFlowError::MathOverflow)?;

        // Execute CPI: Customer -> Treasury (80%)
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_accounts_treasury = Transfer {
            from: ctx.accounts.customer_token_account.to_account_info(),
            to: ctx.accounts.treasury_token_account.to_account_info(),
            authority: ctx.accounts.customer_authority.to_account_info(),
        };
        token::transfer(
            CpiContext::new(cpi_program.clone(), cpi_accounts_treasury),
            treasury_amount,
        )?;

        // Execute CPI: Customer -> Reserve (15%)
        let cpi_accounts_reserve = Transfer {
            from: ctx.accounts.customer_token_account.to_account_info(),
            to: ctx.accounts.reserve_token_account.to_account_info(),
            authority: ctx.accounts.customer_authority.to_account_info(),
        };
        token::transfer(
            CpiContext::new(cpi_program.clone(), cpi_accounts_reserve),
            reserve_amount,
        )?;

        // Execute CPI: Customer -> Affiliate (5%)
        let cpi_accounts_affiliate = Transfer {
            from: ctx.accounts.customer_token_account.to_account_info(),
            to: ctx.accounts.affiliate_token_account.to_account_info(),
            authority: ctx.accounts.customer_authority.to_account_info(),
        };
        token::transfer(
            CpiContext::new(cpi_program, cpi_accounts_affiliate),
            affiliate_amount,
        )?;

        // Record customer metering state on-chain
        let customer_account = &mut ctx.accounts.customer_usage_record;
        customer_account.customer_authority = ctx.accounts.customer_authority.key();
        customer_account.merchant = ctx.accounts.merchant_config.authority;
        customer_account.total_credits = customer_account
            .total_credits
            .checked_add(allocated_credits)
            .ok_or(MozaikFlowError::MathOverflow)?;
        customer_account.last_order_id = order_id.clone();
        customer_account.last_payment_timestamp = Clock::get()?.unix_timestamp;
        customer_account.bump = ctx.bumps.customer_usage_record;

        // Update Merchant Metrics
        let config = &mut ctx.accounts.merchant_config;
        config.total_processed_volume = config
            .total_processed_volume
            .checked_add(amount)
            .ok_or(MozaikFlowError::MathOverflow)?;

        emit!(MozaikPaymentSettled {
            order_id,
            customer: ctx.accounts.customer_authority.key(),
            merchant: config.authority,
            total_amount: amount,
            treasury_amount,
            reserve_amount,
            affiliate_amount,
            allocated_credits,
            timestamp: customer_account.last_payment_timestamp,
        });

        Ok(())
    }
}

// ----------------------------------------------------------------------------
// ACCOUNTS CONTEXTS
// ----------------------------------------------------------------------------

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

    pub payment_mint: Account<'info, Mint>,

    #[account(
        constraint = treasury_token_account.mint == payment_mint.key() @ MozaikFlowError::InvalidMint
    )]
    pub treasury_token_account: Account<'info, TokenAccount>,

    #[account(
        constraint = reserve_token_account.mint == payment_mint.key() @ MozaikFlowError::InvalidMint
    )]
    pub reserve_token_account: Account<'info, TokenAccount>,

    #[account(mut)]
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
#[instruction(order_id: String, amount: u64, allocated_credits: u64)]
pub struct ProcessMozaikPayment<'info> {
    #[account(
        mut,
        seeds = [
            b"merchant_config",
            merchant_config.authority.as_ref(),
            merchant_config.merchant_id.as_bytes()
        ],
        bump = merchant_config.bump
    )]
    pub merchant_config: Account<'info, MerchantConfig>,

    #[account(
        init_if_needed,
        payer = customer_authority,
        space = 8 + CustomerUsageRecord::INIT_SPACE,
        seeds = [
            b"customer_usage",
            merchant_config.key().as_ref(),
            customer_authority.key().as_ref()
        ],
        bump
    )]
    pub customer_usage_record: Account<'info, CustomerUsageRecord>,

    #[account(
        mut,
        constraint = customer_token_account.mint == merchant_config.payment_mint @ MozaikFlowError::InvalidMint,
        constraint = customer_token_account.owner == customer_authority.key() @ MozaikFlowError::InvalidTokenOwner
    )]
    pub customer_token_account: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = treasury_token_account.key() == merchant_config.treasury_token_account @ MozaikFlowError::UnauthorizedTreasury
    )]
    pub treasury_token_account: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = reserve_token_account.key() == merchant_config.reserve_token_account @ MozaikFlowError::UnauthorizedReserve
    )]
    pub reserve_token_account: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = affiliate_token_account.mint == merchant_config.payment_mint @ MozaikFlowError::InvalidMint
    )]
    pub affiliate_token_account: Account<'info, TokenAccount>,

    pub customer_authority: Signer<'info>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

// ----------------------------------------------------------------------------
// STATE SCHEMAS
// ----------------------------------------------------------------------------

#[account]
#[derive(InitSpace)]
pub struct MerchantConfig {
    pub authority: Pubkey,
    pub treasury_token_account: Pubkey,
    pub reserve_token_account: Pubkey,
    pub payment_mint: Pubkey,
    #[max_len(32)]
    pub merchant_id: String,
    pub total_processed_volume: u64,
    pub bump: u8,
}

#[account]
#[derive(InitSpace)]
pub struct CustomerUsageRecord {
    pub customer_authority: Pubkey,
    pub merchant: Pubkey,
    pub total_credits: u64,
    #[max_len(64)]
    pub last_order_id: String,
    pub last_payment_timestamp: i64,
    pub bump: u8,
}

// ----------------------------------------------------------------------------
// EVENTS & ERRORS
// ----------------------------------------------------------------------------

#[event]
pub struct MerchantConfigInitialized {
    pub merchant: Pubkey,
    pub merchant_id: String,
    pub payment_mint: Pubkey,
}

#[event]
pub struct MozaikPaymentSettled {
    pub order_id: String,
    pub customer: Pubkey,
    pub merchant: Pubkey,
    pub total_amount: u64,
    pub treasury_amount: u64,
    pub reserve_amount: u64,
    pub affiliate_amount: u64,
    pub allocated_credits: u64,
    pub timestamp: i64,
}

#[error_code]
pub enum MozaikFlowError {
    #[msg("Calculation overflow detected.")]
    MathOverflow,
    #[msg("Zero payment amounts are not permitted.")]
    ZeroAmountPayment,
    #[msg("Merchant ID length exceeds maximum of 32 bytes.")]
    MerchantIdTooLong,
    #[msg("Order ID length exceeds maximum of 64 bytes.")]
    OrderIdTooLong,
    #[msg("Supplied token mint does not match the merchant configuration.")]
    InvalidMint,
    #[msg("Supplied token account owner is invalid.")]
    InvalidTokenOwner,
    #[msg("Unauthorized destination treasury account.")]
    UnauthorizedTreasury,
    #[msg("Unauthorized destination reserve account.")]
    UnauthorizedReserve,
}