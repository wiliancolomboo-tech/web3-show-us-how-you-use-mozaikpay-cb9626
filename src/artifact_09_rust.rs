use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount, Transfer};
use crate::state::*;
use crate::errors::MozaikError;

#[derive(Accounts)]
pub struct ExecutePayment<'info> {
    #[account(mut)]
    pub crank_caller: Signer<'info>,

    #[account(
        seeds = [PLAN_SEED, plan.merchant_admin.as_ref(), &plan.plan_id.to_le_bytes()],
        bump = plan.bump,
        has_one = merchant_wallet,
        has_one = protocol_treasury,
        has_one = affiliate_wallet,
    )]
    pub plan: Account<'info, PlanConfig>,

    #[account(
        mut,
        seeds = [SUB_SEED, subscription.subscriber.as_ref(), plan.key().as_ref()],
        bump = subscription.bump,
        has_one = plan,
    )]
    pub subscription: Account<'info, SubscriptionRecord>,

    /// CHECK: Delegated subscriber vault token account
    #[account(mut)]
    pub subscriber_token_account: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = merchant_token_account.owner == plan.merchant_wallet
    )]
    pub merchant_token_account: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = protocol_token_account.owner == plan.protocol_treasury
    )]
    pub protocol_token_account: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = affiliate_token_account.owner == plan.affiliate_wallet
    )]
    pub affiliate_token_account: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
}

pub fn handle_execute_payment(ctx: Context<ExecutePayment>) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let plan = &ctx.accounts.plan;
    let sub = &mut ctx.accounts.subscription;

    require!(sub.is_active, MozaikError::SubscriptionInactive);

    // Enforce timing window
    if sub.last_billed_timestamp > 0 {
        let elapsed = now.checked_sub(sub.last_billed_timestamp).ok_or(MozaikError::MathOverflow)?;
        require!(
            elapsed >= plan.billing_frequency_seconds,
            MozaikError::BillingCycleNotDue
        );
    }

    let total_amount = plan.amount_per_cycle;

    // Mathematical split calculations
    let merchant_amount = (total_amount as u128)
        .checked_mul(plan.merchant_bps as u128)
        .ok_or(MozaikError::MathOverflow)?
        .checked_div(BPS_DENOMINATOR as u128)
        .ok_or(MozaikError::MathOverflow)? as u64;

    let protocol_amount = (total_amount as u128)
        .checked_mul(plan.protocol_bps as u128)
        .ok_or(MozaikError::MathOverflow)?
        .checked_div(BPS_DENOMINATOR as u128)
        .ok_or(MozaikError::MathOverflow)? as u64;

    let affiliate_amount = total_amount
        .checked_sub(merchant_amount)
        .ok_or(MozaikError::MathOverflow)?
        .checked_sub(protocol_amount)
        .ok_or(MozaikError::MathOverflow)?;

    // 1. Transfer to Merchant Account
    let cpi_accounts_merchant = Transfer {
        from: ctx.accounts.subscriber_token_account.to_account_info(),
        to: ctx.accounts.merchant_token_account.to_account_info(),
        authority: ctx.accounts.crank_caller.to_account_info(),
    };
    let cpi_ctx_merchant = CpiContext::new(ctx.accounts.token_program.to_account_info(), cpi_accounts_merchant);
    token::transfer(cpi_ctx_merchant, merchant_amount)?;

    // 2. Transfer to Protocol Treasury Account
    let cpi_accounts_treasury = Transfer {
        from: ctx.accounts.subscriber_token_account.to_account_info(),
        to: ctx.accounts.protocol_token_account.to_account_info(),
        authority: ctx.accounts.crank_caller.to_account_info(),
    };
    let cpi_ctx_treasury = CpiContext::new(ctx.accounts.token_program.to_account_info(), cpi_accounts_treasury);
    token::transfer(cpi_ctx_treasury, protocol_amount)?;

    // 3. Transfer to Affiliate Account
    if affiliate_amount > 0 {
        let cpi_accounts_affiliate = Transfer {
            from: ctx.accounts.subscriber_token_account.to_account_info(),
            to: ctx.accounts.affiliate_token_account.to_account_info(),
            authority: ctx.accounts.crank_caller.to_account_info(),
        };
        let cpi_ctx_affiliate = CpiContext::new(ctx.accounts.token_program.to_account_info(), cpi_accounts_affiliate);
        token::transfer(cpi_ctx_affiliate, affiliate_amount)?;
    }

    // Update state
    sub.last_billed_timestamp = now;
    sub.total_cycles_paid = sub.total_cycles_paid.checked_add(1).ok_or(MozaikError::MathOverflow)?;

    emit!(PaymentExecutedEvent {
        subscriber: sub.subscriber,
        plan: plan.key(),
        amount: total_amount,
        cycle: sub.total_cycles_paid,
        timestamp: now,
    });

    Ok(())
}

#[event]
pub struct PaymentExecutedEvent {
    pub subscriber: Pubkey,
    pub plan: Pubkey,
    pub amount: u64,
    pub cycle: u64,
    pub timestamp: i64,
}