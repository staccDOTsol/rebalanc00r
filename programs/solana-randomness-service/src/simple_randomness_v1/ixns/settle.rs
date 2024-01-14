use crate::*;

pub use std::collections::HashMap;

#[derive(Accounts)]
pub struct SimpleRandomnessV1Settle<'info> {
    /// CHECK:
    #[account(mut)]
    pub user: AccountInfo<'info>,

    #[account(
        mut,
        close = user,
        has_one = user,
        has_one = escrow,
        constraint = request.callback.program_id == callback_pid.key() @ RandomnessError::IncorrectCallbackProgramId,
    )]
    pub request: Box<Account<'info, SimpleRandomnessV1Account>>,

    #[account(
        mut,
        constraint = escrow.is_native() && escrow.owner == state.key(),
    )]
    pub escrow: Box<Account<'info, TokenAccount>>,

    #[account(
        seeds = [b"STATE"],
        bump = state.bump,
        has_one = wallet,
        has_one = switchboard_service,
    )]
    pub state: Box<Account<'info, State>>,

    #[account(
        mut,
        constraint = wallet.is_native() && wallet.owner == state.key(),
    )]
    pub wallet: Box<Account<'info, TokenAccount>>,

    // SWITCHBOARD VALIDATION
    #[account(
        constraint = switchboard_function.load()?.validate_service(
            &switchboard_service,
            &enclave_signer.to_account_info(),
        )?
    )]
    pub switchboard_function: AccountLoader<'info, FunctionAccountData>,
    pub switchboard_service: Box<Account<'info, FunctionServiceAccountData>>,
    pub enclave_signer: Signer<'info>,

    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,

    /// The account that pays for the randomness request
    #[account(mut)]
    pub payer: Signer<'info>,

    /// CHECK: todo
    pub callback_pid: AccountInfo<'info>,
    /// CHECK: todo
    #[account(
        address = SYSVAR_INSTRUCTIONS_ID,
    )]
    pub instructions_sysvar: AccountInfo<'info>,
}

impl<'info> SimpleRandomnessV1Settle<'info> {
    pub fn validate(&self, ctx: &Ctx<Self>, randomness: &[u8]) -> anchor_lang::Result<()> {
        msg!("Checking this ixn is not a CPI call ...");

        // Verify this method was not called from a CPI
        assert_not_cpi_call(&ctx.accounts.instructions_sysvar)?;

        let num_bytes = randomness.len();
        if num_bytes == 0 || num_bytes > 32 {
            return Err(error!(RandomnessError::InvalidNumberOfBytes));
        }

        Ok(())
    }

    pub fn actuate(ctx: &mut Ctx<'_, 'info, Self>, randomness: Vec<u8>) -> anchor_lang::Result<()> {
        // Need to make sure the payer is not included in the callback as a writeable account. Otherwise, the payer could be drained of funds.
        for account in ctx.accounts.request.callback.accounts
            [..ctx.accounts.request.callback.accounts.len()]
            .iter()
        {
            if account.pubkey == ctx.accounts.payer.key() && account.is_writable {
                // TODO: We should still transfer funds and close the request without invoking the callback. Wasting our time.
                return Err(error!(RandomnessError::InvalidCallback));
            }
        }

        // Transfer reward (all funds) to the program_state
        let cost = ctx
            .accounts
            .state
            .request_cost(ctx.accounts.request.num_bytes);

        // verify the escrow has enough funds
        if cost > ctx.accounts.escrow.amount {
            return Err(error!(RandomnessError::InsufficientFunds));
        }

        if ctx.accounts.escrow.amount > 0 {
            transfer(
                &ctx.accounts.token_program.to_account_info(),
                &ctx.accounts.escrow,
                &ctx.accounts.wallet,
                &ctx.accounts.state.to_account_info(),
                &[&[b"STATE", &[ctx.accounts.state.bump]]],
                ctx.accounts.escrow.amount,
            )?;
        }

        // Perform callback into the clients program
        let user_callback = &ctx.accounts.request.callback;
        let mut is_success = false;

        if user_callback.program_id == Pubkey::default() {
            msg!("The user's callback is undefined, skipping callback")
        } else {
            let mut callback_account_metas: Vec<anchor_lang::prelude::AccountMeta> =
                Vec::with_capacity(user_callback.accounts.len() + 1);
            let mut callback_account_infos: Vec<AccountInfo> =
                Vec::with_capacity(user_callback.accounts.len() + 1);

            let remaining_accounts: HashMap<Pubkey, AccountInfo<'info>> = ctx
                .remaining_accounts
                .iter()
                .map(|a| (a.key(), a.clone()))
                .collect();

            for account in user_callback.accounts[..user_callback.accounts.len()].iter() {
                if account.pubkey == ctx.accounts.payer.key() && account.is_writable {
                    // TODO: handle this better
                    continue;
                }

                if account.pubkey == ctx.accounts.enclave_signer.key() {
                    // TODO: handle this better
                    continue;
                }

                if account.pubkey == ctx.accounts.request.key() {
                    callback_account_metas.push(account.into());
                    callback_account_infos.push(ctx.accounts.request.to_account_info());
                    continue;
                }

                if account.pubkey == ctx.accounts.state.key() {
                    if !account.is_signer {
                        return Err(error!(RandomnessError::InvalidCallback));
                    }

                    callback_account_metas.push(account.into());
                    callback_account_infos.push(ctx.accounts.state.to_account_info());
                    continue;
                }

                match remaining_accounts.get(&account.pubkey) {
                    None => {
                        msg!(
                            "Failed to find account in remaining_accounts {}",
                            account.pubkey
                        );
                        return Err(error!(RandomnessError::MissingCallbackAccount));
                    }
                    Some(account_info) => {
                        callback_account_metas.push(account.into());
                        callback_account_infos.push(account_info.clone());
                    }
                }
            }

            callback_account_infos.push(ctx.accounts.callback_pid.clone());

            // drop the HashMap
            drop(remaining_accounts);

            let callback_data = [
                user_callback.ix_data[..user_callback.ix_data.len()].to_vec(),
                (randomness.len() as u32).to_le_bytes().to_vec(),
                randomness.to_vec(),
            ]
            .concat();

            let callback_ix = Instruction {
                program_id: user_callback.program_id,
                data: callback_data,
                accounts: callback_account_metas,
            };

            msg!(">>> Invoking user callback <<<");

            // TODO: Why is this panicking and not catching the error?
            match invoke_signed(
                &callback_ix,
                &callback_account_infos,
                &[&[b"STATE", &[ctx.accounts.state.bump]]],
            ) {
                Err(e) => {
                    msg!("Error invoking user callback: {:?}", e);
                }
                Ok(_) => {
                    msg!("Successfully invoked user callback");
                    is_success = true;
                }
            }
        }

        // TODO: we should only close here if the callback was executed successfully

        // Try to close the token account
        ctx.accounts.escrow.reload()?;

        anchor_spl::token::close_account(CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            CloseAccount {
                account: ctx.accounts.escrow.to_account_info(),
                destination: ctx.accounts.user.to_account_info(),
                authority: ctx.accounts.state.to_account_info(),
            },
            &[&[b"STATE", &[ctx.accounts.state.bump]]],
        ))?;

        emit!(SimpleRandomnessV1SettledEvent {
            callback_pid: ctx.accounts.callback_pid.key(),
            request: ctx.accounts.request.key(),
            user: ctx.accounts.user.key(),
            is_success,
            randomness: randomness.to_vec(),
        });

        Ok(())
    }
}
