use anchor_lang::prelude::*;

pub mod state;
pub mod errors;
pub mod events;
pub mod instructions;

use instructions::*;
use state::SplitRecipient;

declare_id!("MozPay1111111111111111111111111111111111111");

#[program]
pub mod mozaik_stream {
    use super::*;

    pub fn initialize_plan(
        ctx: Context<InitializePlan>,
        plan_id: u64,
        amount: u64,
        interval_seconds: i64,
        recipients: Vec<SplitRecipient>,
        crank_fee_bps: u16,
    ) -> Result<()> {
        instructions::initialize_plan::handler(ctx, plan_id, amount, interval_seconds, recipients, crank_fee_bps)
    }

    pub fn subscribe(ctx: Context<Subscribe>) -> Result<()> {
        instructions::subscribe::handler(ctx)
    }

    pub fn execute_charge<'a, 'b, 'c, 'info>(
        ctx: Context<'a, 'b, 'c, 'info, ExecuteCharge<'info>>,
    ) -> Result<()> {
        instructions::execute_charge::handler(ctx)
    }

    pub fn cancel_subscription(ctx: Context<CancelSubscription>) -> Result<()> {
        instructions::cancel_subscription::handler(ctx)
    }
}