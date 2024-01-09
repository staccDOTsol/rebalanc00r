pub use switchboard_solana::prelude::*;

pub use anchor_spl::token::{spl_token::instruction::AuthorityType, SetAuthority};
pub use std::collections::HashMap;
pub use switchboard_solana::prelude::NativeMint;

pub mod macros;
pub use macros::*;

// Only need macros for CPI interfaces
// #[cfg(target_os = "solana")]
// #[cfg_attr(doc_cfg, doc(cfg(target_os = "solana")))]

// SDK is intended for off-chain use functionality like client fetching
#[cfg(not(target_os = "solana"))]
#[cfg_attr(doc_cfg, doc(cfg(not(target_os = "solana"))))]
mod sdk;
#[cfg(not(target_os = "solana"))]
#[cfg_attr(doc_cfg, doc(cfg(not(target_os = "solana"))))]
pub use sdk::*;

pub mod types;
pub use types::*;

mod errors;
pub use errors::*;

pub mod state;
pub use state::*;

pub mod utils;
pub use utils::*;

declare_id!("RANDa4nas8AqeYP7LXGu6VSSZqqHYQv5vW6P82peWsP");

pub use types::{AccountMetaBorsh, AccountMetaZC, Callback, CallbackZC};

// We will listen to this event inside of our service
#[event]
#[derive(Debug, Clone)]
pub struct RandomnessRequested {
    #[index]
    pub request: Pubkey,
    pub user: Pubkey,
    pub callback: Callback,
    pub num_bytes: u32,
}

#[event]
#[derive(Debug, Clone)]
pub struct RandomnessFulfilled {
    #[index]
    pub request: Pubkey,
    pub user: Pubkey,
    pub is_success: bool,
    pub randomness: Vec<u8>,
}

#[program]
pub mod solana_randomness_service {

    use anchor_spl::token::CloseAccount;

    use super::*;

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

    pub fn set_fees(
        ctx: Context<SetFeeConfig>,
        cost_per_byte: u64,
    ) -> anchor_lang::prelude::Result<()> {
        // TODO: this may cause in-flight requests to fail

        ctx.accounts.state.cost_per_byte = cost_per_byte;
        ctx.accounts.state.last_updated = Clock::get()?.unix_timestamp;

        Ok(())
    }

    pub fn request(
        ctx: Context<Request>,
        num_bytes: u32,
        callback: Callback,
    ) -> anchor_lang::prelude::Result<()> {
        // TODO: Add the ability to specify the priority fees on the response txn. Need to inspect txns to verify priority fees were attached.

        if num_bytes == 0 {
            return Err(error!(RandomnessError::MissingNumBytes));
        }
        if num_bytes > 32 {
            return Err(error!(RandomnessError::OverflowNumBytes));
        }

        // TODO: Inspect the callback and ensure no other signers will be required
        // for account in callback.accounts.iter() {}

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
        let request = &mut ctx.accounts.request.load_init()?;

        request.user = ctx.accounts.payer.key();
        request.request_slot = Clock::get()?.slot;
        request.num_bytes = num_bytes;
        request.callback = callback.clone().into();

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

        emit!(RandomnessRequested {
            request: ctx.accounts.request.key(),
            user: ctx.accounts.payer.key(),
            callback,
            num_bytes,
        });

        Ok(())
    }

