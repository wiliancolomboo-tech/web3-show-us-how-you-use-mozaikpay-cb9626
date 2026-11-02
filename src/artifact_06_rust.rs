use anchor_lang::prelude::*;

pub const PLAN_SEED: &[u8] = b"mozaik_plan";
pub const SUB_SEED: &[u8] = b"mozaik_sub";
pub const BPS_DENOMINATOR: u64 = 10_000;

#[account]
#[derive(InitSpace)]
pub struct PlanConfig {
    pub merchant_admin: Pubkey,
    pub payment_mint: Pubkey,
    pub amount_per_cycle: u64,
    pub billing_frequency_seconds: i64,
    pub merchant_wallet: Pubkey,
    pub protocol_treasury: Pubkey,
    pub affiliate_wallet: Pubkey,
    pub merchant_bps: u16,    // e.g. 8500 = 85%
    pub protocol_bps: u16,    // e.g. 1000 = 10%
    pub affiliate_bps: u16,   // e.g. 500  = 5%
    pub plan_id: u64,
    pub bump: u8,
}

#[account]
#[derive(InitSpace)]
pub struct SubscriptionRecord {
    pub subscriber: Pubkey,
    pub plan: Pubkey,
    pub last_billed_timestamp: i64,
    pub total_cycles_paid: u64,
    pub is_active: bool,
    pub bump: u8,
}