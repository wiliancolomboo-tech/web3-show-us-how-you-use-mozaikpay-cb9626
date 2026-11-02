use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};

declare_id!("MoZaiKPAYNexus11111111111111111111111111111");

#[program]
pub mod mozaikpay_nexus {
    use super::*;

    /// Initialize a merchant's configuration with deterministic payout routing rules.
    pub fn initialize_merchant(
        ctx: Context<InitializeMerchant>,
        merchant_id: String,
        affiliate_share_bps: u16,
        protocol_share_bps: u16,
    ) -> Result<()> {
        require!(merchant_id.len() <= 32, NexusError::MerchantIdTooLong);
        require!(
            affiliate_share_bps + protocol_share_bps < 10_000,
            NexusError::InvalidBpsConfiguration
        );

        let merchant = &mut ctx.accounts.merchant_account;
        merchant.authority = ctx.accounts.authority.key();
        merchant.merchant_id = merchant_id;
        merchant.treasury_vault = ctx.accounts.treasury_vault.key();
        merchant.affiliate_payout_account = ctx.accounts.affiliate_payout_account.key();
        merchant.protocol_fee_account = ctx.accounts.protocol_fee_account.key();
        merchant.payment_mint = ctx.accounts.payment_mint.key();
        merchant.affiliate_share_bps = affiliate_share_bps;
        merchant.protocol_share_bps = protocol_share_bps;
        merchant.total_volume = 0;
        merchant.total_orders = 0;
        merchant.is_active = true;
        merchant.bump = ctx.bumps.merchant_account;

        emit!(MerchantRegisteredEvent {
            merchant: merchant.key(),
            authority: merchant.authority,
            merchant_id: merchant.merchant_id.clone(),
            payment_mint: merchant.payment_mint,
            timestamp: Clock::get()?.unix_timestamp,
        });

        Ok(())
    }

    /// Process an immediate single-order purchase with atomic split settlement.
    pub fn execute_split_settlement(
        ctx: Context<ExecuteSplitSettlement>,
        order_id: String,
        amount: u64,
    ) -> Result<()> {
        require!(amount > 0, NexusError::InvalidAmount);
        require!(order_id.len() <= 36, NexusError::OrderIdTooLong);

        let merchant = &mut ctx.accounts.merchant_account;
        require!(merchant.is_active, NexusError::MerchantInactive);

        // Calculate split allocations (BPS denominator = 10,000)
        let affiliate_amount = (amount as u128)
            .checked_mul(merchant.affiliate_share_bps as u128)
            .ok_or(NexusError::MathOverflow)?
            .checked_div(10_000)
            .ok_or(NexusError::MathOverflow)? as u64;

        let protocol_amount = (amount as u128)
            .checked_mul(merchant.protocol_share_bps as u128)
            .ok_or(NexusError::MathOverflow)?
            .checked_div(10_000)
            .ok_or(NexusError::MathOverflow)? as u64;

        let merchant_amount = amount
            .checked_sub(affiliate_amount)
            .ok_or(NexusError::MathUnderflow)?
            .checked_sub(protocol_amount)
            .ok_or(NexusError::MathUnderflow)?;

        // 1. Transfer merchant portion
        token::transfer(
            ctx.accounts.transfer_to_merchant_context(),
            merchant_amount,
        )?;

        // 2. Transfer affiliate portion if > 0
        if affiliate_amount > 0 {
            token::transfer(
                ctx.accounts.transfer_to_affiliate_context(),
                affiliate_amount,
            )?;
        }

        // 3. Transfer protocol fee reserve if > 0
        if protocol_amount > 0 {
            token::transfer(
                ctx.accounts.transfer_to_protocol_context(),
                protocol_amount,
            )?;
        }

        merchant.total_volume = merchant
            .total_volume
            .checked_add(amount)
            .ok_or(NexusError::MathOverflow)?;
        merchant.total_orders = merchant
            .total_orders
            .checked_add(1)
            .ok_or(NexusError::MathOverflow)?;

        emit!(PaymentSettledEvent {
            merchant: merchant.key(),
            order_id,
            payer: ctx.accounts.payer.key(),
            total_amount: amount,
            merchant_amount,
            affiliate_amount,
            protocol_amount,
            timestamp: Clock::get()?.unix_timestamp,
        });

        Ok(())
    }

