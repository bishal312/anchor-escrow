use crate::{error::EscrowError, state::Escrow};
use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{
        close_account, transfer_checked, CloseAccount, Mint, TokenAccount, TokenInterface,
        TransferChecked,
    },
};

#[derive(Accounts)]
pub struct Refund<'info> {
    // The original creator of the escrow (Maker). They must sign to cancel the trade 
    // and will receive rent lamports refunded when accounts close.
    #[account(mut)]
    pub maker: Signer<'info>,

    // The Escrow state PDA tracking the active trade.
    #[account(
        mut,
        close = maker, // Closes the escrow account upon refund and returns rent lamports to the maker
        seeds = [b"escrow", maker.key().as_ref(), escrow.seed.to_le_bytes().as_ref()],
        bump = escrow.bump,
        // Security checks ensuring the signer is the original maker and Mint A matches state
        has_one = maker @ EscrowError::InvalidMaker,
        has_one = mint_a @ EscrowError::InvalidMintA,
    )]
    pub escrow: Box<Account<'info, Escrow>>,

    // The token mint definition for Token A
    #[account(mint::token_program = token_program)]
    pub mint_a: Box<InterfaceAccount<'info, Mint>>,

    // The escrow's token vault holding the locked Token A
    #[account(
        mut,
        associated_token::mint = mint_a,
        associated_token::authority = escrow,
        associated_token::token_program = token_program
    )]
    pub vault: Box<InterfaceAccount<'info, TokenAccount>>,

    // Maker's Associated Token Account for Token A (initialized here if it doesn't exist, 
    // allowing the refunded tokens to flow safely back into their wallet)
    #[account(
        init,
        payer = maker,
        associated_token::mint = mint_a,
        associated_token::authority = maker,
        associated_token::token_program = token_program
    )]
    pub maker_ata_a: Box<InterfaceAccount<'info, TokenAccount>>,

    // Required system and token program references
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

impl<'info> Refund<'info> {
    // Withdraws locked Token A from the escrow vault back to the maker, then closes the vault and escrow accounts
    fn withdraw_and_close_vault(&mut self) -> Result<()> {
        // Construct PDA signing seeds so the Escrow PDA can authorize the transfer and account closures
        let signer_seeds: [&[&[u8]]; 1] = [&[
            b"escrow",
            self.maker.to_account_info().key.as_ref(),
            &self.escrow.seed.to_le_bytes()[..],
            &[self.escrow.bump],
        ]];

        // Transfer all remaining Token A out of the escrow vault back into the maker's Token A ATA
        transfer_checked(
            CpiContext::new_with_signer(
                self.token_program.key(),
                TransferChecked {
                    from: self.vault.to_account_info(),
                    to: self.maker_ata_a.to_account_info(),
                    mint: self.mint_a.to_account_info(),
                    authority: self.escrow.to_account_info(),
                },
                &signer_seeds,
            ),
            self.vault.amount, // Withdraws the full token balance currently sitting in the vault
            self.mint_a.decimals,
        )?;

        // Close the empty vault token account and send its remaining rent lamports back to the maker
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

// Main execution handler for the Refund instruction
pub fn handler(ctx: Context<Refund>) -> Result<()> {
    // Return locked tokens to the maker and clean up all escrow-related accounts
    ctx.accounts.withdraw_and_close_vault()?;

    Ok(())
}