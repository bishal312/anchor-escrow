// Imports and dependencies

use crate::error::EscrowError;
use crate::state::Escrow;
use anchor_lang::prelude::*;

use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{transfer_checked, Mint, TokenAccount, TokenInterface, TransferChecked},
};


// Make Accounts
#[derive(Accounts)]
pub struct Take<'info> {
    // The user fulfilling the escrow offer. They sign the transaction and pay rent for new accounts.
    #[account(mut)]
    pub taker: Signer<'info>,

    // The original creator of the escrow. Marked as a SystemAccount because they will receive 
    // the rent lamports when the escrow and vault accounts are closed.
    #[account(mut)]
    pub maker: SystemAccount<'info>,

    // The Escrow state PDA holding all trade rules and metadata.
    #[account(
        mut,
        close = maker, // Closes this escrow account upon completion and refunds remaining rent to the maker
        seeds = [b"escrow", maker.key().as_ref(), escrow.seed.to_le_bytes().as_ref()],
        bump = escrow.bump,
        // Security constraints ensuring the accounts passed match what was originally saved in the escrow state
        has_one = maker @ EscrowError::InvalidMaker,
        has_one = mint_a @ EscrowError::InvalidMintA,
        has_one = mint_b @ EscrowError::InvalidMintB,
    )]
    pub escrow: Box<Account<'info, Escrow>>,

    // Mint definitions for Token A (offered) and Token B (requested)
    pub mint_a: Box<InterfaceAccount<'info, Mint>>,
    pub mint_b: Box<InterfaceAccount<'info, Mint>>,

    // The token vault owned by the escrow PDA that holds the Maker's Token A
    #[account(
        mut,
        associated_token::mint = mint_a,
        associated_token::authority = escrow,
        associated_token::token_program = token_program,
    )]
    pub vault: Box<InterfaceAccount<'info, TokenAccount>>,

    // Taker's Associated Token Account for Token A (initialized here to receive the offered tokens)
    #[account(
        init,
        payer = taker,
        associated_token::mint = mint_a,
        associated_token::authority = taker,
        associated_token::token_program = token_program,
    )]
    pub taker_ata_a: Box<InterfaceAccount<'info, TokenAccount>>,

    // Taker's Associated Token Account for Token B (from which payment will be sent to the maker)
    #[account(
       mut,
       associated_token::mint = mint_b,
       associated_token::authority = taker,
       associated_token::token_program = token_program,
    )]
    pub taker_ata_b: Box<InterfaceAccount<'info, TokenAccount>>,

    // Maker's Associated Token Account for Token B (initialized here to receive payment from the taker)
    #[account(
        init,
        payer = taker,
        associated_token::mint = mint_b,
        associated_token::authority = maker,
        associated_token::token_program = token_program,
    )]
    pub maker_ata_b: Box<InterfaceAccount<'info, TokenAccount>>,

    // Required system and token program references
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

impl<'info> Take<'info> {
    // Step 1: Transfers Token B from the Taker's wallet to the Maker's Token B ATA
    fn transfer_to_maker(&mut self) -> Result<()> {
        transfer_checked(
            CpiContext::new(
                self.token_program.key(),
                TransferChecked {
                    from: self.taker_ata_b.to_account_info(),
                    to: self.maker_ata_b.to_account_info(),
                    mint: self.mint_b.to_account_info(),
                    authority: self.taker.to_account_info(),
                },
            ),
            self.escrow.receive, // The exact amount of Token B requested by the maker
            self.mint_b.decimals,
        )?;
        Ok(())
    }

    // Step 2: Withdraws Token A from the escrow vault to the Taker, then cleans up accounts
    fn withdraw_and_close_vault(&mut self) -> Result<()> {
        // Construct the PDA signing seeds so the program can authorize actions on behalf of the Escrow PDA
        let signer_seeds: [&[&[u8]]; 1] = [&[
            b"escrow",
            self.maker.to_account_info().key.as_ref(),
            &self.escrow.seed.to_le_bytes()[..],
            &[self.escrow.bump],
        ]];

        // Transfer all Token A out of the escrow vault into the Taker's Token A ATA using PDA signature
        transfer_checked(
            CpiContext::new_with_signer(
                self.token_program.key(),
                TransferChecked {
                    from: self.vault.to_account_info(),
                    to: self.taker_ata_a.to_account_info(),
                    mint: self.mint_a.to_account_info(),
                    authority: self.escrow.to_account_info(),
                },
                &signer_seeds,
            ),
            self.vault.amount, // Withdraws the full balance available in the vault
            self.mint_a.decimals,
        )?;

        // Close the empty vault token account and return its rent lamports back to the maker
        close_account(CpiContext::new_with_signer(
            self.token_program.key(),
            CloseAccount {
                account: self.vault.to_account_info(),
                authority: self.escrow.to_account_info(),
                destination: self.maker.to_account_info(),
            },
            &signer_seeds,
        ))?;
        Ok(())
    }
}

// Main execution handler for the Take instruction
pub fn handler(ctx: Context<Take>) -> Result<()> {
    // 1. Send the requested Token B payment from the taker to the maker
    ctx.accounts.transfer_to_maker()?;
    
    // 2. Release Token A to the taker, close the vault, and refund rent to the maker
    ctx.accounts.withdraw_and_close_vault()?;
    Ok(())
}