    /// Register a deterministic recurring subscription agreement.
    pub fn create_subscription(
        ctx: Context<CreateSubscription>,
        plan_id: String,
        recurring_amount: u64,
        frequency_seconds: i64,
    ) -> Result<()> {
        require!(plan_id.len() <= 32, NexusError::PlanIdTooLong);
        require!(recurring_amount > 0, NexusError::InvalidAmount);
        require!(frequency_seconds >= 60, NexusError::FrequencyTooShort);

        let sub = &mut ctx.accounts.subscription_agreement;
        let clock = Clock::get()?;

        sub.subscriber = ctx.accounts.subscriber.key();
        sub.merchant = ctx.accounts.merchant_account.key();
        sub.plan_id = plan_id;
        sub.recurring_amount = recurring_amount;
        sub.frequency_seconds = frequency_seconds;
        sub.last_billed_at = clock.unix_timestamp;
        sub.is_active = true;
        sub.bump = ctx.bumps.subscription_agreement;

        emit!(SubscriptionCreatedEvent {
            subscription: sub.key(),
            subscriber: sub.subscriber,
            merchant: sub.merchant,
            recurring_amount: sub.recurring_amount,
            frequency_seconds: sub.frequency_seconds,
            timestamp: clock.unix_timestamp,
        });

        Ok(())
    }

    /// Debit a scheduled subscription payment using delegated token vault allowance.
    pub fn execute_subscription_charge(
        ctx: Context<ExecuteSubscriptionCharge>,
    ) -> Result<()> {
        let clock = Clock::get()?;
        let sub = &mut ctx.accounts.subscription_agreement;
        let merchant = &mut ctx.accounts.merchant_account;

        require!(sub.is_active, NexusError::SubscriptionInactive);
        require!(merchant.is_active, NexusError::MerchantInactive);

        let elapsed = clock
            .unix_timestamp
            .checked_sub(sub.last_billed_at)
            .ok_or(NexusError::MathUnderflow)?;
        require!(elapsed >= sub.frequency_seconds, NexusError::BillingCyclePremature);

        let amount = sub.recurring_amount;

        // Calculate splits
        let affiliate_amount = (amount as u128)
            .checked_mul(merchant.affiliate_share_bps as u128)
            .ok_or(NexusError::MathOverflow)?
            .checked_div(10_000)
            .ok_or(NexusError::MathOverflow)? as u64;

        let protocol_amount = (amount as u128)
            .checked_mul(merchant.protocol_share_bps as u128)
            .ok_or(NexusError::MathOverflow)?
            .checked_div(10_000)
            .ok_or(NexusError::MathOverflow)? as u64;

        let merchant_amount = amount
            .checked_sub(affiliate_amount)
            .ok_or(NexusError::MathUnderflow)?
            .checked_sub(protocol_amount)
            .ok_or(NexusError::MathUnderflow)?;

        // Execute delegated transfers via Subscriber Vault PDA
        token::transfer(
            ctx.accounts.transfer_from_subscriber_vault(merchant_amount, ctx.accounts.merchant_treasury.to_account_info()),
            merchant_amount,
        )?;

        if affiliate_amount > 0 {
            token::transfer(
                ctx.accounts.transfer_from_subscriber_vault(affiliate_amount, ctx.accounts.affiliate_payout.to_account_info()),
                affiliate_amount,
            )?;
        }

        if protocol_amount > 0 {
            token::transfer(
                ctx.accounts.transfer_from_subscriber_vault(protocol_amount, ctx.accounts.protocol_fee_payout.to_account_info()),
                protocol_amount,
            )?;
        }

        sub.last_billed_at = clock.unix_timestamp;
        merchant.total_volume = merchant.total_volume.checked_add(amount).ok_or(NexusError::MathOverflow)?;
        merchant.total_orders = merchant.total_orders.checked_add(1).ok_or(NexusError::MathOverflow)?;

        emit!(SubscriptionChargedEvent {
            subscription: sub.key(),
            merchant: merchant.key(),
            amount,
            timestamp: clock.unix_timestamp,
        });

        Ok(())
    }
}

// -----------------------------------------------------------------------------
// Context Accounts
// -----------------------------------------------------------------------------

