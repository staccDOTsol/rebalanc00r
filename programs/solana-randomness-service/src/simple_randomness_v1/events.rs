use crate::*;

use crate::types::Callback;

// We will listen to this event inside of our service
#[event]
#[derive(Clone, Debug)]
pub struct SimpleRandomnessV1RequestedEvent {
    #[index]
    pub callback_pid: Pubkey,
    #[index]
    pub user: Pubkey,
    pub request: Pubkey,
    pub callback: Callback,
    pub num_bytes: u8,
}

#[event]
#[derive(Debug, Clone)]
pub struct SimpleRandomnessV1SettledEvent {
    #[index]
    pub callback_pid: Pubkey,
    #[index]
    pub user: Pubkey,
    pub request: Pubkey,
    pub is_success: bool,
    pub randomness: Vec<u8>,
}

#[event]
#[derive(Debug, Clone)]
pub struct SimpleRandomnessV1CallbackErrorEvent {
    #[index]
    pub callback_pid: Pubkey,
    #[index]
    pub user: Pubkey,
    pub request: Pubkey,
    pub error_message: String,
}
