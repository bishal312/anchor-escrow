// Imports and dependencies

use crate::errors::EscrowError;
use crate::state::Escrow;
use anchor_lang::prelude::*;

use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{transfer_checked, Mint, TokenAccount, TokenInterface, TransferChecked},
};


// Make Accounts
#[derive(Accounts)]
#[instruction(seed: u64)]
pub struct Make<'info> {
    #[account(mut)]
    pub maker: Signer<'info>,
    #[account(
        init,
        payer=maker,
        space=Escrow::INIT_SPACE + Escrow::DISCRIMINATOR.len(),
        
        //Derives a unique address using the literal string "escrow", the maker's wallet address, and a unique seed number provided by the user. This allows a maker to create multiple distinct escrows.
        seeds=[b"escrow", maker.key().as_ref(), seed.to_le_bytes().as_ref()],

        bump,
    )]

    pub escrow: Account<'info, Escrow>
    #[account(
        mint::token_program=token_program,
    )]
    pub mint_a: InterfaceAccount<'info, Mint>
    #[account(
        mint::token_program=token_program,
    )]
    pub mint_b: InterfaceAccount<'info, Mint>,
    #[account(
        mut,
        associated_token::mint=mint_a,
        associated_token::authority=maker,
        associated_token::token_program=token_program,
    )]
    pub maker_ata_a: InterfaceAccount<'info, TokenAccount>, //Maker's Associated Token Account
    #[account(
        init,
        payer=maker,
        associated_token::mint = mint_a,
        associated_token::authority=escrow,
        associated_token::token_program=token_program,
    )]
    pub vault: InterfaceAccount<'info, TokenAccount>, //This vault acts as the secure holding box where the offered tokens sit.


    // Program references required by Solana and Anchor to initialize accounts and manage token transfers
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,

    // Helper implementation Block
    impl<'info> Make<'info> {
        // populate_escrow: Writes the trade parameters (seed, maker address, token types, requested amount (receive), and the PDA bump) directly into the newly created escrow account space.
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

        //deposite_tokens: Executes a Cross-Program Invocation (CPI) to the Token Program using transfer_checked. It securely moves the specified amount of Token A from the maker's wallet (maker_ata_a) into the escrow's holding vault (vault), verifying token decimals.
        fn deposite_tokens(&mut self, amount: u64) -> Result<()> {
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

    // the main instruction handler
    pub fn handler(ctx: Context<Make>, seed: u64, receive: u64, amount: u64) -> Result<()> {

        // Validation Checks: Uses require_gt! macro to ensure both the amount requested (receive) and the amount deposited (amount) are strictly greater than 0, returning a custom EscrowError::InvalidAmount if invalid.
        require_gt!(receive, 0, EscrowError::InvalidAmount);
        require_gt!(amount, 0, EscrowError::InvalidAmount);

        // State Initialization: Calls populate_escrow to save trade metadata onto the escrow account, passing the automatically generated PDA bump (ctx.bumps.escrow).
        ctx.account
            .populate_escrow(seed, receive, ctx.bumps.escrow)?;

        // Token Transfer: Calls deposite_tokens to transfer the Maker's tokens into the escrow vault.
        ctx.accounts.deposite_tokens(amount)?;
        Ok(())
    }
}