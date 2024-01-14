use crate::*;

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