#[derive(Accounts)]
#[instruction(merchant_id: String)]
pub struct InitializeMerchant<'info> {
    #[account(
        init,
        payer = authority,
        space = 8 + MerchantAccount::MAX_SIZE,
        seeds = [b"merchant", authority.key().as_ref(), merchant_id.as_bytes()],
        bump
    )]
    pub merchant_account: Account<'info, MerchantAccount>,

    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        constraint = treasury_vault.mint == payment_mint.key() @ NexusError::InvalidTokenAccountMint
    )]
    pub treasury_vault: Account<'info, TokenAccount>,

    #[account(
        constraint = affiliate_payout_account.mint == payment_mint.key() @ NexusError::InvalidTokenAccountMint
    )]
    pub affiliate_payout_account: Account<'info, TokenAccount>,

    #[account(
        constraint = protocol_fee_account.mint == payment_mint.key() @ NexusError::InvalidTokenAccountMint
    )]
    pub protocol_fee_account: Account<'info, TokenAccount>,

    pub payment_mint: Account<'info, Mint>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(order_id: String)]
pub struct ExecuteSplitSettlement<'info> {
    #[account(
        mut,
        has_one = treasury_vault @ NexusError::UnauthorizedVault,
        has_one = affiliate_payout_account @ NexusError::UnauthorizedVault,
        has_one = protocol_fee_account @ NexusError::UnauthorizedVault,
    )]
    pub merchant_account: Account<'info, MerchantAccount>,

    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(
        mut,
        constraint = payer_token_account.owner == payer.key() @ NexusError::InvalidPayerAccount,
        constraint = payer_token_account.mint == merchant_account.payment_mint @ NexusError::InvalidTokenAccountMint
    )]
    pub payer_token_account: Account<'info, TokenAccount>,

    #[account(mut)]
    pub treasury_vault: Account<'info, TokenAccount>,

    #[account(mut)]
    pub affiliate_payout_account: Account<'info, TokenAccount>,

    #[account(mut)]
    pub protocol_fee_account: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
}

impl<'info> ExecuteSplitSettlement<'info> {
    pub fn transfer_to_merchant_context(&self) -> CpiContext<'_, '_, '_, 'info, Transfer<'info>> {
        let cpi_accounts = Transfer {
            from: self.payer_token_account.to_account_info(),
            to: self.treasury_vault.to_account_info(),
            authority: self.payer.to_account_info(),
        };
        CpiContext::new(self.token_program.to_account_info(), cpi_accounts)
    }

    pub fn transfer_to_affiliate_context(&self) -> CpiContext<'_, '_, '_, 'info, Transfer<'info>> {
        let cpi_accounts = Transfer {
            from: self.payer_token_account.to_account_info(),
            to: self.affiliate_payout_account.to_account_info(),
            authority: self.payer.to_account_info(),
        };
        CpiContext::new(self.token_program.to_account_info(), cpi_accounts)
    }

    pub fn transfer_to_protocol_context(&self) -> CpiContext<'_, '_, '_, 'info, Transfer<'info>> {
        let cpi_accounts = Transfer {
            from: self.payer_token_account.to_account_info(),
            to: self.protocol_fee_account.to_account_info(),
            authority: self.payer.to_account_info(),
        };
        CpiContext::new(self.token_program.to_account_info(), cpi_accounts)
    }
}

#[derive(Accounts)]
#[instruction(plan_id: String)]
pub struct CreateSubscription<'info> {
    #[account(
        init,
        payer = subscriber,
        space = 8 + SubscriptionAgreement::MAX_SIZE,
        seeds = [b"subscription", subscriber.key().as_ref(), merchant_account.key().as_ref(), plan_id.as_bytes()],
        bump
    )]
    pub subscription_agreement: Account<'info, SubscriptionAgreement>,

    #[account(mut)]
    pub subscriber: Signer<'info>,

    #[account(mut)]
    pub merchant_account: Account<'info, MerchantAccount>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ExecuteSubscriptionCharge<'info> {
    #[account(
        mut,
        has_one = merchant @ NexusError::InvalidMerchantAccount,
    )]
    pub subscription_agreement: Account<'info, SubscriptionAgreement>,

    #[account(mut)]
    pub merchant_account: Account<'info, MerchantAccount>,

    /// CHECK: Safe delegated token source account
    #[account(mut)]
    pub subscriber_token_account: Account<'info, TokenAccount>,

    /// Signer acting as subscription charge worker / relayer
    pub keeper: Signer<'info>,

    #[account(mut)]
    pub merchant_treasury: Account<'info, TokenAccount>,

    #[account(mut)]
    pub affiliate_payout: Account<'info, TokenAccount>,

    #[account(mut)]
    pub protocol_fee_payout: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
}

