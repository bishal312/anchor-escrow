use crate::error::EscrowError;
use crate::state::Escrow;
use anchor_lang::prelude::*;

use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{
        close_account, transfer_checked, CloseAccount, Mint, TokenAccount, TokenInterface,
        TransferChecked,
    },
};

#[derive(Accounts)]
pub struct Take<'info> {
    // The user fulfilling/taking the escrow offer. They sign and pay for new account creations.
    #[account(mut)]
    pub taker: Signer<'info>,

    // The original creator of the escrow. Marked as a SystemAccount because they receive rent back 
    // when accounts close and receive Token B.
    #[account(mut)]
    pub maker: SystemAccount<'info>,

    // The Escrow state PDA holding the trade details.
    #[account(
        mut,
        close = maker, // Closes the escrow account upon completion and refunds remaining rent to the maker
        seeds = [b"escrow", maker.key().as_ref(), escrow.seed.to_le_bytes().as_ref()],
        bump = escrow.bump,
        has_one = maker @ EscrowError::InvalidMaker,   // Validates the maker matches stored state
        has_one = mint_a @ EscrowError::InvalidMintA, // Validates Mint A matches stored state
        has_one = mint_b @ EscrowError::InvalidMintB, // Validates Mint B matches stored state
    )]
    pub escrow: Box<Account<'info, Escrow>>,

    // Mint definitions for Token A and Token B
    pub mint_a: Box<InterfaceAccount<'info, Mint>>,
    pub mint_b: Box<InterfaceAccount<'info, Mint>>,

    // The escrow's token vault holding the Maker's Token A
    #[account(
        mut,
        associated_token::mint = mint_a,
        associated_token::authority = escrow,
        associated_token::token_program = token_program,
    )]
    pub vault: Box<InterfaceAccount<'info, TokenAccount>>,

    // Taker's ATA for Token A (initialized during this instruction to receive Token A)
    #[account(
        init,
        payer = taker,
        associated_token::mint = mint_a,
        associated_token::authority = taker,
        associated_token::token_program = token_program,
    )]
    pub taker_ata_a: Box<InterfaceAccount<'info, TokenAccount>>,

    // Taker's ATA for Token B (from which they will pay the Maker)
    #[account(
       mut,
       associated_token::mint = mint_b,
       associated_token::authority = taker,
       associated_token::token_program = token_program,
    )]
    pub taker_ata_b: Box<InterfaceAccount<'info, TokenAccount>>,

    // Maker's ATA for Token B (initialized during this instruction to receive payment from the Taker)
    #[account(
        init,
        payer = taker,
        associated_token::mint = mint_b,
        associated_token::authority = maker,
        associated_token::token_program = token_program,
    )]
    pub maker_ata_b: Box<InterfaceAccount<'info, TokenAccount>>,

    // Required system programs for account creation and token transfers
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

impl<'info> Take<'info> {
    // Step 1: Transfers Token B from the Taker to the Maker's Token B ATA
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
            self.escrow.receive, // Amount of Token B the maker requested
            self.mint_b.decimals,
        )?;
        Ok(())
    }

    // Step 2: Withdraws Token A from the escrow vault to the Taker, then closes the vault account
    fn withdraw_and_close_vault(&mut self) -> Result<()> {
        // Construct PDA signing seeds so the Escrow PDA can authorize the transfer and account closure
        let signer_seeds: [&[&[u8]]; 1] = [&[
            b"escrow",
            self.maker.to_account_info().key.as_ref(),
            &self.escrow.seed.to_le_bytes()[..],
            &[self.escrow.bump],
        ]];

        // Transfer Token A out of the vault to the Taker's ATA A
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
            self.vault.amount, // Withdraws all remaining Token A in the vault
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

// Main execution handler called when a user triggers the "Take" instruction
pub fn handler(ctx: Context<Take>) -> Result<()> {
    // 1. Pay the maker their requested Token B
    ctx.accounts.transfer_to_maker()?;
    
    // 2. Give the taker their Token A from the vault and clean up accounts
    ctx.accounts.withdraw_and_close_vault()?;
    Ok(())
}