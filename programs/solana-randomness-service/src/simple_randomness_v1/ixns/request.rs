use crate::*;

#[derive(Accounts)]
#[instruction(num_bytes: u8, callback: Callback)]
pub struct SimpleRandomnessV1Request<'info> {
    #[account(
        init,
        payer = payer,
        space = SimpleRandomnessV1Account::space(&callback)
    )]
    pub request: Box<Account<'info, SimpleRandomnessV1Account>>,

    #[account(
        init,
        payer = payer,
        associated_token::mint = mint,
        associated_token::authority = request,
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

impl SimpleRandomnessV1Request<'_> {
    pub fn validate(
        &self,
        ctx: &Context<Self>,
        num_bytes: u8,
        callback: &Callback,
    ) -> anchor_lang::Result<()> {
        if num_bytes == 0 || num_bytes > 32 {
            return Err(error!(RandomnessError::InvalidNumberOfBytes));
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
                continue;
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

        Ok(())
    }

    pub fn actuate(
        ctx: &mut Context<Self>,
        num_bytes: u8,
        callback: &Callback,
    ) -> anchor_lang::Result<()> {
        let request_account_info = ctx.accounts.request.to_account_info();

        // set the escrow authority to our state account
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

        ctx.accounts.request.num_bytes = num_bytes;
        ctx.accounts.request.user = ctx.accounts.payer.key();
        ctx.accounts.request.escrow = ctx.accounts.escrow.key();
        ctx.accounts.request.request_slot = Clock::get()?.slot;
        ctx.accounts.request.callback = callback.clone().into();

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

        emit!(SimpleRandomnessV1RequestedEvent {
            callback_pid: callback.program_id,
            user: ctx.accounts.payer.key(),
            request: ctx.accounts.request.key(),
            callback: callback.clone(),
            num_bytes,
        });

        Ok(())
    }
}
