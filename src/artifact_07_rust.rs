use anchor_lang::prelude::*;
use anchor_spl::token::Mint;
use crate::state::*;
use crate::errors::MozaikError;

#[derive(Accounts)]
#[instruction(plan_id: u64)]
pub struct CreatePlan<'info> {
    #[account(mut)]
    pub merchant_admin: Signer<'info>,

    #[account(
        init,
        payer = merchant_admin,
        space = 8 + PlanConfig::INIT_SPACE,
        seeds = [PLAN_SEED, merchant_admin.key().as_ref(), &plan_id.to_le_bytes()],
        bump
    )]
    pub plan: Account<'info, PlanConfig>,

    pub payment_mint: Account<'info, Mint>,

    /// CHECK: Primary settlement wallet validated via plan config
    pub merchant_wallet: UncheckedAccount<'info>,
    /// CHECK: Protocol treasury wallet validated via plan config
    pub protocol_treasury: UncheckedAccount<'info>,
    /// CHECK: Optional affiliate/sales partner wallet
    pub affiliate_wallet: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

pub fn handle_create_plan(
    ctx: Context<CreatePlan>,
    plan_id: u64,
    amount_per_cycle: u64,
    frequency_seconds: i64,
    merchant_bps: u16,
    protocol_bps: u16,
    affiliate_bps: u16,
) -> Result<()> {
    require!(
        (merchant_bps as u64 + protocol_bps as u64 + affiliate_bps as u64) == BPS_DENOMINATOR,
        MozaikError::InvalidSplitBps
    );

    let plan = &mut ctx.accounts.plan;
    plan.merchant_admin = ctx.accounts.merchant_admin.key();
    plan.payment_mint = ctx.accounts.payment_mint.key();
    plan.amount_per_cycle = amount_per_cycle;
    plan.billing_frequency_seconds = frequency_seconds;
    plan.merchant_wallet = ctx.accounts.merchant_wallet.key();
    plan.protocol_treasury = ctx.accounts.protocol_treasury.key();
    plan.affiliate_wallet = ctx.accounts.affiliate_wallet.key();
    plan.merchant_bps = merchant_bps;
    plan.protocol_bps = protocol_bps;
    plan.affiliate_bps = affiliate_bps;
    plan.plan_id = plan_id;
    plan.bump = ctx.bumps.plan;

    msg!("MozaikFlow Plan #{} created successfully for merchant {}", plan_id, plan.merchant_admin);
    Ok(())
}