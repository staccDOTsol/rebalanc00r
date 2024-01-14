use crate::*;

#[derive(Accounts)]
pub struct SimpleRandomnessV1CallbackError<'info> {
    #[account(
        mut,
        constraint = request.is_completed == 0 @ RandomnessError::RequestAlreadyCompleted,
        has_one = escrow,
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

    /// CHECK: todo
    #[account(
        address = SYSVAR_INSTRUCTIONS_ID,
    )]
    pub instructions_sysvar: AccountInfo<'info>,
}

impl<'info> SimpleRandomnessV1CallbackError<'info> {
    pub fn validate(&self, ctx: &Ctx<Self>, error_message: &str) -> anchor_lang::Result<()> {
        msg!("Checking this ixn is not a CPI call ...");

        // Verify this method was not called from a CPI
        assert_not_cpi_call(&ctx.accounts.instructions_sysvar)?;

        if error_message.len() > 128 {
            return Err(error!(RandomnessError::ErrorMessageOverflow));
        }

        Ok(())
    }

    pub fn actuate(ctx: &mut Ctx<'_, 'info, Self>, error_message: &str) -> anchor_lang::Result<()> {
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

        ctx.accounts.request.error_message = error_message.to_string();
        ctx.accounts.request.is_completed = 1;

        emit!(SimpleRandomnessV1CallbackErrorEvent {
            callback_pid: ctx.accounts.request.callback.program_id,
            user: ctx.accounts.request.user,
            request: ctx.accounts.request.key(),
            error_message: error_message.to_string()
        });

        Ok(())
    }
}
