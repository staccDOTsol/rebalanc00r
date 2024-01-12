pub use switchboard_solana::prelude::*;

pub use anchor_spl::token::{spl_token::instruction::AuthorityType, SetAuthority};
pub use solana_program::sysvar::instructions::ID as SYSVAR_INSTRUCTIONS_ID;
pub use std::collections::HashMap;
pub use switchboard_solana::prelude::NativeMint;

pub mod types;
pub use types::*;

mod errors;
pub use errors::*;

pub mod state;
pub use state::*;

pub mod utils;
pub use utils::*;

declare_id!("RANDMa8hJmXEnKyQtbgrWsg4AgomUG1PFDr1yPP1hFA");

pub use types::{AccountMetaBorsh, Callback};

// We will listen to this event inside of our service
#[event]
#[derive(Debug, Clone)]
pub struct RandomnessRequestedEvent {
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
pub struct RandomnessFulfilledEvent {
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
pub struct UserCallbackFailedEvent {
    #[index]
    pub callback_pid: Pubkey,
    #[index]
    pub user: Pubkey,
    pub request: Pubkey,
    pub error_message: String,
}

#[program]
pub mod solana_randomness_service {

    use anchor_spl::token::CloseAccount;

    use super::*;

    /// Initializes the program state and sets the:
    /// - program authority
    /// - Switchboard service that is used to fulfill requests
    /// - The cost per randomness byte, in addition to 5000 lamports for the txn fee
    /// - The wallet that will accrue rewards for fulfilling randomness
    pub fn initialize(
        ctx: Context<Initialize>,
        cost_per_byte: u64,
    ) -> anchor_lang::prelude::Result<()> {
        ctx.accounts.state.bump = ctx.bumps.state;
        ctx.accounts.state.authority = ctx.accounts.payer.key();
        ctx.accounts.state.mint = ctx.accounts.mint.key();
        ctx.accounts.state.switchboard_service = ctx.accounts.switchboard_service.key();
        ctx.accounts.state.wallet = ctx.accounts.wallet.key();
        ctx.accounts.state.cost_per_byte = cost_per_byte;

        Ok(())
    }

    /// Sets the fees for requesting randomness
    pub fn set_fees(
        ctx: Context<SetFeeConfig>,
        cost_per_byte: u64,
    ) -> anchor_lang::prelude::Result<()> {
        // TODO: this may cause in-flight requests to fail

        ctx.accounts.state.cost_per_byte = cost_per_byte;
        ctx.accounts.state.last_updated = Clock::get()?.unix_timestamp;

        Ok(())
    }

    /// Sets the Switchboard service that will be used to fulfill requests.
    pub fn set_switchboard_service(
        ctx: Context<SetSwitchboardService>,
    ) -> anchor_lang::prelude::Result<()> {
        ctx.accounts.state.switchboard_service = ctx.accounts.switchboard_service.key();

        Ok(())
    }

    /// Request randomness from the Switchboard service.
    pub fn request(
        ctx: Context<Request>,
        num_bytes: u8,
        callback: Callback,
    ) -> anchor_lang::prelude::Result<()> {
        // TODO: Add the ability to specify the priority fees on the response txn. Need to inspect txns to verify priority fees were attached.

        if num_bytes == 0 {
            return Err(error!(RandomnessError::MissingNumBytes));
        }
        if num_bytes > 32 {
            return Err(error!(RandomnessError::OverflowNumBytes));
        }

        // Inspect the callback and ensure:
        // - State PDA is required as a signer
        // - no other signers will be required
        let mut num_signers = 0;
        let mut has_randomness_state_signer = false;
        for account in callback.accounts.iter() {
            if account.pubkey == ctx.accounts.state.key() {
                if !account.is_signer {
                    msg!("Randomness state must be a signer to validate the request");
                    return Err(error!(RandomnessError::InvalidCallback));
                }

                has_randomness_state_signer = true;
            }

            if account.is_signer {
                num_signers += 1;
            }
        }

        if !has_randomness_state_signer {
            msg!("Randomness state must be provided as a signer to validate the request");
            return Err(error!(RandomnessError::InvalidCallback));
        }

        if num_signers > 0 {
            msg!("Randomness callback must not require any signers other than the state account");
            return Err(error!(RandomnessError::InvalidCallback));
        }

        let request_account_info = ctx.accounts.request.to_account_info();

        // set the escrow authority to our state account
        msg!("setting token authority");
        anchor_spl::token::set_authority(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info().clone(),
                SetAuthority {
                    account_or_mint: ctx.accounts.escrow.to_account_info().clone(),
                    current_authority: request_account_info.clone(),
                },
            ),
            AuthorityType::AccountOwner,
            Some(ctx.accounts.state.key()),
        )?;

        msg!("initializing the request");

        ctx.accounts.request.user = ctx.accounts.payer.key();
        ctx.accounts.request.request_slot = Clock::get()?.slot;
        ctx.accounts.request.num_bytes = num_bytes;
        ctx.accounts.request.callback = callback.clone();

