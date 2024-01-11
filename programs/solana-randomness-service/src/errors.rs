use crate::*;

#[error_code]
#[derive(Eq, PartialEq)]
pub enum RandomnessError {
    #[msg("num_bytes must be greater than 0")]
    MissingNumBytes,
    #[msg("num_bytes must be less than or equal to 32")]
    OverflowNumBytes,
    #[msg("Invalid token account")]
    InvalidEscrow,
    #[msg("User escrow has insufficient funds")]
    InsufficientFunds,
    #[msg("User's callback cannot be executed")]
    InvalidCallback,
    #[msg("Account not found in remaining_accounts")]
    MissingCallbackAccount,
    #[msg("This instruction cannot be invoked with a CPI call")]
    CpiUnauthorized,
    #[msg("The provided request has not been completed yet")]
    RequestStillActive,
    #[msg("The randomness request has already been completed")]
    RequestAlreadyCompleted,
    #[msg("The provided error message exceeded 512 bytes")]
    ErrorMessageOverflow,
}
