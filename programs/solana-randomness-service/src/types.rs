use crate::*;

// #[derive(Debug, Clone, AnchorSerialize, AnchorDeserialize, InitSpace)]
// pub struct CallbackBorsh {
//     /// The program ID of the callback program being invoked.
//     pub program_id: Pubkey,
//     /// The accounts being used in the callback instruction.
//     #[max_len(32)]
//     pub accounts: Vec<AccountMetaBorsh>,
//     /// The serialized instruction data.
//     #[max_len(1024)]
//     pub ix_data: Vec<u8>,
// }

// #[derive(Debug, Clone, Copy, Default, AnchorSerialize, AnchorDeserialize, InitSpace)]
// pub struct AccountMetaBorsh {
//     pub pubkey: Pubkey,
//     pub is_signer: bool,
//     pub is_writable: bool,
// }
// impl From<AccountMetaBorsh> for anchor_lang::prelude::AccountMeta {
//     fn from(item: AccountMetaBorsh) -> Self {
//         Self {
//             pubkey: item.pubkey,
//             is_signer: item.is_signer,
//             is_writable: item.is_writable,
//         }
//     }
// }
// impl From<&AccountMetaBorsh> for anchor_lang::prelude::AccountMeta {
//     fn from(item: &AccountMetaBorsh) -> Self {
//         Self {
//             pubkey: item.pubkey,
//             is_signer: item.is_signer,
//             is_writable: item.is_writable,
//         }
//     }
// }

#[zero_copy(unsafe)]
#[derive(Debug)]
#[repr(packed)]
pub struct AccountMetaZC {
    pub pubkey: Pubkey,
    pub is_signer: u8,
    pub is_writable: u8,
}
impl AccountMetaZC {
    pub fn new(pubkey: Pubkey, is_signer: bool) -> Self {
        Self {
            pubkey,
            is_signer: is_signer.to_u8(),
            is_writable: 1,
        }
    }

    pub fn new_readonly(pubkey: Pubkey, is_signer: bool) -> Self {
        Self {
            pubkey,
            is_signer: is_signer.to_u8(),
            is_writable: 0,
        }
    }
}

#[derive(Clone, Debug, Default, AnchorSerialize, AnchorDeserialize)]
pub struct AccountMetaBorsh {
    pub pubkey: Pubkey,
    pub is_signer: bool,
    pub is_writable: bool,
}
impl AccountMetaBorsh {
    pub fn new(pubkey: Pubkey, is_signer: bool) -> Self {
        Self {
            pubkey,
            is_signer,
            is_writable: true,
        }
    }

    pub fn new_readonly(pubkey: Pubkey, is_signer: bool) -> Self {
        Self {
            pubkey,
            is_signer,
            is_writable: false,
        }
    }
}

#[zero_copy(unsafe)]
#[derive(Debug)]
#[repr(packed)]
pub struct CallbackZC {
    /// The program ID of the callback program being invoked.
    pub program_id: Pubkey,
    /// The accounts being used in the callback instruction.
    pub accounts: [AccountMetaZC; 32],
    /// The number of accounts used in the callback
    pub accounts_len: u32,
    /// The serialized instruction data.
    pub ix_data: [u8; 1024],
    /// The number of serialized bytes in the instruction data.
    pub ix_data_len: u32,
}

#[derive(Clone, Debug, Default, AnchorSerialize, AnchorDeserialize)]
pub struct Callback {
    pub program_id: Pubkey,
    pub accounts: Vec<AccountMetaBorsh>,
    pub ix_data: Vec<u8>,
}
// impl From<Callback> for CallbackZC {
//     fn from(val: Callback) -> Self {
//         let mut cb: CallbackZC = Default::default();
//         cb.program_id = val.program_id;
//         cb.ix_data_len = val.ix_data.len().try_into().unwrap();
//         cb.ix_data[..val.ix_data.len()].clone_from_slice(val.ix_data.as_slice());
//         cb.accounts_len = val.accounts.len().try_into().unwrap();
//         for i in 0..val.accounts.len() {
//             cb.accounts[i] = AccountMetaZC {
//                 pubkey: val.accounts[i].pubkey,
//                 is_signer: val.accounts[i].is_signer.to_u8(),
//                 is_writable: val.accounts[i].is_writable.to_u8(),
//             };
//         }
//         cb
//     }
// }

impl Into<Callback> for CallbackZC {
    fn into(self) -> Callback {
        let mut accounts = Vec::with_capacity(self.accounts_len as usize);
        for i in 0..self.accounts_len as usize {
            accounts.push(AccountMetaBorsh {
                pubkey: self.accounts[i].pubkey,
                is_signer: self.accounts[i].is_signer.to_bool(),
                is_writable: self.accounts[i].is_writable.to_bool(),
            });
        }

        let ix_data = self.ix_data[..self.ix_data_len as usize].to_vec();

        Callback {
            program_id: self.program_id,
            accounts,
            ix_data,
        }
    }
}

impl Into<CallbackZC> for Callback {
    fn into(self) -> CallbackZC {
        let mut cb: CallbackZC = Default::default();
        cb.program_id = self.program_id;
        cb.ix_data_len = self.ix_data.len().try_into().unwrap();
        cb.ix_data[..self.ix_data.len()].clone_from_slice(self.ix_data.as_slice());
        cb.accounts_len = self.accounts.len().try_into().unwrap();
        for i in 0..self.accounts.len() {
            cb.accounts[i] = AccountMetaZC {
                pubkey: self.accounts[i].pubkey,
                is_signer: self.accounts[i].is_signer.to_u8(),
                is_writable: self.accounts[i].is_writable.to_u8(),
            };
        }
        cb
    }
}

impl Default for CallbackZC {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
impl From<AccountMetaZC> for AccountMeta {
    fn from(val: AccountMetaZC) -> Self {
        AccountMeta {
            pubkey: val.pubkey,
            is_signer: val.is_signer.to_bool(),
            is_writable: val.is_writable.to_bool(),
        }
    }
}
impl From<&AccountMetaZC> for AccountMeta {
    fn from(val: &AccountMetaZC) -> Self {
        AccountMeta {
            pubkey: val.pubkey,
            is_signer: val.is_signer.to_bool(),
            is_writable: val.is_writable.to_bool(),
        }
    }
}

impl From<AccountMeta> for AccountMetaZC {
    fn from(val: AccountMeta) -> Self {
        AccountMetaZC {
            pubkey: val.pubkey,
            is_signer: val.is_signer.to_u8(),
            is_writable: val.is_writable.to_u8(),
        }
    }
}
impl From<&AccountMeta> for AccountMetaZC {
    fn from(val: &AccountMeta) -> Self {
        AccountMetaZC {
            pubkey: val.pubkey,
            is_signer: val.is_signer.to_u8(),
            is_writable: val.is_writable.to_u8(),
        }
    }
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

pub trait ToU8 {
    fn to_u8(&self) -> u8;
}
impl ToU8 for bool {
    fn to_u8(&self) -> u8 {
        if *self {
            1
        } else {
            0
        }
    }
}
impl ToU8 for &bool {
    fn to_u8(&self) -> u8 {
        if **self {
            1
        } else {
            0
        }
    }
}
pub trait ToBool {
    fn to_bool(&self) -> bool;
}

impl ToBool for u8 {
    fn to_bool(&self) -> bool {
        !matches!(*self, 0)
    }
}
impl ToBool for &u8 {
    fn to_bool(&self) -> bool {
        !matches!(**self, 0)
    }
}
