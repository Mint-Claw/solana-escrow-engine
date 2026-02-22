use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};

declare_id!("DgS6gJZToqri3RN6LmvMYNxAMKNnipHdEDAVyU5QFE6t");

#[program]
pub mod solana_escrow_engine {
    use super::*;

    /// Creates a new escrow with buyer depositing funds
    pub fn create_escrow(
        ctx: Context<CreateEscrow>,
        amount: u64,
        timeout_duration: i64,
        description: String,
    ) -> Result<()> {
        let escrow = &mut ctx.accounts.escrow;
        let clock = Clock::get()?;
        
        // Initialize escrow account
        escrow.buyer = ctx.accounts.buyer.key();
        escrow.seller = Pubkey::default(); // Will be set when seller accepts
        escrow.mint = ctx.accounts.mint.key();
        escrow.amount = amount;
        escrow.created_at = clock.unix_timestamp;
        escrow.timeout_at = clock.unix_timestamp + timeout_duration;
        escrow.state = EscrowState::Created;
        escrow.description = description;
        escrow.bump = ctx.bumps.escrow;

        // Transfer funds to escrow vault
        let cpi_accounts = Transfer {
            from: ctx.accounts.buyer_token_account.to_account_info(),
            to: ctx.accounts.vault_token_account.to_account_info(),
            authority: ctx.accounts.buyer.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
        token::transfer(cpi_ctx, amount)?;

        msg!("Escrow created: {} tokens deposited", amount);
        Ok(())
    }

    /// Seller accepts the escrow and commits to delivery
    pub fn accept_escrow(ctx: Context<AcceptEscrow>) -> Result<()> {
        let escrow = &mut ctx.accounts.escrow;
        
        require!(escrow.state == EscrowState::Created, EscrowError::InvalidState);
        require!(escrow.seller == Pubkey::default(), EscrowError::AlreadyAccepted);
        
        escrow.seller = ctx.accounts.seller.key();
        escrow.state = EscrowState::Accepted;
        
        let clock = Clock::get()?;
        escrow.accepted_at = clock.unix_timestamp;
        
        msg!("Escrow accepted by seller: {}", ctx.accounts.seller.key());
        Ok(())
    }

    /// Buyer confirms receipt and releases funds to seller
    pub fn confirm_delivery(ctx: Context<ConfirmDelivery>) -> Result<()> {
        let escrow = &mut ctx.accounts.escrow;
        
        require!(escrow.state == EscrowState::Accepted, EscrowError::InvalidState);
        require!(escrow.buyer == ctx.accounts.buyer.key(), EscrowError::UnauthorizedBuyer);
        
        // Transfer funds from vault to seller
        let seeds = &[
            b"escrow",
            escrow.buyer.as_ref(),
            escrow.mint.as_ref(),
            &[escrow.bump],
        ];
        let signer = &[&seeds[..]];
        
        let cpi_accounts = Transfer {
            from: ctx.accounts.vault_token_account.to_account_info(),
            to: ctx.accounts.seller_token_account.to_account_info(),
            authority: escrow.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer);
        token::transfer(cpi_ctx, escrow.amount)?;

        escrow.state = EscrowState::Completed;
        let clock = Clock::get()?;
        escrow.completed_at = clock.unix_timestamp;
        
        msg!("Delivery confirmed, funds released to seller");
        Ok(())
    }

    /// Cancel escrow before seller acceptance (buyer gets refund)
    pub fn cancel_escrow(ctx: Context<CancelEscrow>) -> Result<()> {
        let escrow = &mut ctx.accounts.escrow;
        
        require!(escrow.state == EscrowState::Created, EscrowError::InvalidState);
        require!(escrow.buyer == ctx.accounts.buyer.key(), EscrowError::UnauthorizedBuyer);
        
        // Transfer funds back to buyer
        let seeds = &[
            b"escrow",
            escrow.buyer.as_ref(),
            escrow.mint.as_ref(),
            &[escrow.bump],
        ];
        let signer = &[&seeds[..]];
        
        let cpi_accounts = Transfer {
            from: ctx.accounts.vault_token_account.to_account_info(),
            to: ctx.accounts.buyer_token_account.to_account_info(),
            authority: escrow.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer);
        token::transfer(cpi_ctx, escrow.amount)?;

        escrow.state = EscrowState::Cancelled;
        let clock = Clock::get()?;
        escrow.cancelled_at = clock.unix_timestamp;
        
        msg!("Escrow cancelled, funds returned to buyer");
        Ok(())
    }

    /// Resolve timeout - automatically release funds if timeout passed
    pub fn resolve_timeout(ctx: Context<ResolveTimeout>) -> Result<()> {
        let escrow = &mut ctx.accounts.escrow;
        let clock = Clock::get()?;
        
        require!(escrow.state == EscrowState::Accepted, EscrowError::InvalidState);
        require!(clock.unix_timestamp >= escrow.timeout_at, EscrowError::TimeoutNotReached);
        
        // Transfer funds from vault to seller (timeout favors seller)
        let seeds = &[
            b"escrow",
            escrow.buyer.as_ref(),
            escrow.mint.as_ref(),
            &[escrow.bump],
        ];
        let signer = &[&seeds[..]];
        
        let cpi_accounts = Transfer {
            from: ctx.accounts.vault_token_account.to_account_info(),
            to: ctx.accounts.seller_token_account.to_account_info(),
            authority: escrow.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer);
        token::transfer(cpi_ctx, escrow.amount)?;

        escrow.state = EscrowState::TimedOut;
        escrow.completed_at = clock.unix_timestamp;
        
        msg!("Timeout resolved, funds released to seller");
        Ok(())
    }
}

#[derive(Accounts)]
#[instruction(amount: u64, timeout_duration: i64, description: String)]
pub struct CreateEscrow<'info> {
    #[account(mut)]
    pub buyer: Signer<'info>,
    
