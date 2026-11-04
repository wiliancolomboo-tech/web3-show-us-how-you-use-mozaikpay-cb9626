use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};

declare_id!("MoZaiKP111111111111111111111111111111111111");

pub const BPS_DENOMINATOR: u16 = 10_000;
pub const MAX_SPLIT_RECIPIENTS: usize = 5;

#[program]
pub mod mozaik_pulse {
    use super::*;

    /// Initialize a multi-party split-settlement configuration for enterprise checkouts
    pub fn initialize_merchant_config(
        ctx: Context<InitializeMerchantConfig>,
        merchant_id: String,
        split_percentages: Vec<u16>,
    ) -> Result<()> {
        require!(merchant_id.len() <= 32, PulseError::IdentifierTooLong);
        require!(
            split_percentages.len() == ctx.remaining_accounts.len(),
            PulseError::MismatchedSplitAccounts
        );

        let total_bps: u32 = split_percentages.iter().map(|&x| x as u32).sum();
        require!(total_bps == BPS_DENOMINATOR as u32, PulseError::InvalidSplitBasisPoints);

        let config = &mut ctx.accounts.merchant_config;
        config.merchant_authority = ctx.accounts.authority.key();
        config.merchant_id = merchant_id;
        config.settlement_mint = ctx.accounts.settlement_mint.key();
        config.recipient_count = split_percentages.len() as u8;
        config.bump = ctx.bumps.merchant_config;

        for (i, (&percentage, acc_info)) in split_percentages.iter().zip(ctx.remaining_accounts.iter()).enumerate() {
            config.splits[i] = SplitRecipient {
                wallet: acc_info.key(),
                basis_points: percentage,
            };
        }

        emit!(MerchantConfigCreated {
            merchant: ctx.accounts.authority.key(),
            merchant_id: config.merchant_id.clone(),
            recipient_count: config.recipient_count,
        });

        Ok(())
    }

    /// Process an instant multi-vendor checkout split settlement
    pub fn execute_checkout_payment(
        ctx: Context<ExecuteCheckoutPayment>,
        order_id: String,
        amount: u64,
    ) -> Result<()> {
        require!(amount > 0, PulseError::ZeroPaymentAmount);
        require!(order_id.len() <= 64, PulseError::IdentifierTooLong);

        let config = &ctx.accounts.merchant_config;
        let recipient_count = config.recipient_count as usize;
        require!(
            ctx.remaining_accounts.len() == recipient_count,
            PulseError::InvalidSplitAccountsLength
        );

        let mut distributed_total: u64 = 0;

        // Perform split distribution directly to vendor vaults
        for i in 0..recipient_count {
            let split = &config.splits[i];
            let target_token_account_info = &ctx.remaining_accounts[i];
            
            // Verify recipient address matches registered config
            require_keys_eq!(
                split.wallet,
                target_token_account_info.key(),
                PulseError::UnauthorizedSplitTarget
            );

            // Compute split fraction; last recipient absorbs fractional remainder dust
            let split_amount = if i == recipient_count - 1 {
                amount.checked_sub(distributed_total).ok_or(PulseError::MathOverflow)?
            } else {
                (amount as u128)
                    .checked_mul(split.basis_points as u128)
                    .ok_or(PulseError::MathOverflow)?
                    .checked_div(BPS_DENOMINATOR as u128)
                    .ok_or(PulseError::MathOverflow)? as u64
            };

            distributed_total = distributed_total
                .checked_add(split_amount)
                .ok_or(PulseError::MathOverflow)?;

            if split_amount > 0 {
                let cpi_accounts = Transfer {
                    from: ctx.accounts.payer_token_account.to_account_info(),
                    to: target_token_account_info.clone(),
                    authority: ctx.accounts.payer.to_account_info(),
                };
                let cpi_program = ctx.accounts.token_program.to_account_info();
                let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
                token::transfer(cpi_ctx, split_amount)?;
            }
        }

        emit!(PaymentSettled {
            order_id,
            payer: ctx.accounts.payer.key(),
            merchant: config.merchant_authority,
            total_amount: amount,
            timestamp: Clock::get()?.unix_timestamp,
        });

        Ok(())
    }

