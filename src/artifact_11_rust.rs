// programs/mozaikflow/src/lib.rs
use anchor_lang::prelude::*;

pub mod errors;
pub mod instructions;
pub mod state;

use instructions::*;

declare_id!("MozkF1ow11111111111111111111111111111111111");

#[program]
pub mod mozaikflow {
    use super::*;

    pub fn create_plan(
        ctx: Context<CreatePlan>,
        plan_id: u64,
        amount_per_cycle: u64,
        frequency_seconds: i64,
        merchant_bps: u16,
        protocol_bps: u16,
        affiliate_bps: u16,
    ) -> Result<()> {
        instructions::create_plan::handle_create_plan(
            ctx,
            plan_id,
            amount_per_cycle,
            frequency_seconds,
            merchant_bps,
            protocol_bps,
            affiliate_bps,
        )
    }

    pub fn initialize_subscription(ctx: Context<InitializeSubscription>) -> Result<()> {
        instructions::initialize_subscription::handle_initialize_subscription(ctx)
    }

    pub fn execute_payment(ctx: Context<ExecutePayment>) -> Result<()> {
        instructions::execute_payment::handle_execute_payment(ctx)
    }
}