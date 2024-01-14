use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Fields, FieldsNamed, ItemStruct, Lifetime, Meta};

/**
The `request_rebalancing` procedural macro is designed to add the required fields used to request rebalancing from the
Switchboard Rebalancing Service. This macro is intended to decorate the Anchor accounts context that is also decorated
with `#[derive(Accounts)]`.

The macro will add the following fields to the struct if they are not found:
- `rebalancing_request`: The account that will be created on-chain to hold the rebalancing request.
    Used by the off-chain oracle to pickup the request and fulfill it.
- `rebalancing_escrow`: The TokenAccount that will store the funds for the rebalancing request.
    Used by the off-chain oracle to pickup the request and fulfill it.
- `rebalancing_state`: The rebalancing service's state account. Responsible for storing the reward
    escrow and the cost per random byte.
- `rebalancing_mint`: The token mint to use for paying for rebalancing requests.
- `payer`: The account that will pay for the rebalancing request.
- `system_program`: The Solana System program. Used to allocate space on-chain for the rebalancing_request account.
- `token_program`: The Solana Token program. Used to transfer funds to the rebalancing escrow.
- `associated_token_program`: The Solana Associated Token program. Used to create the TokenAccount for the rebalancing escrow.
- `rebalancing_service`: The Solana Rebalancing Service program.

## Example of Expanded Code:
Given a struct `MyStruct`, the expanded code would look something like this:

```rust
#[request_rebalancing]
#[derive(Accounts)]
pub struct MyStruct<'info> {
    // Existing fields...
    pub some_account: AccountLoader<'info, SomeAccount>,

    // Added fields:
    #[account(
        mut,
        signer,
        owner = system_program.key(),
        constraint = rebalancing_request.data_len() == 0 && rebalancing_request.lamports() == 0,
    )]
    pub rebalancing_request: AccountInfo<'info>,
    #[account(
        mut,
        owner = system_program.key(),
        constraint = rebalancing_escrow.data_len() == 0 && rebalancing_escrow.lamports() == 0,
    )]
    pub rebalancing_escrow: AccountInfo<'info>,
    // ... other added fields ...
}

impl<'info> MyStruct<'info> {
    pub fn request_rebalancing(
        ctx: &Context<MyStruct<'info>>,
        num_bytes: u32,
        callback: raydium_rebalanc00r_service::types::Callback,
    ) -> ProgramResult {
        // Function implementation...
    }
}
*/
#[proc_macro_attribute]
pub fn request_rebalancing(_attr: TokenStream, item: TokenStream) -> TokenStream {
    // Parse the struct
    let mut input = parse_macro_input!(item as ItemStruct);

    // Check if `#[derive(Accounts)]` is present
    if !input.attrs.iter().any(|attr| {
        if let Meta::List(meta) = &attr.meta {
            if meta.path.is_ident("derive") {
                return meta.tokens.to_string().ends_with("Accounts");
            }
        }
        false
    }) {
        return syn::Error::new_spanned(
            &input,
            "This struct must be marked with #[derive(Accounts)]",
        )
        .to_compile_error()
        .into();
    }

    // Extract name of lifetime
    let lifetime: Lifetime = if let Some(lifetime) = input.generics.lifetimes().next() {
        lifetime.lifetime.clone()
    } else {
        return syn::Error::new_spanned(
            &input,
            "This struct must have exactly one lifetime parameter",
        )
        .to_compile_error()
        .into();
    };

    // Define the additional fields
    let additional_fields = quote! {
        {
            /// The account that will be created on-chain to hold the rebalancing request.
            /// Used by the off-chain oracle to pickup the request and fulfill it.
            /// CHECK: todo
            #[account(
                mut,
                signer,
                owner = system_program.key(),
                constraint = rebalancing_request.data_len() == 0 && rebalancing_request.lamports() == 0,
            )]
            pub rebalancing_request: AccountInfo<#lifetime>,

            /// The TokenAccount that will store the funds for the rebalancing request.
            /// CHECK: todo
            #[account(
                mut,
                owner = system_program.key(),
                constraint = rebalancing_escrow.data_len() == 0 && rebalancing_escrow.lamports() == 0,
            )]
            pub rebalancing_escrow: AccountInfo<#lifetime>,

            /// The rebalancing service's state account. Responsible for storing the
            /// reward escrow and the cost per random byte.
            #[account(
                seeds = [b"STATE"],
                bump = rebalancing_state.bump,
                seeds::program = rebalancing_service.key(),
            )]
            pub rebalancing_state: Box<Account<#lifetime, raydium_rebalanc00r_service::State>>,

            /// The token mint to use for paying for rebalancing requests.
            #[account(address = NativeMint::ID)]
            pub rebalancing_mint: Account<#lifetime, Mint>,

            /// The account that will pay for the rebalancing request.
            #[account(mut)]
            pub payer: Signer<'info>,

            /// The Solana System program. Used to allocate space on-chain for the rebalancing_request account.
            pub system_program: Program<#lifetime, System>,

            /// The Solana Token program. Used to transfer funds to the rebalancing escrow.
            pub token_program: Program<#lifetime, Token>,

            /// The Solana Associated Token program. Used to create the TokenAccount for the rebalancing escrow.
            pub associated_token_program: Program<#lifetime, AssociatedToken>,

            /// The Solana Rebalancing Service program.
            pub rebalancing_service: Program<#lifetime, RaydiumRebalanc00rService>,
        }
    };

    let new_fields = match syn::parse2::<syn::FieldsNamed>(additional_fields) {
        Ok(parsed_fields) => parsed_fields,
        Err(err) => {
            return syn::Error::new_spanned(
                err.to_compile_error(),
                format!("Failed to parse additional fields: {:?}", err),
            )
            .to_compile_error()
            .into();
        }
    };

    // Check if the struct has named fields and add a new field
    if let Fields::Named(FieldsNamed { ref mut named, .. }) = input.fields {
        for new_field in new_fields.named {
            if let Some(ident) = &new_field.ident {
                // Now check whether our struct already has this field
                match named.iter().find(|f| {
                    if let Some(existing_ident) = &f.ident {
                        existing_ident == ident
                    } else {
                        false
                    }
                }) {
                    // If the field already exists, we can perform some light analysis to make sure it conforms correctly
                    Some(_existing_field) => {
                        // TODO: here we can do some comparison between new_field and existing_field
                        // if ident == "rebalancing_request" {}
                    }
                    // If the field does NOT exist, add it to the struct
                    None => {
                        named.push(new_field);
                    }
                }
            }
        }
    }

    let name = input.ident.clone();

    // Recreate the struct including the additional fields
    let output = quote! {
        #input

        impl<#lifetime> #name<#lifetime> {
            /// Requests rebalancing from the Solana Rebalancing Service.
            ///
            /// This method is programatically added to the struct decorated with `#[request_rebalancing]` and
            /// encapsulates the logic required to invoke the Cross Program Invokation (CPI) to the Switchboard Rebalancing Service.
            ///
            /// # Arguments
            /// * `ctx` - A reference to the context holding the account information.
            /// * `num_bytes` - The number of bytes of rebalancing to request.
            /// * `callback` - A callback function to handle the rebalancing once it's available.
            ///
            /// # Returns
            /// This method returns a `ProgramResult`. On success, it indicates that the rebalancing request
            /// has been successfully initiated. Any errors during the process are also encapsulated within the `ProgramResult`.
            ///
            /// # Example
            /// ```
            /// let result = MyStruct::request_rebalancing(&context, 32, callback_function);
            ///
            /// match result {
            ///     Ok(_) => println!("Rebalancing requested successfully"),
            ///     Err(e) => println!("Error invoking rebalancing request: {:?}", e),
            /// }
            /// ```
            pub fn request_rebalancing(
                ctx: &Context<#name<#lifetime>>,
                num_bytes: u32,
                callback: raydium_rebalanc00r_service::types::Callback,
            ) -> ProgramResult {
                // Call the rebalancing service and request a new value
                raydium_rebalanc00r_service::cpi::request(
                    CpiContext::new(
                        ctx.accounts.rebalancing_service.to_account_info(),
                        raydium_rebalanc00r_service::cpi::accounts::Request {
                            request: ctx.accounts.rebalancing_request.to_account_info(),
                            escrow: ctx.accounts.rebalancing_escrow.to_account_info(),
                            state: ctx.accounts.rebalancing_state.to_account_info(),
                            mint: ctx.accounts.rebalancing_mint.to_account_info(),
                            payer: ctx.accounts.payer.to_account_info(),
                            system_program: ctx.accounts.system_program.to_account_info(),
                            token_program: ctx.accounts.token_program.to_account_info(),
                            associated_token_program: ctx.accounts.associated_token_program.to_account_info(),
                        },
                    ),
                    num_bytes,
                    callback,
                )?;

                Ok(())
            }
        }
    };

    // Return the new TokenStream
    output.into()
}

// use syn::parse::{Parse, ParseStream};

// #[derive(Clone, Default)]
// struct RebalancingAccounts {}

// impl Parse for RebalancingAccounts {
//     fn parse(input: ParseStream) -> syn::Result<Self> {
//         if input.is_empty() {
//             return Ok(Default::default());
//         }

//         Ok(Self {})
//     }
// }

// Parse the macro parameters
// let args = match syn::parse::<RebalancingAccounts>(attr) {
//     Ok(args) => args,
//     Err(err) => {
//         return syn::Error::new_spanned(
//             err.to_compile_error(),
//             format!("Failed to parse macro parameters: {:?}", err),
//         )
//         .to_compile_error()
//         .into();
//     }
// };
