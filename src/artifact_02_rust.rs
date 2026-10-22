use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};

declare_id!("MzkPay1111111111111111111111111111111111111");

#[program]
pub mod mozaik_escrow {
    use super::*;

    /// Initializes a new MozaikPay escrow backed by SPL token collateral.
    pub fn initialize_escrow(
        ctx: Context<InitializeEscrow>,
        escrow_id: String,
        total_amount: u64,
        milestone_count: u8,
        dispute_timeout_seconds: i64,
    ) -> Result<()> {
        require!(escrow_id.len() <= 32, EscrowError::EscrowIdTooLong);
        require!(total_amount > 0, EscrowError::InvalidTotalAmount);
        require!(milestone_count > 0, EscrowError::ZeroMilestones);

        let escrow = &mut ctx.accounts.escrow_state;
        let clock = Clock::get()?;

        escrow.buyer = ctx.accounts.buyer.key();
        escrow.merchant = ctx.accounts.merchant.key();
        escrow.mint = ctx.accounts.token_mint.key();
        escrow.escrow_id = escrow_id.clone();
        escrow.total_amount = total_amount;
        escrow.amount_remaining = total_amount;
        escrow.milestone_count = milestone_count;
        escrow.current_milestone = 0;
        escrow.bump = ctx.bumps.escrow_state;
        escrow.vault_bump = ctx.bumps.vault;
        escrow.created_at = clock.unix_timestamp;
        escrow.dispute_timeout = clock.unix_timestamp + dispute_timeout_seconds;
        escrow.status = EscrowStatus::Funded;

        // Transfer funds from Buyer to Program Vault PDA
        let cpi_accounts = Transfer {
            from: ctx.accounts.buyer_token_account.to_account_info(),
            to: ctx.accounts.vault.to_account_info(),
            authority: ctx.accounts.buyer.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
        token::transfer(cpi_ctx, total_amount)?;

        emit!(EscrowFundedEvent {
            escrow_id,
            buyer: escrow.buyer,
            merchant: escrow.merchant,
            amount: total_amount,
            timestamp: clock.unix_timestamp,
        });

        Ok(())
    }

    /// Releases a designated milestone tranche to the merchant.
    pub fn release_milestone(
        ctx: Context<ReleaseMilestone>,
        milestone_index: u8,
        amount_to_release: u64,
    ) -> Result<()> {
        let escrow = &mut ctx.accounts.escrow_state;
        let clock = Clock::get()?;

        require!(escrow.status == EscrowStatus::Funded, EscrowError::InvalidEscrowStatus);
        require!(ctx.accounts.buyer.key() == escrow.buyer, EscrowError::UnauthorizedCaller);
        require!(milestone_index == escrow.current_milestone, EscrowError::MilestoneOutOfOrder);
        require!(amount_to_release <= escrow.amount_remaining, EscrowError::InsufficientFundsRemaining);

        escrow.amount_remaining = escrow.amount_remaining.checked_sub(amount_to_release)
            .ok_or(EscrowError::MathOverflow)?;
        escrow.current_milestone = escrow.current_milestone.checked_add(1)
            .ok_or(EscrowError::MathOverflow)?;

        if escrow.current_milestone == escrow.milestone_count || escrow.amount_remaining == 0 {
            escrow.status = EscrowStatus::Completed;
        }

        // PDA Signer seeds for Vault transfer
        let escrow_id_bytes = escrow.escrow_id.as_bytes();
        let seeds = &[
            b"vault",
            escrow_id_bytes,
            &[escrow.vault_bump],
        ];
        let signer = &[&seeds[..]];

        let cpi_accounts = Transfer {
            from: ctx.accounts.vault.to_account_info(),
            to: ctx.accounts.merchant_token_account.to_account_info(),
            authority: ctx.accounts.vault.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer);
        token::transfer(cpi_ctx, amount_to_release)?;

        emit!(MilestoneReleasedEvent {
            escrow_id: escrow.escrow_id.clone(),
            milestone_index,
            amount_released: amount_to_release,
            remaining_balance: escrow.amount_remaining,
            timestamp: clock.unix_timestamp,
        });

        Ok(())
    }

    /// Refunds the remaining escrow collateral back to the buyer if dispute timeout expires.
    pub fn refund_expired(ctx: Context<RefundExpired>) -> Result<()> {
        let escrow = &mut ctx.accounts.escrow_state;
        let clock = Clock::get()?;

        require!(clock.unix_timestamp > escrow.dispute_timeout, EscrowError::TimeoutNotReached);
        require!(escrow.status == EscrowStatus::Funded, EscrowError::InvalidEscrowStatus);
        require!(escrow.amount_remaining > 0, EscrowError::ZeroBalance);

        let refund_amount = escrow.amount_remaining;
        escrow.amount_remaining = 0;
        escrow.status = EscrowStatus::Refunded;

        let escrow_id_bytes = escrow.escrow_id.as_bytes();
        let seeds = &[
            b"vault",
            escrow_id_bytes,
            &[escrow.vault_bump],
        ];
        let signer = &[&seeds[..]];

        let cpi_accounts = Transfer {
            from: ctx.accounts.vault.to_account_info(),
            to: ctx.accounts.buyer_token_account.to_account_info(),
            authority: ctx.accounts.vault.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer);
        token::transfer(cpi_ctx, refund_amount)?;

        emit!(EscrowRefundedEvent {
            escrow_id: escrow.escrow_id.clone(),
            buyer: escrow.buyer,
            amount_refunded: refund_amount,
            timestamp: clock.unix_timestamp,
        });

        Ok(())
    }
}

// -----------------------------------------------------------------------------
// ACCOUNT VALIDATION CONTEXTS
// -----------------------------------------------------------------------------

#[derive(Accounts)]
#[instruction(escrow_id: String)]
pub struct InitializeEscrow<'info> {
    #[account(mut)]
    pub buyer: Signer<'info>,