        // wrap funds from the payer to the escrow account to reward Switchboard service for fuliflling our request
        let cost = ctx.accounts.state.request_cost(num_bytes);
        if cost > 0 {
            msg!("cost: {:?}", cost);
            wrap_native(
                &ctx.accounts.system_program.to_account_info(),
                &ctx.accounts.token_program.to_account_info(),
                &ctx.accounts.escrow,
                &ctx.accounts.payer,
                &[&[b"STATE", &[ctx.accounts.state.bump]]],
                cost,
            )?;
        }

        emit!(RandomnessRequestedEvent {
            callback_pid: callback.program_id,
            user: ctx.accounts.payer.key(),
            request: ctx.accounts.request.key(),
            callback,
            num_bytes,
        });

        Ok(())
    }

    /// Settles a randomness request and invokes the user's callback.
    pub fn settle<'a, 'b, 'c, 'info>(
        ctx: Context<'a, 'b, 'c, 'info, Settle<'info>>,
        result: Vec<u8>,
    ) -> anchor_lang::prelude::Result<()> {
        // Verify this method was not called from a CPI
        assert_not_cpi_call(&ctx.accounts.instructions_sysvar)?;

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
        msg!("cost: {:?}", cost);

        // verify the escrow has enough funds
        if cost > ctx.accounts.escrow.amount {
            return Err(error!(RandomnessError::InsufficientFunds));
        }

        if ctx.accounts.escrow.amount > 0 {
            msg!("transferrring {:?} to wallet", ctx.accounts.escrow.amount);
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
                (result.len() as u32).to_le_bytes().to_vec(),
                result.to_vec(),
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

        emit!(RandomnessFulfilledEvent {
            callback_pid: ctx.accounts.callback_pid.key(),
            request: ctx.accounts.request.key(),
            user: ctx.accounts.user.key(),
            is_success,
            randomness: result.clone(),
        });

        Ok(())
    }

    pub fn user_callback_failed<'a, 'b, 'c, 'info>(
        ctx: Context<'a, 'b, 'c, 'info, UserCallbackFailed<'info>>,
        error_message: String,
    ) -> anchor_lang::prelude::Result<()> {
        // Verify this method was not called from a CPI
        assert_not_cpi_call(&ctx.accounts.instructions_sysvar)?;

        if error_message.len() > 512 {
            return Err(error!(RandomnessError::MissingNumBytes));
        }

        // Transfer reward (all funds) to the program_state
        let cost = ctx
            .accounts
            .state
            .request_cost(ctx.accounts.request.num_bytes);
        msg!("cost: {:?}", cost);

        // verify the escrow has enough funds
        if cost > ctx.accounts.escrow.amount {
            return Err(error!(RandomnessError::InsufficientFunds));
        }

        if ctx.accounts.escrow.amount > 0 {
            msg!("transferrring {:?} to wallet", ctx.accounts.escrow.amount);
            transfer(
                &ctx.accounts.token_program.to_account_info(),
                &ctx.accounts.escrow,
                &ctx.accounts.wallet,
                &ctx.accounts.state.to_account_info(),
                &[&[b"STATE", &[ctx.accounts.state.bump]]],
                ctx.accounts.escrow.amount,
            )?;
        }

        ctx.accounts.request.error_message = error_message.clone();

        Ok(())
    }

    /// Allows the user to close the request after acknowledging the error message.
    pub fn close_request<'a, 'b, 'c, 'info>(
        ctx: Context<'a, 'b, 'c, 'info, CloseRequest<'info>>,
    ) -> anchor_lang::prelude::Result<()> {
        // Close the token account
        anchor_spl::token::close_account(CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            CloseAccount {
                account: ctx.accounts.escrow.to_account_info(),
                destination: ctx.accounts.user.to_account_info(),
                authority: ctx.accounts.state.to_account_info(),
            },
            &[&[b"STATE", &[ctx.accounts.state.bump]]],
        ))?;

        Ok(())
    }
}

/////////////////////////////////////////////////////////////
/// Initialize
/////////////////////////////////////////////////////////////

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(
        init,
        payer = payer,
        space = State::size(),
        seeds = [b"STATE"],
        bump,
    )]
    pub state: Box<Account<'info, State>>,

    #[account(
        init,
        payer = payer,
        associated_token::mint = mint,
        associated_token::authority = state
    )]
    pub wallet: Box<Account<'info, TokenAccount>>,

    #[account(address = NativeMint::ID)]
    pub mint: Account<'info, Mint>,

    #[account(
        // constraint = switchboard_function.load()?.services_disabled.is_disabled() &&
        constraint = switchboard_service.function == switchboard_function.key()
    )]
    pub switchboard_function: AccountLoader<'info, FunctionAccountData>,
    pub switchboard_service: Box<Account<'info, FunctionServiceAccountData>>,

    pub system_program: Program<'info, System>,

    pub token_program: Program<'info, Token>,

    pub associated_token_program: Program<'info, AssociatedToken>,
}

