use crate::error::EscrowError;
use crate::state::Escrow;
use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{transfer_checked, Mint, TokenAccount, TokenInterface, TransferChecked},
};

#[derive(Accounts)]
#[instruction(seed: u64)]
pub struct Make<'info> {
    // The user creating the escrow offer (Maker). They sign the transaction and pay rent for account creations.
    #[account(mut)]
    pub maker: Signer<'info>,

    // The Escrow state PDA that stores trade metadata (maker, mints, requested amount, bump).
    #[account(
        init,
        payer = maker,
        space = Escrow::INIT_SPACE + Escrow::DISCRIMINATOR.len(),
        // Derives a deterministic PDA using "escrow", the maker's public key, and a unique seed
        seeds = [b"escrow", maker.key().as_ref(), seed.to_le_bytes().as_ref()],
        bump,
    )]
    pub escrow: Account<'info, Escrow>,

    // Token mint definition for Token A (the token being offered by the maker)
    #[account(mint::token_program = token_program)]
    pub mint_a: InterfaceAccount<'info, Mint>,

    // Token mint definition for Token B (the token requested in return)
    #[account(mint::token_program = token_program)]
    pub mint_b: InterfaceAccount<'info, Mint>,

    // Maker's Associated Token Account for Token A (from which offered tokens are withdrawn)
    #[account(
        mut,
        associated_token::mint = mint_a,
        associated_token::authority = maker,
        associated_token::token_program = token_program,
    )]
    pub maker_ata_a: InterfaceAccount<'info, TokenAccount>,

    // The escrow's token vault (ATA owned by the escrow PDA) where offered Token A is safely locked
    #[account(
        init,
        payer = maker,
        associated_token::mint = mint_a,
        associated_token::authority = escrow,
        associated_token::token_program = token_program,
    )]
    pub vault: InterfaceAccount<'info, TokenAccount>,

    // Required system and token program references
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

impl<'info> Make<'info> {
    // Writes trade parameters and metadata directly into the newly initialized escrow account
    fn populate_escrow(&mut self, seed: u64, amount: u64, bump: u8) -> Result<()> {
        self.escrow.set_inner(Escrow {
            seed,
            maker: self.maker.key(),
            mint_a: self.mint_a.key(),
            mint_b: self.mint_b.key(),
            receive: amount,
            bump,
        });
        Ok(())
    }

    // Executes a Cross-Program Invocation (CPI) to transfer Token A from the maker's wallet into the escrow vault
    fn deposit_tokens(&mut self, amount: u64) -> Result<()> {
        transfer_checked(
            CpiContext::new(
                self.token_program.key(),
                TransferChecked {
                    from: self.maker_ata_a.to_account_info(),
                    mint: self.mint_a.to_account_info(),
                    to: self.vault.to_account_info(),
                    authority: self.maker.to_account_info(),
                },
            ),
            amount,
            self.mint_a.decimals,
        )?;
        Ok(())
    }
}

// Main execution handler called when a user triggers the Make instruction
pub fn handler(ctx: Context<Make>, seed: u64, receive: u64, amount: u64) -> Result<()> {
    // Ensure that both the requested receive amount and the deposit amount are strictly greater than zero
    require_gt!(receive, 0, EscrowError::InvalidAmount);
    require_gt!(amount, 0, EscrowError::InvalidAmount);

    // Save the escrow trade configuration details onto the escrow state PDA
    ctx.accounts
        .populate_escrow(seed, receive, ctx.bumps.escrow)?;
        
    // Lock the maker's Token A inside the escrow vault
    ctx.accounts.deposit_tokens(amount)?;
    
    Ok(())
}