    /// Create an enterprise linear payment stream (e.g. for payroll or retainers)
    pub fn create_linear_stream(
        ctx: Context<CreateLinearStream>,
        stream_id: String,
        total_deposit: u64,
        start_time: i64,
        end_time: i64,
    ) -> Result<()> {
        let current_time = Clock::get()?.unix_timestamp;
        require!(start_time >= current_time - 300, PulseError::InvalidStartTime);
        require!(end_time > start_time, PulseError::InvalidEndTime);
        require!(total_deposit > 0, PulseError::ZeroPaymentAmount);
        require!(stream_id.len() <= 32, PulseError::IdentifierTooLong);

        let stream = &mut ctx.accounts.stream_account;
        stream.stream_id = stream_id.clone();
        stream.sender = ctx.accounts.sender.key();
        stream.recipient = ctx.accounts.recipient.key();
        stream.mint = ctx.accounts.mint.key();
        stream.vault = ctx.accounts.vault_token_account.key();
        stream.total_deposit = total_deposit;
        stream.withdrawn_amount = 0;
        stream.start_time = start_time;
        stream.end_time = end_time;
        stream.is_active = true;
        stream.bump = ctx.bumps.stream_account;
        stream.vault_bump = ctx.bumps.vault_token_account;

        // Escrow funds into PDA token vault
        let cpi_accounts = Transfer {
            from: ctx.accounts.sender_token_account.to_account_info(),
            to: ctx.accounts.vault_token_account.to_account_info(),
            authority: ctx.accounts.sender.to_account_info(),
        };
        let cpi_ctx = CpiContext::new(ctx.accounts.token_program.to_account_info(), cpi_accounts);
        token::transfer(cpi_ctx, total_deposit)?;

        emit!(StreamCreated {
            stream_id,
            sender: ctx.accounts.sender.key(),
            recipient: ctx.accounts.recipient.key(),
            total_deposit,
            start_time,
            end_time,
        });

        Ok(())
    }

    /// Withdraw vested funds from an active stream
    pub fn withdraw_from_stream(
        ctx: Context<WithdrawFromStream>,
        amount_requested: u64,
    ) -> Result<()> {
        let stream = &mut ctx.accounts.stream_account;
        let current_time = Clock::get()?.unix_timestamp;

        require!(stream.is_active, PulseError::StreamInactive);
        require_keys_eq!(stream.recipient, ctx.accounts.recipient.key(), PulseError::Unauthorized);

        // Calculate vested tokens based on continuous linear curve
        let vested_amount = if current_time >= stream.end_time {
            stream.total_deposit
        } else if current_time <= stream.start_time {
            0
        } else {
            let elapsed = (current_time - stream.start_time) as u128;
            let duration = (stream.end_time - stream.start_time) as u128;
            ((stream.total_deposit as u128) * elapsed / duration) as u64
        };

        let available_to_withdraw = vested_amount
            .checked_sub(stream.withdrawn_amount)
            .ok_or(PulseError::MathOverflow)?;

        require!(available_to_withdraw > 0, PulseError::NoAvailableVestedFunds);
        
        let withdraw_actual = std::cmp::min(amount_requested, available_to_withdraw);
        require!(withdraw_actual > 0, PulseError::ZeroWithdrawal);

        stream.withdrawn_amount = stream
            .withdrawn_amount
            .checked_add(withdraw_actual)
            .ok_or(PulseError::MathOverflow)?;

        // PDA signer seeds
        let stream_id_bytes = stream.stream_id.as_bytes();
        let seeds = &[
            b"stream_vault",
            stream_id_bytes,
            &[stream.vault_bump],
        ];
        let signer_seeds = &[&seeds[..]];

        let cpi_accounts = Transfer {
            from: ctx.accounts.vault_token_account.to_account_info(),
            to: ctx.accounts.recipient_token_account.to_account_info(),
            authority: ctx.accounts.vault_token_account.to_account_info(),
        };
        let cpi_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            cpi_accounts,
            signer_seeds,
        );
        token::transfer(cpi_ctx, withdraw_actual)?;

        emit!(FundsWithdrawn {
            stream_id: stream.stream_id.clone(),
            recipient: ctx.accounts.recipient.key(),
            amount: withdraw_actual,
            total_withdrawn: stream.withdrawn_amount,
            timestamp: current_time,
        });

        Ok(())
    }
}

// -----------------------------------------------------------------------------
// ACCOUNT VALIDATION CONTEXTS
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
    pub settlement_mint: Account<'info, Mint>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(order_id: String)]
pub struct ExecuteCheckoutPayment<'info> {
    #[account(
        seeds = [
            b"merchant_config", 
            merchant_config.merchant_authority.as_ref(), 
            merchant_config.merchant_id.as_bytes()
        ],
        bump = merchant_config.bump
    )]
    pub merchant_config: Account<'info, MerchantConfig>,

    #[account(
        mut,
        constraint = payer_token_account.mint == merchant_config.settlement_mint @ PulseError::InvalidMint
    )]
    pub payer_token_account: Account<'info, TokenAccount>,

    pub payer: Signer<'info>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