impl<'info> ExecuteSubscriptionCharge<'info> {
    pub fn transfer_from_subscriber_vault<'a>(
        &'a self,
        _amount: u64,
        destination: AccountInfo<'info>,
    ) -> CpiContext<'a, 'a, 'a, 'info, Transfer<'info>> {
        let cpi_accounts = Transfer {
            from: self.subscriber_token_account.to_account_info(),
            to: destination,
            authority: self.subscription_agreement.to_account_info(),
        };
        CpiContext::new(self.token_program.to_account_info(), cpi_accounts)
    }
}

// -----------------------------------------------------------------------------
// State Accounts
// -----------------------------------------------------------------------------

#[account]
pub struct MerchantAccount {
    pub authority: Pubkey,
    pub merchant_id: String,
    pub treasury_vault: Pubkey,
    pub affiliate_payout_account: Pubkey,
    pub protocol_fee_account: Pubkey,
    pub payment_mint: Pubkey,
    pub affiliate_share_bps: u16,
    pub protocol_share_bps: u16,
    pub total_volume: u64,
    pub total_orders: u64,
    pub is_active: bool,
    pub bump: u8,
}

impl MerchantAccount {
    pub const MAX_SIZE: usize = 32 + (4 + 32) + 32 + 32 + 32 + 32 + 2 + 2 + 8 + 8 + 1 + 1 + 64;
}

#[account]
pub struct SubscriptionAgreement {
    pub subscriber: Pubkey,
    pub merchant: Pubkey,
    pub plan_id: String,
    pub recurring_amount: u64,
    pub frequency_seconds: i64,
    pub last_billed_at: i64,
    pub is_active: bool,
    pub bump: u8,
}

impl SubscriptionAgreement {
    pub const MAX_SIZE: usize = 32 + 32 + (4 + 32) + 8 + 8 + 8 + 1 + 1 + 32;
}

// -----------------------------------------------------------------------------
// Events
// -----------------------------------------------------------------------------

#[event]
pub struct MerchantRegisteredEvent {
    pub merchant: Pubkey,
    pub authority: Pubkey,
    pub merchant_id: String,
    pub payment_mint: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct PaymentSettledEvent {
    pub merchant: Pubkey,
    pub order_id: String,
    pub payer: Pubkey,
    pub total_amount: u64,
    pub merchant_amount: u64,
    pub affiliate_amount: u64,
    pub protocol_amount: u64,
    pub timestamp: i64,
}

#[event]
pub struct SubscriptionCreatedEvent {
    pub subscription: Pubkey,
    pub subscriber: Pubkey,
    pub merchant: Pubkey,
    pub recurring_amount: u64,
    pub frequency_seconds: i64,
    pub timestamp: i64,
}

#[event]
pub struct SubscriptionChargedEvent {
    pub subscription: Pubkey,
    pub merchant: Pubkey,
    pub amount: u64,
    pub timestamp: i64,
}

// -----------------------------------------------------------------------------
// Errors
// -----------------------------------------------------------------------------

#[error_code]
pub enum NexusError {
    #[msg("Merchant ID must be <= 32 characters.")]
    MerchantIdTooLong,
    #[msg("Order ID must be <= 36 characters.")]
    OrderIdTooLong,
    #[msg("Plan ID must be <= 32 characters.")]
    PlanIdTooLong,
    #[msg("Invalid BPS configuration. Total rev-share must be under 10000.")]
    InvalidBpsConfiguration,
    #[msg("Payment amount must be greater than zero.")]
    InvalidAmount,
    #[msg("Merchant account is currently inactive.")]
    MerchantInactive,
    #[msg("Subscription agreement is currently inactive.")]
    SubscriptionInactive,
    #[msg("Billing frequency must be at least 60 seconds.")]
    FrequencyTooShort,
    #[msg("Premature billing cycle invocation.")]
    BillingCyclePremature,
    #[msg("Math calculation resulted in overflow.")]
    MathOverflow,
    #[msg("Math calculation resulted in underflow.")]
    MathUnderflow,
    #[msg("Token account mint mismatch.")]
    InvalidTokenAccountMint,
    #[msg("Provided vault does not match merchant authorization.")]
    UnauthorizedVault,
    #[msg("Payer does not own the debit account.")]
    InvalidPayerAccount,
    #[msg("Merchant reference mismatch.")]
    InvalidMerchantAccount,
}