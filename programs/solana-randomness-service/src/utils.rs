use crate::*;
use solana_program::instruction::{get_stack_height, TRANSACTION_LEVEL_STACK_HEIGHT};
use solana_program::sysvar::instructions::{
    load_current_index_checked, load_instruction_at_checked,
};

pub fn wrap_native<'a>(
    system_program: &AccountInfo<'a>,
    token_program: &AccountInfo<'a>,
    native_token_account: &Account<'a, TokenAccount>,
    payer: &AccountInfo<'a>,
    auth_seed: &[&[&[u8]]],
    amount: u64,
) -> anchor_lang::prelude::Result<()> {
    if amount == 0 {
        return Ok(());
    }

    if native_token_account.mint != anchor_spl::token::spl_token::native_mint::ID {
        return Err(error!(RandomnessError::InvalidEscrow));
    }

    // first transfer the SOL to the token account
    let transfer_accounts = anchor_lang::system_program::Transfer {
        from: payer.to_account_info(),
        to: native_token_account.to_account_info(),
    };
    let transfer_ctx = CpiContext::new(system_program.clone(), transfer_accounts);
    anchor_lang::system_program::transfer(transfer_ctx, amount)?;

    // then call sync native which
    let sync_accounts = anchor_spl::token::SyncNative {
        account: native_token_account.to_account_info(),
    };
    let sync_ctx = CpiContext::new_with_signer(token_program.clone(), sync_accounts, auth_seed);
    anchor_spl::token::sync_native(sync_ctx)?;

    Ok(())
}

pub fn transfer<'a>(
    token_program: &AccountInfo<'a>,
    from: &Account<'a, TokenAccount>,
    to: &Account<'a, TokenAccount>,
    authority: &AccountInfo<'a>,
    auth_seed: &[&[&[u8]]],
    amount: u64,
) -> anchor_lang::prelude::Result<()> {
    if amount == 0 {
        return Ok(());
    }
    let cpi_program = token_program.clone();
    let cpi_accounts = anchor_spl::token::Transfer {
        from: from.to_account_info(),
        to: to.to_account_info(),
        authority: authority.clone(),
    };
    let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, auth_seed);
    anchor_spl::token::transfer(cpi_ctx, amount)?;
    Ok(())
}

/// Asserts that the current instruction is not a CPI call. This is to prevent re-entrancy from user provided callbacks.
pub fn assert_not_cpi_call(sysvar_info: &AccountInfo) -> std::result::Result<(), ProgramError> {
    let ix_idx: usize = load_current_index_checked(sysvar_info)?.into();

    // say the tx looks like:
    // ix 0
    //   - ix a
    //   - ix b
    //   - ix c
    // ix 1
    // and we call "load_current_index_checked" from b, we will get 0. And when we
    // load_instruction_at_checked(0), we will get ix 0.
    // tldr; instructions sysvar only stores top-level instructions, never CPI instructions.
    let current_ixn = load_instruction_at_checked(ix_idx.into(), sysvar_info)?;

    // the current ixn must match the flash_* ix. otherwise, it's a CPI. Comparing program_ids is a
    // cheaper way of verifying this property, bc token-lending doesn't allow re-entrancy anywhere.
    if crate::ID != current_ixn.program_id {
        return Err(error!(RandomnessError::CpiUnauthorized).into());
    }

    if get_stack_height() > TRANSACTION_LEVEL_STACK_HEIGHT {
        return Err(error!(RandomnessError::CpiUnauthorized).into());
    }

    Ok(())
}
