use anchor_lang::prelude::*;

use crate::types::{AccountMetaBorsh, Callback};

// TODO: Add a base_mint, base_wallet, and base_cost_per_byte to the program state for wrapped SOL costs. Then other fees can be in a custom mint.
// TODO: Add the ability to unwrap rewards to fund the hot wallet

/// Program global state for processing randomness requests.
#[account]
#[derive(Debug, InitSpace)]
pub struct State {
    /// The PDA bump.
    pub bump: u8,
    /// The program authority.
    pub authority: Pubkey,
    /// The token mint for the program reward.
    pub mint: Pubkey,
    /// The Switchboard Service responsible for responding to randomness requests
    pub switchboard_service: Pubkey,
    /// Token wallet used for rewards
    pub wallet: Pubkey,
    /// The cost for each randomness request.
    pub cost_per_byte: u64,
    /// The unix timestamp when the cost per byte was last updated.
    pub last_updated: i64,
    /// Reserved for future use.
    pub _ebuf: [u8; 512],
}
impl State {
    /// Returns the size of the function account data in bytes. Includes the discriminator.
    pub fn size() -> usize {
        8 + State::INIT_SPACE
    }

    pub fn request_cost(&self, num_bytes: u8) -> u64 {
        // @DEV - here we can add some lamports if we want to hardcode a priority fee.
        5000u64 + (self.cost_per_byte * u64::from(num_bytes))
    }
}

/// Keypair account used as a fallback for listening to randomness requests.
/// These accounts are ephemeral and are intended to be closed upon completion.
#[account]
#[derive(Debug, Default, InitSpace)]
pub struct RandomnessRequest {
    /// Flag for determining whether the request has been completed.
    pub is_completed: u8,
    pub num_bytes: u8,
    pub user: Pubkey,
    pub escrow: Pubkey,
    pub request_slot: u64,
    pub callback: Callback,
    // #[max_len(512)]
    // pub error_message: String,
}
impl RandomnessRequest {
    pub fn space(callback: &Callback) -> usize {
        let base: usize = 8  // discriminator
            + std::mem::size_of::<RandomnessRequest>();

        msg!(
            "base: {}, ix_data_len: {}, accounts_len: {} * {}",
            base,
            callback.ix_data.len(),
            callback.accounts.len(),
            std::mem::size_of::<AccountMetaBorsh>()
        );

        base
        + (callback.ix_data.len()) // callback ix data len
        + (std::mem::size_of::<AccountMetaBorsh>() * callback.accounts.len())
        // callback accounts len
        // + (512) // error message
    }
}