    #[account(
        init,
        payer = buyer,
        space = 8 + Escrow::LEN,
        seeds = [b"escrow", buyer.key().as_ref(), mint.key().as_ref()],
        bump
    )]
    pub escrow: Account<'info, Escrow>,
    
    pub mint: Account<'info, Mint>,
    
    #[account(
        mut,
        constraint = buyer_token_account.owner == buyer.key(),
        constraint = buyer_token_account.mint == mint.key(),
    )]
    pub buyer_token_account: Account<'info, TokenAccount>,
    
    #[account(
        init,
        payer = buyer,
        token::mint = mint,
        token::authority = escrow,
        seeds = [b"vault", escrow.key().as_ref()],
        bump
    )]
    pub vault_token_account: Account<'info, TokenAccount>,
    
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct AcceptEscrow<'info> {
    #[account(mut)]
    pub seller: Signer<'info>,
    
    #[account(
        mut,
        seeds = [b"escrow", escrow.buyer.as_ref(), escrow.mint.as_ref()],
        bump = escrow.bump
    )]
    pub escrow: Account<'info, Escrow>,
}

#[derive(Accounts)]
pub struct ConfirmDelivery<'info> {
    #[account(mut)]
    pub buyer: Signer<'info>,
    
    #[account(
        mut,
        seeds = [b"escrow", escrow.buyer.as_ref(), escrow.mint.as_ref()],
        bump = escrow.bump
    )]
    pub escrow: Account<'info, Escrow>,
    
    #[account(
        mut,
        seeds = [b"vault", escrow.key().as_ref()],
        bump
    )]
    pub vault_token_account: Account<'info, TokenAccount>,
    
    #[account(
        mut,
        constraint = seller_token_account.owner == escrow.seller,
        constraint = seller_token_account.mint == escrow.mint,
    )]
    pub seller_token_account: Account<'info, TokenAccount>,
    
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct CancelEscrow<'info> {
    #[account(mut)]
    pub buyer: Signer<'info>,
    
    #[account(
        mut,
        seeds = [b"escrow", escrow.buyer.as_ref(), escrow.mint.as_ref()],
        bump = escrow.bump
    )]
    pub escrow: Account<'info, Escrow>,
    
    #[account(
        mut,
        seeds = [b"vault", escrow.key().as_ref()],
        bump
    )]
    pub vault_token_account: Account<'info, TokenAccount>,
    
    #[account(
        mut,
        constraint = buyer_token_account.owner == buyer.key(),
        constraint = buyer_token_account.mint == escrow.mint,
    )]
    pub buyer_token_account: Account<'info, TokenAccount>,
    
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct ResolveTimeout<'info> {
    /// Anyone can call this to resolve timeout
    pub resolver: Signer<'info>,
    
    #[account(
        mut,
        seeds = [b"escrow", escrow.buyer.as_ref(), escrow.mint.as_ref()],
        bump = escrow.bump
    )]
    pub escrow: Account<'info, Escrow>,
    
    #[account(
        mut,
        seeds = [b"vault", escrow.key().as_ref()],
        bump
    )]
    pub vault_token_account: Account<'info, TokenAccount>,
    
    #[account(
        mut,
        constraint = seller_token_account.owner == escrow.seller,
        constraint = seller_token_account.mint == escrow.mint,
    )]
    pub seller_token_account: Account<'info, TokenAccount>,
    
    pub token_program: Program<'info, Token>,
}

#[account]
pub struct Escrow {
    pub buyer: Pubkey,
    pub seller: Pubkey,
    pub mint: Pubkey,
    pub amount: u64,
    pub state: EscrowState,
    pub created_at: i64,
    pub accepted_at: i64,
    pub completed_at: i64,
    pub cancelled_at: i64,
    pub timeout_at: i64,
    pub description: String,
    pub bump: u8,
}

impl Escrow {
    pub const LEN: usize = 32 + 32 + 32 + 8 + 1 + 8 + 8 + 8 + 8 + 8 + (4 + 200) + 1;
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq, Eq)]
pub enum EscrowState {
    Created,
    Accepted,
    Completed,
    Cancelled,
    TimedOut,
}

#[error_code]
pub enum EscrowError {
    #[msg("Invalid escrow state for this operation")]
    InvalidState,
    #[msg("Escrow has already been accepted by a seller")]
    AlreadyAccepted,
    #[msg("Only the buyer can perform this action")]
    UnauthorizedBuyer,
    #[msg("Only the seller can perform this action")]
    UnauthorizedSeller,
    #[msg("Timeout has not been reached yet")]
    TimeoutNotReached,
}