    /// CHECK: Safe external merchant pubkey
    pub merchant: AccountInfo<'info>,

    pub token_mint: Account<'info, Mint>,

    #[account(
        mut,
        constraint = buyer_token_account.mint == token_mint.key(),
        constraint = buyer_token_account.owner == buyer.key()
    )]
    pub buyer_token_account: Account<'info, TokenAccount>,

    #[account(
        init,
        payer = buyer,
        space = 8 + EscrowAccount::INIT_SPACE,
        seeds = [b"escrow", escrow_id.as_bytes()],
        bump
    )]
    pub escrow_state: Account<'info, EscrowAccount>,

    #[account(
        init,
        payer = buyer,
        seeds = [b"vault", escrow_id.as_bytes()],
        bump,
        token::mint = token_mint,
        token::authority = vault,
    )]
    pub vault: Account<'info, TokenAccount>,

    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct ReleaseMilestone<'info> {
    #[account(mut)]
    pub buyer: Signer<'info>,

    /// CHECK: Verified against state
    #[account(mut, address = escrow_state.merchant)]
    pub merchant: AccountInfo<'info>,

    #[account(
        mut,
        constraint = merchant_token_account.mint == escrow_state.mint,
        constraint = merchant_token_account.owner == escrow_state.merchant
    )]
    pub merchant_token_account: Account<'info, TokenAccount>,

    #[account(
        mut,
        seeds = [b"escrow", escrow_state.escrow_id.as_bytes()],
        bump = escrow_state.bump,
        has_one = buyer,
    )]
    pub escrow_state: Account<'info, EscrowAccount>,

    #[account(
        mut,
        seeds = [b"vault", escrow_state.escrow_id.as_bytes()],
        bump = escrow_state.vault_bump,
    )]
    pub vault: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct RefundExpired<'info> {
    #[account(mut)]
    pub caller: Signer<'info>,

    /// CHECK: Recipient must be original buyer
    #[account(mut, address = escrow_state.buyer)]
    pub buyer: AccountInfo<'info>,

    #[account(
        mut,
        constraint = buyer_token_account.mint == escrow_state.mint,
        constraint = buyer_token_account.owner == escrow_state.buyer
    )]
    pub buyer_token_account: Account<'info, TokenAccount>,

    #[account(
        mut,
        seeds = [b"escrow", escrow_state.escrow_id.as_bytes()],
        bump = escrow_state.bump,
        has_one = buyer,
    )]
    pub escrow_state: Account<'info, EscrowAccount>,

    #[account(
        mut,
        seeds = [b"vault", escrow_state.escrow_id.as_bytes()],
        bump = escrow_state.vault_bump,
    )]
    pub vault: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
}

// -----------------------------------------------------------------------------
// STATE DEFINITIONS & ENUMS
// -----------------------------------------------------------------------------

#[account]
#[derive(InitSpace)]
pub struct EscrowAccount {
    pub buyer: Pubkey,
    pub merchant: Pubkey,
    pub mint: Pubkey,
    #[max_len(32)]
    pub escrow_id: String,
    pub total_amount: u64,
    pub amount_remaining: u64,
    pub current_milestone: u8,
    pub milestone_count: u8,
    pub bump: u8,
    pub vault_bump: u8,
    pub created_at: i64,
    pub dispute_timeout: i64,
    pub status: EscrowStatus,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, InitSpace)]
pub enum EscrowStatus {
    Funded,
    Completed,
    Refunded,
}

// -----------------------------------------------------------------------------
// EVENTS
// -----------------------------------------------------------------------------

#[event]
pub struct EscrowFundedEvent {
    pub escrow_id: String,
    pub buyer: Pubkey,
    pub merchant: Pubkey,
    pub amount: u64,
    pub timestamp: i64,
}

#[event]
pub struct MilestoneReleasedEvent {
    pub escrow_id: String,
    pub milestone_index: u8,
    pub amount_released: u64,
    pub remaining_balance: u64,
    pub timestamp: i64,
}

#[event]
pub struct EscrowRefundedEvent {
    pub escrow_id: String,
    pub buyer: Pubkey,
    pub amount_refunded: u64,
    pub timestamp: i64,
}

// -----------------------------------------------------------------------------
// ERRORS
// -----------------------------------------------------------------------------

#[error_code]
pub enum EscrowError {
    #[msg("Escrow identifier must not exceed 32 ASCII bytes.")]
    EscrowIdTooLong,
    #[msg("Total deposit amount must be greater than zero.")]
    InvalidTotalAmount,
    #[msg("Escrow milestone count must be at least 1.")]
    ZeroMilestones,
    #[msg("Escrow is not in the Funded active state.")]
    InvalidEscrowStatus,
    #[msg("Caller is unauthorized to release funds.")]
    UnauthorizedCaller,
    #[msg("Milestone released out of chronological order.")]
    MilestoneOutOfOrder,
    #[msg("Requested release amount exceeds available escrow balance.")]
    InsufficientFundsRemaining,
    #[msg("Dispute timeout period has not elapsed yet.")]
    TimeoutNotReached,
    #[msg("No remaining balance exists in the vault.")]
    ZeroBalance,
    #[msg("Arithmetic overflow occurred during balance calculation.")]
    MathOverflow,
}