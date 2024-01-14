pub mod types;
pub use types::{AccountMetaBorsh, Callback};

pub use anchor_spl::token::CloseAccount;
pub use anchor_spl::token::{spl_token::instruction::AuthorityType, SetAuthority};
pub use solana_program::sysvar::instructions::ID as SYSVAR_INSTRUCTIONS_ID;
pub use switchboard_solana::prelude::NativeMint;
pub use switchboard_solana::prelude::*;

pub mod simple_randomness_v1;
pub use simple_randomness_v1::*;

mod errors;
pub use errors::*;

pub mod state;
pub use state::*;

pub mod utils;
pub use utils::*;

declare_id!("RANDMo5gFnqnXJW5Z52KNmd24sAo95KAd5VbiCtq5Rh");

pub type Ctx<'a, 'info, T> = Context<'a, 'a, 'a, 'info, T>;

#[program]
pub mod solana_randomness_service {

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

    ////////////////////////////////////////////////////////////////////////
    /// Simple Randomness V1
    ////////////////////////////////////////////////////////////////////////

    /// Request randomness from the Switchboard service.
    #[access_control(ctx.accounts.validate(&ctx, num_bytes, &callback))]
    pub fn simple_randomness_v1<'a>(
        mut ctx: Ctx<'_, 'a, SimpleRandomnessV1Request<'a>>,
        num_bytes: u8,
        callback: Callback,
    ) -> anchor_lang::prelude::Result<()> {
        SimpleRandomnessV1Request::actuate(&mut ctx, num_bytes, &callback)
    }

    /// Settles a randomness request and invokes the user's callback.
    #[access_control(ctx.accounts.validate(&ctx,  &randomness))]
    pub fn simple_randomness_v1_settle<'a>(
        mut ctx: Ctx<'_, 'a, SimpleRandomnessV1Settle<'a>>,
        randomness: Vec<u8>,
    ) -> anchor_lang::prelude::Result<()> {
        SimpleRandomnessV1Settle::actuate(&mut ctx, &randomness)
    }

    #[access_control(ctx.accounts.validate(&ctx,  &error_message))]
    pub fn simple_randomness_v1_callback_error<'a>(
        mut ctx: Ctx<'_, 'a, SimpleRandomnessV1CallbackError<'a>>,
        error_message: String,
    ) -> anchor_lang::prelude::Result<()> {
        SimpleRandomnessV1CallbackError::actuate(&mut ctx, &error_message)
    }

    #[access_control(ctx.accounts.validate(&ctx,  ))]
    pub fn simple_randomness_v1_callback_close<'a>(
        mut ctx: Ctx<'_, 'a, SimpleRandomnessV1Close<'a>>,
    ) -> anchor_lang::prelude::Result<()> {
        SimpleRandomnessV1Close::actuate(&mut ctx)
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
