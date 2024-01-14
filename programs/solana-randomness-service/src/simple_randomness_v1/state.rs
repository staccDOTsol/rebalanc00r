use crate::*;

/// Keypair account used as a fallback for listening to randomness requests.
/// These accounts are ephemeral and are intended to be closed upon completion.
#[account]
#[derive(Debug, Default, InitSpace)]
pub struct SimpleRandomnessV1Account {
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
impl SimpleRandomnessV1Account {
    pub fn space(callback: &Callback) -> usize {
        let base: usize = 8  // discriminator
            + std::mem::size_of::<SimpleRandomnessV1Account>();

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
