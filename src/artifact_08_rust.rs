use anchor_lang::prelude::*;
use crate::state::*;

#[derive(Accounts)]
pub struct InitializeSubscription<'info> {
    #[account(mut)]
    pub subscriber: Signer<'info>,

    pub plan: Account<'info, PlanConfig>,

    #[account(
        init,
        payer = subscriber,
        space = 8 + SubscriptionRecord::INIT_SPACE,
        seeds = [SUB_SEED, subscriber.key().as_ref(), plan.key().as_ref()],
        bump
    )]
    pub subscription: Account<'info, SubscriptionRecord>,

    pub system_program: Program<'info, System>,
}

pub fn handle_initialize_subscription(ctx: Context<InitializeSubscription>) -> Result<()> {
    let sub = &mut ctx.accounts.subscription;
    sub.subscriber = ctx.accounts.subscriber.key();
    sub.plan = ctx.accounts.plan.key();
    sub.last_billed_timestamp = 0; // Trigger immediately on first execution
    sub.total_cycles_paid = 0;
    sub.is_active = true;
    sub.bump = ctx.bumps.subscription;

    msg!(
        "Subscription registered. Subscriber: {}, Plan: {}",
        sub.subscriber,
        sub.plan
    );
    Ok(())
}