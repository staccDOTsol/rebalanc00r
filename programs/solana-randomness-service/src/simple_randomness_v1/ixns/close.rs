use crate::*;

use anchor_spl::token::CloseAccount;

#[derive(Accounts)]
pub struct SimpleRandomnessV1Close<'info> {
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

impl<'info> SimpleRandomnessV1Close<'info> {
    pub fn validate(&self, _ctx: &Ctx<Self>) -> anchor_lang::Result<()> {
        Ok(())
    }

    pub fn actuate(ctx: &mut Ctx<'_, 'info, Self>) -> anchor_lang::Result<()> {
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