#[instruction(stream_id: String)]
pub struct CreateLinearStream<'info> {
    #[account(
        init,
        payer = sender,
        space = 8 + StreamAccount::INIT_SPACE,
        seeds = [b"stream_account", sender.key().as_ref(), stream_id.as_bytes()],
        bump
    )]
    pub stream_account: Account<'info, StreamAccount>,

    pub mint: Account<'info, Mint>,

    #[account(
        mut,
        constraint = sender_token_account.mint == mint.key() @ PulseError::InvalidMint
    )]
    pub sender_token_account: Account<'info, TokenAccount>,

    #[account(
        init,
        payer = sender,
        seeds = [b"stream_vault", stream_id.as_bytes()],
        bump,
        token::mint = mint,
        token::authority = vault_token_account
    )]
    pub vault_token_account: Account<'info, TokenAccount>,

    /// CHECK: Stream recipient can be any Solana system account or PDA
    pub recipient: UncheckedAccount<'info>,

    #[account(mut)]
    pub sender: Signer<'info>,
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct WithdrawFromStream<'info> {
    #[account(
        mut,
        seeds = [b"stream_account", stream_account.sender.as_ref(), stream_account.stream_id.as_bytes()],
        bump = stream_account.bump
    )]
    pub stream_account: Account<'info, StreamAccount>,

    #[account(
        mut,
        seeds = [b"stream_vault", stream_account.stream_id.as_bytes()],
        bump = stream_account.vault_bump
    )]
    pub vault_token_account: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = recipient_token_account.mint == stream_account.mint @ PulseError::InvalidMint,
        constraint = recipient_token_account.owner == recipient.key() @ PulseError::Unauthorized
    )]
    pub recipient_token_account: Account<'info, TokenAccount>,

    pub recipient: Signer<'info>,
    pub token_program: Program<'info, Token>,
}

// -----------------------------------------------------------------------------
// STATE SCHEMAS & DATA STRUCTURES
// -----------------------------------------------------------------------------

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Default)]
pub struct SplitRecipient {
    pub wallet: Pubkey,
    pub basis_points: u16,
}

#[account]
pub struct MerchantConfig {
    pub merchant_authority: Pubkey,
    pub settlement_mint: Pubkey,
    pub merchant_id: String,
    pub recipient_count: u8,
    pub bump: u8,
    pub splits: [SplitRecipient; MAX_SPLIT_RECIPIENTS],
}

impl MerchantConfig {
    pub const INIT_SPACE: usize = 32 + 32 + (4 + 32) + 1 + 1 + ((32 + 2) * MAX_SPLIT_RECIPIENTS);
}

#[account]
pub struct StreamAccount {
    pub stream_id: String,
    pub sender: Pubkey,
    pub recipient: Pubkey,
    pub mint: Pubkey,
    pub vault: Pubkey,
    pub total_deposit: u64,
    pub withdrawn_amount: u64,
    pub start_time: i64,
    pub end_time: i64,
    pub is_active: bool,
    pub bump: u8,
    pub vault_bump: u8,
}

impl StreamAccount {
    pub const INIT_SPACE: usize = (4 + 32) + 32 + 32 + 32 + 32 + 8 + 8 + 8 + 8 + 1 + 1 + 1;
}

// -----------------------------------------------------------------------------
// EVENTS
// -----------------------------------------------------------------------------

#[event]
pub struct MerchantConfigCreated {
    pub merchant: Pubkey,
    pub merchant_id: String,
    pub recipient_count: u8,
}

#[event]
pub struct PaymentSettled {
    pub order_id: String,
    pub payer: Pubkey,
    pub merchant: Pubkey,
    pub total_amount: u64,
    pub timestamp: i64,
}

#[event]
pub struct StreamCreated {
    pub stream_id: String,
    pub sender: Pubkey,
    pub recipient: Pubkey,
    pub total_deposit: u64,
    pub start_time: i64,
    pub end_time: i64,
}

#[event]
pub struct FundsWithdrawn {
    pub stream_id: String,
    pub recipient: Pubkey,
    pub amount: u64,
    pub total_withdrawn: u64,
    pub timestamp: i64,
}

// -----------------------------------------------------------------------------
// CUSTOM ERROR CODES
// -----------------------------------------------------------------------------

#[error_code]
pub enum PulseError {
    #[msg("Identifier exceeds maximum allowable length")]
    IdentifierTooLong,
    #[msg("Mismatched remaining split accounts count vs percentages vector")]
    MismatchedSplitAccounts,
    #[msg("Split basis points sum must equal exactly 10,000 (100%)")]
    InvalidSplitBasisPoints,
    #[msg("Payment amount must be greater than zero")]
    ZeroPaymentAmount,
    #[msg("Supplied remaining accounts count does not match merchant config")]
    InvalidSplitAccountsLength,
    #[msg("Unauthorized destination token account for split target")]
    UnauthorizedSplitTarget,
    #[msg("Numerical math overflow encountered during split settlement")]
    MathOverflow,
    #[msg("Invalid stream start time: cannot be in the distant past")]
    InvalidStartTime,
    #[msg("Stream end time must be strictly greater than start time")]
    InvalidEndTime,
    #[msg("The requested payment stream is inactive or terminated")]
    StreamInactive,
    #[msg("Caller is unauthorized to perform this stream action")]
    Unauthorized,
    #[msg("No vested funds currently available for withdrawal")]
    NoAvailableVestedFunds,
    #[msg("Withdrawal request amount must be greater than zero")]
    ZeroWithdrawal,
    #[msg("Token account mint does not match expected settlement mint")]
    InvalidMint,
}