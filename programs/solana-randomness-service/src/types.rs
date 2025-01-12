use anchor_lang::prelude::*;

#[derive(Clone, Debug, Default, AnchorSerialize, AnchorDeserialize, InitSpace)]
pub struct AccountMetaBorsh {
    pub pubkey: Pubkey,
    pub is_signer: bool,
    pub is_writable: bool,
}
impl From<AccountMeta> for AccountMetaBorsh {
    fn from(value: AccountMeta) -> Self {
        Self {
            pubkey: value.pubkey,
            is_signer: value.is_signer,
            is_writable: value.is_writable,
        }
    }
}

#[derive(Clone, Debug, Default, AnchorSerialize, AnchorDeserialize, InitSpace)]
pub struct Callback {
    pub program_id: Pubkey,
    #[max_len(32)]
    pub accounts: Vec<AccountMetaBorsh>,
    #[max_len(1024)]
    pub ix_data: Vec<u8>,
}

impl From<AccountMetaBorsh> for AccountMeta {
    fn from(val: AccountMetaBorsh) -> Self {
        AccountMeta {
            pubkey: val.pubkey,
            is_signer: val.is_signer,
            is_writable: val.is_writable,
        }
    }
}
impl From<&AccountMetaBorsh> for AccountMeta {
    fn from(val: &AccountMetaBorsh) -> Self {
        AccountMeta {
            pubkey: val.pubkey,
            is_signer: val.is_signer,
            is_writable: val.is_writable,
        }
    }
}
