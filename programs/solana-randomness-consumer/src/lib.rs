// use solana_randomness_service::macros::*;
use solana_randomness_service::{
    program::SolanaRandomnessService, request_randomness, RandomnessRequest,
};
use switchboard_solana::prelude::*;
use switchboard_solana::utils::get_ixn_discriminator;

declare_id!("F2kR2Es3YgFkX1rReUDneVxv1bp2UZJWrpEGBXvdfKyg");

#[program]
pub mod solana_randomness_consumer {
    use super::*;

    pub fn request_randomness(ctx: Context<RequestRandomness>) -> anchor_lang::prelude::Result<()> {
        msg!("Requesting randomness...");

        RequestRandomness::request_randomness(
            &ctx,
            8,
            solana_randomness_service::types::Callback {
                program_id: ID,
                accounts: vec![solana_randomness_service::types::AccountMetaBorsh {
                    pubkey: ctx.accounts.randomness_request.key(),
                    is_signer: false,
                    is_writable: false,
                }],
                ix_data: get_ixn_discriminator("consume_randomness").to_vec(),
            },
        )?;

        // Here we can emit some event to index our requests

        Ok(())
    }

    pub fn consume_randomness(
        _ctx: Context<ConsumeRandomness>,
        result: Vec<u8>,
    ) -> anchor_lang::prelude::Result<()> {
        msg!("Randomness received: {:?}", result);
        Ok(())
    }
}

// The request_randomness macro breaks IDL generation. So we'll manually implement.
// #[request_randomness]
#[derive(Accounts)]
pub struct RequestRandomness<'info> {
    /// The Solana Randomness Service program.
    pub randomness_service: Program<'info, SolanaRandomnessService>,

    /// The account that will be created on-chain to hold the randomness request.
    /// Used by the off-chain oracle to pickup the request and fulfill it.
    /// CHECK: todo
    #[account(
        mut,
        signer,
        owner = system_program.key(),
        constraint = randomness_request.data_len() == 0 && randomness_request.lamports() == 0,
    )]
    pub randomness_request: AccountInfo<'info>,

    /// The TokenAccount that will store the funds for the randomness request.
    /// CHECK: todo
    #[account(
        mut,
        owner = system_program.key(),
        constraint = randomness_escrow.data_len() == 0 && randomness_escrow.lamports() == 0,
    )]
    pub randomness_escrow: AccountInfo<'info>,

    /// The randomness service's state account. Responsible for storing the
    /// reward escrow and the cost per random byte.
    #[account(
        seeds = [b"STATE"],
        bump = randomness_state.bump,
        seeds::program = randomness_service.key(),
    )]
    pub randomness_state: Box<Account<'info, solana_randomness_service::State>>,

    /// The token mint to use for paying for randomness requests.
    #[account(address = NativeMint::ID)]
    pub randomness_mint: Account<'info, Mint>,

    /// The account that will pay for the randomness request.
    #[account(mut)]
    pub payer: Signer<'info>,

    /// The Solana System program. Used to allocate space on-chain for the randomness_request account.
    pub system_program: Program<'info, System>,

    /// The Solana Token program. Used to transfer funds to the randomness escrow.
    pub token_program: Program<'info, Token>,

    /// The Solana Associated Token program. Used to create the TokenAccount for the randomness escrow.
    pub associated_token_program: Program<'info, AssociatedToken>,
}

impl<'info> RequestRandomness<'info> {
    /// Requests randomness from the Solana Randomness Service.
    ///
    /// This method is programatically added to the struct decorated with `#[request_randomness]` and
    /// encapsulates the logic required to invoke the Cross Program Invokation (CPI) to the Switchboard Randomness Service.
    ///
    /// # Arguments
    /// * `ctx` - A reference to the context holding the account information.
    /// * `num_bytes` - The number of bytes of randomness to request.
    /// * `callback` - A callback function to handle the randomness once it's available.
    ///
    /// # Returns
    /// This method returns a `ProgramResult`. On success, it indicates that the randomness request
    /// has been successfully initiated. Any errors during the process are also encapsulated within the `ProgramResult`.
    ///
    /// # Example
    /// ```
    /// let result = MyStruct::request_randomness(&context, 32, callback_function);
    ///
    /// match result {
    ///     Ok(_) => println!("Randomness requested successfully"),
    ///     Err(e) => println!("Error invoking randomness request: {:?}", e),
    /// }
    /// ```
    pub fn request_randomness(
        ctx: &Context<RequestRandomness<'info>>,
        num_bytes: u32,
        callback: solana_randomness_service::types::Callback,
    ) -> ProgramResult {
        // Call the randomness service and request a new value
        solana_randomness_service::cpi::request(
            CpiContext::new(
                ctx.accounts.randomness_service.to_account_info(),
                solana_randomness_service::cpi::accounts::Request {
                    request: ctx.accounts.randomness_request.to_account_info(),
                    escrow: ctx.accounts.randomness_escrow.to_account_info(),
                    state: ctx.accounts.randomness_state.to_account_info(),
                    mint: ctx.accounts.randomness_mint.to_account_info(),
                    payer: ctx.accounts.payer.to_account_info(),
                    system_program: ctx.accounts.system_program.to_account_info(),
                    token_program: ctx.accounts.token_program.to_account_info(),
                    associated_token_program: ctx
                        .accounts
                        .associated_token_program
                        .to_account_info(),
                },
            ),
            num_bytes,
            callback,
        )?;

        Ok(())
    }
}

#[derive(Accounts)]
pub struct ConsumeRandomness<'info> {
    pub request: AccountLoader<'info, RandomnessRequest>,
}
