use anchor_lang::prelude::*;

#[error_code]
pub enum EscrowError {
    #[msg("Invalid amount")]
    InvalidAmount,
    #[msg("Invalid maker")]
    InvalidMaker,
    #[msg("Invalid min a")]
    InvalidMintA,
    #[msg("Invalid mint b")]
    InvalidMintB,
}