#[derive(Accounts)]
pub struct SetFeeConfig<'info> {
    #[account(
        mut,
        seeds = [b"STATE"],
        bump = state.bump,
        has_one = authority,
    )]
    pub state: Box<Account<'info, State>>,

    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct SetSwitchboardService<'info> {
    #[account(
        mut,
        seeds = [b"STATE"],
        bump = state.bump,
        has_one = authority,
    )]
    pub state: Box<Account<'info, State>>,

    pub authority: Signer<'info>,

    #[account(
        // constraint = switchboard_function.load()?.services_disabled.is_disabled() &&
        constraint = switchboard_service.function == switchboard_function.key()
    )]
    pub switchboard_function: AccountLoader<'info, FunctionAccountData>,
    pub switchboard_service: Box<Account<'info, FunctionServiceAccountData>>,
}

/////////////////////////////////////////////////////////////
/// Request
/////////////////////////////////////////////////////////////

#[derive(Accounts)]
#[instruction(num_bytes: u32, callback: Callback)]
pub struct Request<'info> {
    #[account(
        init,
        payer = payer,
        space = RandomnessRequest::space(&callback)
    )]
    pub request: Box<Account<'info, RandomnessRequest>>,

    #[account(
        init,
        payer = payer,
        associated_token::mint = mint,
        associated_token::authority = request
    )]
    pub escrow: Box<Account<'info, TokenAccount>>,

    #[account(
        seeds = [b"STATE"],
        bump = state.bump,
        has_one = mint,
    )]
    pub state: Box<Account<'info, State>>,

    #[account(address = NativeMint::ID)]
    pub mint: Account<'info, Mint>,

    #[account(
        mut,
        owner = system_program.key(),
    )]
    pub payer: Signer<'info>,

    pub system_program: Program<'info, System>,

    pub token_program: Program<'info, Token>,

    pub associated_token_program: Program<'info, AssociatedToken>,
}

/////////////////////////////////////////////////////////////
/// Settle
/////////////////////////////////////////////////////////////
#[derive(Accounts)]
pub struct Settle<'info> {
    /// The account that pays for the randomness request
    #[account(
        mut,
        owner = system_program.key(),
    )]
    pub payer: Signer<'info>,

    #[account(
        mut,
        close = user,
        has_one = user,
    )]
    pub request: Box<Account<'info, RandomnessRequest>>,

    /// CHECK:
    #[account(
            mut, // receives SOL
            owner = system_program.key(),
    )]
    pub user: AccountInfo<'info>,

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
    // #[account(
    //     constraint = switchboard_function.load()?.validate_service(
    //         &switchboard_service,
    //         &enclave_signer.to_account_info(),
    //     )?
    // )]
    pub switchboard_function: AccountLoader<'info, FunctionAccountData>,
    #[account(
        constraint = switchboard_service.function == switchboard_function.key()
    )]
    pub switchboard_service: Box<Account<'info, FunctionServiceAccountData>>,

    pub enclave_signer: Signer<'info>,

    pub system_program: Program<'info, System>,

    pub token_program: Program<'info, Token>,

    /// CHECK: todo
    pub callback_pid: AccountInfo<'info>,

    /// CHECK: todo
    #[account(
        address = SYSVAR_INSTRUCTIONS_ID,
    )]
    pub instructions_sysvar: AccountInfo<'info>,
}

/////////////////////////////////////////////////////////////
/// Callback Failed
/////////////////////////////////////////////////////////////
#[derive(Accounts)]
pub struct UserCallbackFailed<'info> {
    #[account(
        mut,
        constraint = request.is_completed == 0 @ RandomnessError::RequestAlreadyCompleted,
        has_one = escrow,
    )]
    pub request: Box<Account<'info, RandomnessRequest>>,

    #[account(
        mut,
        // escrow::mint = mint,
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
    #[account(
        constraint = switchboard_service.function == switchboard_function.key()
    )]
    pub switchboard_service: Box<Account<'info, FunctionServiceAccountData>>,

    pub enclave_signer: Signer<'info>,

    pub system_program: Program<'info, System>,

    pub token_program: Program<'info, Token>,

    /// CHECK: todo
    #[account(
        address = SYSVAR_INSTRUCTIONS_ID,
    )]
    pub instructions_sysvar: AccountInfo<'info>,
}

/////////////////////////////////////////////////////////////
/// Close Request
/////////////////////////////////////////////////////////////
#[derive(Accounts)]
pub struct CloseRequest<'info> {
    /// CHECK: should we require them to sign or allow anyone to close?
    #[account(mut)]
    pub user: AccountInfo<'info>,

    #[account(
        mut,
        close = user,
        has_one = user,
        has_one = escrow,
        constraint = request.is_completed == 1 @ RandomnessError::RequestStillActive,
    )]
    pub request: Box<Account<'info, RandomnessRequest>>,

    #[account(
        mut,
        constraint = escrow.is_native() && escrow.owner == state.key(),
    )]
    pub escrow: Box<Account<'info, TokenAccount>>,

    #[account(
        seeds = [b"STATE"],
        bump = state.bump,
        has_one = wallet,
    )]
    pub state: Box<Account<'info, State>>,

    #[account(
        mut,
        constraint = wallet.is_native() && wallet.owner == state.key(),
    )]
    pub wallet: Box<Account<'info, TokenAccount>>,

    pub system_program: Program<'info, System>,

    pub token_program: Program<'info, Token>,
}