    pub fn settle<'a, 'b, 'c, 'info>(
        ctx: Context<'a, 'b, 'c, 'info, Settle<'info>>,
        result: Vec<u8>,
    ) -> anchor_lang::prelude::Result<()> {
        let request = ctx.accounts.request.load()?;

        // Need to make sure the payer is not included in the callback as a writeable account. Otherwise, the payer could be drained of funds.
        for account in request.callback.accounts[..request.callback.accounts_len as usize].iter() {
            if account.pubkey == ctx.accounts.payer.key() && account.is_writable == 1 {
                // TODO: We should still transfer funds and close the request without invoking the callback. Wasting our time.
                return Err(error!(RandomnessError::InvalidCallback));
            }
        }

        // Transfer reward (all funds) to the program_state
        let cost = ctx.accounts.state.request_cost(request.num_bytes);
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
        let user_callback = &request.callback;
        let mut is_success = false;

        if user_callback.program_id == Pubkey::default() {
            msg!("The user's callback is undefined, skipping callback")
        } else {
            let mut callback_account_metas: Vec<anchor_lang::prelude::AccountMeta> =
                Vec::with_capacity(user_callback.accounts_len as usize + 1);
            let mut callback_account_infos: Vec<AccountInfo> =
                Vec::with_capacity(user_callback.accounts_len as usize + 1);

            {
                let remaining_accounts: HashMap<Pubkey, AccountInfo<'info>> = ctx
                    .remaining_accounts
                    .iter()
                    .map(|a| (a.key(), a.clone()))
                    .collect();

                for account in user_callback.accounts[..user_callback.accounts_len as usize].iter()
                {
                    if account.pubkey == ctx.accounts.payer.key() && account.is_writable == 1 {
                        // TODO: handle this better
                        continue;
                    }

                    if account.pubkey == ctx.accounts.request.key() {
                        callback_account_metas.push(account.into());
                        callback_account_infos.push(ctx.accounts.request.to_account_info());
                        continue;
                    }

                    if account.pubkey == ctx.accounts.state.key() {
                        callback_account_metas.push(account.into());
                        callback_account_infos.push(ctx.accounts.state.to_account_info());
                        continue;
                    }

                    let account_info = remaining_accounts.get(&account.pubkey).unwrap();

                    callback_account_metas.push(account.into());
                    callback_account_infos.push(account_info.clone());
                }

                callback_account_infos.push(ctx.accounts.callback_pid.clone());

                // drop the HashMap
            }

            let callback_data = [
                user_callback.ix_data[..user_callback.ix_data_len as usize].to_vec(),
                (result.len() as u32).to_le_bytes().to_vec(),
                result.to_vec(),
            ]
            .concat();
            msg!(
                "callback_data ({}): {:?}",
                callback_data.len(),
                callback_data
            );

            let callback_ix = Instruction {
                program_id: user_callback.program_id,
                data: callback_data,
                accounts: callback_account_metas,
            };

            match invoke(&callback_ix, &callback_account_infos) {
                Err(e) => {
                    msg!("Error invoking user callback: {:?}", e);
                }
                Ok(_) => {
                    msg!("Successfully invoked user callback");
                    is_success = true;
                }
            };
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

        emit!(RandomnessFulfilled {
            request: ctx.accounts.request.key(),
            user: ctx.accounts.user.key(),
            is_success,
            randomness: result.clone(),
        });

        Ok(())
    }
}

/////////////////////////////////////////////////////////////
/// Initialize
/////////////////////////////////////////////////////////////

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(
        init,
        payer = payer,
        space = State::size(),
        seeds = [b"STATE"],
        bump,
    )]
    pub state: Box<Account<'info, State>>,

    pub switchboard_service: Box<Account<'info, FunctionServiceAccountData>>,

    #[account(
        init,
        payer = payer,
        associated_token::mint = mint,
        associated_token::authority = state
    )]
    pub wallet: Box<Account<'info, TokenAccount>>,

    #[account(address = NativeMint::ID)]
    pub mint: Account<'info, Mint>,

    #[account(mut)]
    pub payer: Signer<'info>,

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

/////////////////////////////////////////////////////////////
/// Request
/////////////////////////////////////////////////////////////

// #[event_cpi]
#[derive(Accounts)]
pub struct Request<'info> {
    #[account(
        init,
        payer = payer,
        space = RandomnessRequest::size()
    )]
    pub request: AccountLoader<'info, RandomnessRequest>,

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
// #[event_cpi]
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
    pub request: AccountLoader<'info, RandomnessRequest>,

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
    pub callback_pid: AccountInfo<'info>,
}
