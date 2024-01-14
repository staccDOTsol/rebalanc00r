use crate::*;

use futures_util::stream::{FuturesOrdered, FuturesUnordered, StreamExt};
use raydium_amm_v3::states::TICK_ARRAY_SEED;
use solana_client::client_error::{ClientError, ClientErrorKind};
use solana_client::rpc_request::{RpcError, RpcResponseErrorData};
use solana_client::rpc_response::RpcSimulateTransactionResult;
use solana_program::instruction::InstructionError;
pub use solana_program::sysvar::instructions::ID as SYSVAR_INSTRUCTIONS_ID;
use solana_account_decoder::{
    parse_token::{TokenAccountType, UiAccountState},
    UiAccountData, UiAccountEncoding,
};

use solana_randomness_service::SimpleRandomnessV1Account;
use solana_sdk::{sysvar, system_program};
use solana_sdk::transaction::TransactionError;
use switchboard_solana::Secrets;
use switchboard_solana::rust_decimal::prelude::Zero;
use raydium_amm_v3::{
    libraries::{fixed_point_64, liquidity_math, tick_math},
    states::{PoolState, TickArrayBitmapExtension, TickArrayState, POOL_TICK_ARRAY_BITMAP_SEED},
};

use raydium_amm_v3::accounts as raydium_accounts;
use raydium_amm_v3::instruction as raydium_instruction;
use raydium_amm_v3::states::{
    AMM_CONFIG_SEED, OPERATION_SEED, POOL_SEED, POOL_VAULT_SEED, POSITION_SEED
};
use switchboard_solana::solana_client::rpc_request::TokenAccountsFilter;
use tokio::sync::RwLockReadGuard;
const RAYDIUM_V3_PROGRAM: &str = "devi51mZmdwUJGU9hjN27vEz64Gps7uUefqxg27EAtH";
const REQUEST_ACCOUNT_DOESNT_EXIST_ERROR_STR: &str = "Program log: AnchorError caused by account: request. Error Code: AccountOwnedByWrongProgram. Error Number: 3007. Error Message: The given account is owned by a different program than expected.\nProgram log: Left:\nProgram log: 11111111111111111111111111111111";

#[derive(Debug)]
pub struct ClientConfig {
    http_url: String,
    ws_url: String,
    payer: Arc<Keypair>,
    admin_path: String,
    raydium_v3_program: Pubkey,
    slippage: f64,
    amm_config_key: Pubkey,

    mint0: Option<Pubkey>,
    mint1: Option<Pubkey>,
    pool_id_account: Option<Pubkey>,
    tickarray_bitmap_extension: Option<Pubkey>,
    amm_config_index: u16,
    user: Pubkey
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct TokenInfo {
    key: Pubkey,
    mint: Pubkey,
    amount: u64,
    decimals: u8,
}
fn get_nft_account_and_position_by_owner(
    client: &solana_client::rpc_client::RpcClient,
    owner: &Pubkey,
    raydium_amm_v3_program: &Pubkey,
) -> (Vec<TokenInfo>, Vec<Pubkey>) {
    let all_tokens = client
        .get_token_accounts_by_owner(owner, TokenAccountsFilter::ProgramId(spl_token::id()))
        .unwrap();
    let mut nft_account = Vec::new();
    let mut user_position_account = Vec::new();
    for keyed_account in all_tokens {
        if let UiAccountData::Json(parsed_account) = keyed_account.account.data {
            if parsed_account.program == "spl-token" {
                if let Ok(TokenAccountType::Account(ui_token_account)) =
                    serde_json::from_value(parsed_account.parsed)
                {
                    let _frozen = ui_token_account.state == UiAccountState::Frozen;

                    let token = ui_token_account
                        .mint
                        .parse::<Pubkey>()
                        .unwrap_or_else(|err| panic!("Invalid mint: {}", err));
                    let token_account = keyed_account
                        .pubkey
                        .parse::<Pubkey>()
                        .unwrap_or_else(|err| panic!("Invalid token account: {}", err));
                    let token_amount = ui_token_account
                        .token_amount
                        .amount
                        .parse::<u64>()
                        .unwrap_or_else(|err| panic!("Invalid token amount: {}", err));

                    let _close_authority = ui_token_account.close_authority.map_or(*owner, |s| {
                        s.parse::<Pubkey>()
                            .unwrap_or_else(|err| panic!("Invalid close authority: {}", err))
                    });

                    if ui_token_account.token_amount.decimals == 0 && token_amount == 1 {
                        let (position_pda, _) = Pubkey::find_program_address(
                            &[
                                raydium_amm_v3::states::POSITION_SEED.as_bytes(),
                                token.to_bytes().as_ref(),
                            ],
                            &raydium_amm_v3_program,
                        );
                        nft_account.push(TokenInfo {
                            key: token_account,
                            mint: token,
                            amount: token_amount,
                            decimals: ui_token_account.token_amount.decimals,
                        });
                        user_position_account.push(position_pda);
                    }
                }
            }
        }
    }
    (nft_account, user_position_account)
}


impl RandomnessTrait for RebalancingV1CompiledTask {
    fn id(&self) -> String {
        return self.request.to_string();
    }
    fn build_ixn<'a>(
        &self,
        ctx: &ServiceContext,
        payer: Pubkey,
        switchboard_function: Pubkey,
        switchboard_service: Pubkey,
        enclave_signer: Pubkey,
        keypair: Arc<Keypair>
    ) -> Result<Instruction, SbError> {
        
        let rpc_client = ctx.rpc.clone();
        let rpc_client = Arc::new(solana_client::rpc_client::RpcClient::new_with_commitment(
            rpc_client.url().to_string(),
            CommitmentConfig {
                commitment: CommitmentLevel::Confirmed,
            },
        ));
        let user = self.user.clone();
        let request = self.request.clone();
        let request_account = rpc_client.get_account(&request);
        let request_account = match request_account {
            Ok(request_account) => {
              
                    let parsed_request = deserialize_anchor_account::<SimpleRandomnessV1Account>(
                        &request_account,
                    ).unwrap();
                    parsed_request
            },
            Err(e) => {
                return Err(SbError::CustomError {
                    message: "Failed to get request account".to_string(),
                    source: Arc::new(e),
                });
            }
        };
      

        // load position
        let (_nft_tokens, positions) = get_nft_account_and_position_by_owner(
            &rpc_client,
            &user,
            &RAYDIUM_V3_PROGRAM.parse().unwrap(),
        );
        let rsps = rpc_client.get_multiple_accounts(&positions).unwrap();
        let mut user_positions = Vec::new();
        for rsp in rsps {
            match rsp {
                None => continue,
                Some(rsp) => {
                    let position = deserialize_anchor_account::<
                        raydium_amm_v3::states::PersonalPositionState,
                    >(&rsp).unwrap();
                    user_positions.push(position);
                }
            }
        }
        let mut find_position = raydium_amm_v3::states::PersonalPositionState::default();
        let mut ixs = vec![Instruction {
            program_id: raydium_amm_v3::id(),
            accounts: vec![],
            data: vec![],
        }];
        for position in user_positions {
            let pool = rpc_client.get_account(&position.pool_id);
            let pool = match pool {
                Ok(pool) => {
                    let parsed_pool = deserialize_anchor_account::<PoolState>(&pool).unwrap();
                    parsed_pool
                },
                Err(e) => {
                    return Err(SbError::CustomError {
                        message: "Failed to get pool account".to_string(),
                        source: Arc::new(e),
                    });
                }
            };
                let (tick_lower_index, tick_upper_index) = (position.tick_lower_index, position.tick_upper_index);
                let current_tick_array = pool.tick_current;
                let tick_spacing = pool.tick_spacing;
                let (current_tick_less_spacing, current_tick_plus_spacing) =
                   (current_tick_array - tick_spacing as i32, current_tick_array + tick_spacing as i32);
                   let are_we_profitable = if current_tick_less_spacing > tick_lower_index && current_tick_plus_spacing < tick_upper_index {
                    true
                } else {
                    false
                };
                let liquidity = position.liquidity;
                let (amount_0, amount_1) = liquidity_math::get_delta_amounts_signed(
                    pool.tick_current,
                    pool.sqrt_price_x64,
                    tick_lower_index,
                    tick_upper_index,
                    liquidity as i128,
                ).unwrap();
                println!(
                    "amount_0:{}, amount_1:{}, liquidity:{}",
                    amount_0, amount_1, liquidity
                );
                // calc with slippage
                let amount_0_with_slippage =
                    amount_with_slippage(amount_0 as u64, 0.06, true);
                let amount_1_with_slippage =
                    amount_with_slippage(amount_1 as u64, 0.06, true);
                // calc with transfer_fee
                let transfer_fee = get_pool_mints_inverse_fee(
                    &rpc_client,
                    pool.token_mint_0,
                    pool.token_mint_1,
                    amount_0_with_slippage,
                    amount_1_with_slippage,
                );
                println!(
                    "transfer_fee_0:{}, transfer_fee_1:{}",
                    transfer_fee.0.transfer_fee, transfer_fee.1.transfer_fee
                );
                let amount_0_max = (amount_0_with_slippage as u64)
                    .checked_add(transfer_fee.0.transfer_fee)
                    .unwrap();
                let amount_1_max = (amount_1_with_slippage as u64)
                    .checked_add(transfer_fee.1.transfer_fee)
                    .unwrap();

                let tick_array_lower_start_index =
                    raydium_amm_v3::states::TickArrayState::get_array_start_index(
                        tick_lower_index,
                        pool.tick_spacing.into(),
                    );
                let tick_array_upper_start_index =
                    raydium_amm_v3::states::TickArrayState::get_array_start_index(
                        tick_upper_index,
                        pool.tick_spacing.into(),
                    );
                                    
                // personal position exist
                let mut remaining_accounts = Vec::new();
                let tickarray_bitmap: Pubkey = Pubkey::create_with_seed(
                    &pool.key(),
                    &POOL_TICK_ARRAY_BITMAP_SEED,
                    &raydium_amm_v3::id(),
                ).unwrap();

                remaining_accounts.push(AccountMeta::new_readonly(
                    tickarray_bitmap,
                    false,
                ));
                let pool_config: ClientConfig = ClientConfig {
                    http_url: rpc_client.url().to_string(),
                    ws_url: rpc_client.url().to_string().replace("http", "ws"),
                    payer: keypair.clone(),
                    admin_path: "/Users/chen/.config/solana/id.json".to_string(),
                    raydium_v3_program: raydium_amm_v3::id(),
                    slippage: 0.5,
                    amm_config_key: raydium_amm_v3::id(),
                    mint0: Some(pool.token_mint_0),
                    mint1: Some(pool.token_mint_1),
                    pool_id_account: Some(pool.key()),
                    tickarray_bitmap_extension: Some(tickarray_bitmap),
                    amm_config_index: 0,
                    user: user.clone(),
                };
                if are_we_profitable {

                    ixs = increase_liquidity_instr(
                        &pool_config,
                        pool_config.pool_id_account.unwrap(),
                        pool.token_vault_0,
                        pool.token_vault_1,
                        pool.token_mint_0,
                        pool.token_mint_1,
                        find_position.nft_mint,
                        spl_associated_token_account::get_associated_token_address(
                            &user,
                            &pool_config.mint0.unwrap(),
                        ),
                        spl_associated_token_account::get_associated_token_address(
                            &user,
                            &pool_config.mint1.unwrap(),
                        ),
                        remaining_accounts,
                        liquidity,
                        amount_0_max,
                        amount_1_max,
                        tick_lower_index,
                        tick_upper_index,
                        tick_array_lower_start_index,
                        tick_array_upper_start_index,
                    ).unwrap();
                }
                else {
                    // decrease liquidity to 0 
                    ixs = decrease_liquidity_instr(
                        &pool_config,
                        pool_config.pool_id_account.unwrap(),
                        pool.token_vault_0,
                        pool.token_vault_1,
                        pool.token_mint_0,
                        pool.token_mint_1,
                        find_position.nft_mint,
                        spl_associated_token_account::get_associated_token_address(
                            &user,
                            &pool_config.mint0.unwrap(),
                        ),
                        spl_associated_token_account::get_associated_token_address(
                            &user,
                            &pool_config.mint1.unwrap(),
                        ),
                        remaining_accounts.clone(),
                        liquidity,
                        amount_0_max,
                        amount_1_max,
                        tick_lower_index,
                        tick_upper_index,
                        tick_array_lower_start_index,
                        tick_array_upper_start_index,
                    ).unwrap();
                    
                    let liquidity = liquidity / 10 as u128;
                    let tick_lower_index = current_tick_less_spacing;
                    let tick_upper_index = current_tick_plus_spacing;
                    let (amount_0, amount_1) = liquidity_math::get_delta_amounts_signed(
                        pool.tick_current,
                        pool.sqrt_price_x64,
                        tick_lower_index,
                        tick_upper_index,
                        liquidity as i128,
                    ).unwrap();
                    println!(
                        "amount_0:{}, amount_1:{}, liquidity:{}",
                        amount_0, amount_1, liquidity
                    );
                    // calc with slippage
                    let amount_0_with_slippage =
                        amount_with_slippage(amount_0 as u64, 0.06, true);
                    let amount_1_with_slippage =
                        amount_with_slippage(amount_1 as u64, 0.06, true);   
                    let transfer_fee = get_pool_mints_inverse_fee(
                        &rpc_client,
                        pool.token_mint_0,
                        pool.token_mint_1,
                        amount_0_with_slippage,
                        amount_1_with_slippage,
                    );
                    println!(
                        "transfer_fee_0:{}, transfer_fee_1:{}",
                        transfer_fee.0.transfer_fee, transfer_fee.1.transfer_fee
                    );
                    let amount_0_max = (amount_0_with_slippage as u64)
                        .checked_add(transfer_fee.0.transfer_fee)
                        .unwrap();
                    let amount_1_max = (amount_1_with_slippage as u64)
                        .checked_add(transfer_fee.1.transfer_fee)
                        .unwrap();
                    ixs.append(&mut open_position_instr(
                        &pool_config,
                        pool_config.pool_id_account.unwrap(),
                        pool.token_vault_0,
                        pool.token_vault_1,
                        pool.token_mint_0,
                        pool.token_mint_1,
                        find_position.nft_mint,
                        user,
                        spl_associated_token_account::get_associated_token_address(
                            &user,
                            &pool_config.mint0.unwrap(),
                        ),
                        spl_associated_token_account::get_associated_token_address(
                            &user,
                            &pool_config.mint1.unwrap(),
                        ),
                        remaining_accounts,
                        liquidity,
                        amount_0_max,
                        amount_1_max,
                        tick_lower_index,
                        tick_upper_index,
                        tick_array_lower_start_index,
                        tick_array_upper_start_index,
                        false,
                    ).unwrap());
            }
        }
        let recent_blockhash = rpc_client.get_latest_blockhash().unwrap();
        let mut tx = Transaction::new_with_payer(&ixs, Some(&keypair.pubkey()));
        let signers = vec![keypair.as_ref()];
        
        match tx.try_sign(&signers, recent_blockhash) {
            Ok(_) => {}
            Err(e) => {
                error!("[WORKER][FAILURE] Failed to sign transaction: {:#?}", e);

                // TODO: we should ensure the users callback doesnt require any signers

                return Err(SbError::CustomError {
                    message: "Failed to sign transaction".to_string(),
                    source: Arc::new(e),
                });
            }
        }

        let signature = rpc_client
            .send_and_confirm_transaction_with_spinner_and_config(
                &tx,
                CommitmentConfig {
                    commitment: CommitmentLevel::Processed,
                },
                RpcSendTransactionConfig {
                    preflight_commitment: Some(CommitmentLevel::Processed),
                    ..Default::default()
                },
            );
        println!("signature: {:?}", signature);
        let randomness_bytes: [u8; 32] = [0; 32];
        let ixn_data_len = 8 + 4 + randomness_bytes.len();
        let mut ixn_data: &mut [u8] = &mut vec![0u8; ixn_data_len];
        ixn_data[0..8].copy_from_slice(&get_ixn_discriminator("simple_randomness_v1_settle"));
        ixn_data[8..12].copy_from_slice(&(ixn_data_len as u32).to_le_bytes());
        ixn_data[12..].copy_from_slice(&randomness_bytes);

        info!("ixn_data_len: {}", ixn_data.len());

        let mut ixn = Instruction::new_with_bytes(
            RandomnessServiceID,
            &ixn_data,
            vec![
                // User (mut)
                AccountMeta::new(self.user, false),
                // Request (mut)
                AccountMeta::new(self.request, false),
                // Request Escrow (mut)
                AccountMeta::new(
                    get_associated_token_address(&self.request, &NativeMint::ID),
                    false,
                ),
                // State
                AccountMeta::new_readonly(ctx.randomness_service_state, false),
                // State Wallet (mut)
                AccountMeta::new(ctx.randomness_service_wallet, false),
                // SWITCHBOARD
                AccountMeta::new_readonly(switchboard_function, false),
                AccountMeta::new_readonly(switchboard_service, false),
                AccountMeta::new_readonly(enclave_signer, true),
                // SystemProgram
                AccountMeta::new_readonly(SystemProgramID, false),
                // TokenProgram
                AccountMeta::new_readonly(TokenProgramID, false),
                // Payer (mut, signer)
                AccountMeta::new(payer, true),
                // Callback PID
                AccountMeta::new_readonly(self.callback.program_id, false),
                // Instructions Sysvar
                AccountMeta::new_readonly(SYSVAR_INSTRUCTIONS_ID, false),
            ],
        );

        // Next, add all of the callback accounts
        for account in self.callback.accounts.iter() {
            // Exclude the randomness_state and randomness_request accounts to reduce number of accounts
            if account.pubkey == payer {
                continue;
            }

            if account.pubkey == self.request {
                continue;
            }

            if account.pubkey == ctx.randomness_service_state {
                continue;
            }

            if account.pubkey == enclave_signer {
                continue;
            }

            if account.pubkey == Pubkey::default() {
                continue;
            }

            ixn.accounts.push(account.into());
        }

        Ok(ixn)
    }
}

#[derive(Clone)]
pub struct RebalancingV1CompiledTaskBatch {
    pub tasks: Vec<RebalancingV1CompiledTask>,
    pub enclave_signer: Arc<Keypair>,
    pub recent_blockhash: Arc<(Hash, u64)>,
}
impl RebalancingV1CompiledTaskBatch {
    // consume self
    pub async fn process(self) {
        if self.tasks.is_empty() {
            return;
        }

        let mut futures = FuturesOrdered::new();
        for task in self.tasks.iter() {
            futures.push_back(
                task.process(self.enclave_signer.clone(), self.recent_blockhash.clone()),
            );
        }

        while let Some(result) = futures.next().await {
            match result {
                Ok(signature) => {
                    info!("Signature: {:?}", result);
                }
                Err(sb_err) => {
                    let _should_retry = is_tx_error_retryable(&sb_err).await;
                    // TODO: handle this better
                }
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct RebalancingV1CompiledTask {
    pub request: Pubkey,
    pub user: Pubkey,
    pub callback: Callback,
}

pub fn open_position_instr(
    config: &ClientConfig,
    pool_account_key: Pubkey,
    token_vault_0: Pubkey,
    token_vault_1: Pubkey,
    token_mint_0: Pubkey,
    token_mint_1: Pubkey,
    nft_mint_key: Pubkey,
    nft_to_owner: Pubkey,
    user_token_account_0: Pubkey,
    user_token_account_1: Pubkey,
    remaining_accounts: Vec<AccountMeta>,
    liquidity: u128,
    amount_0_max: u64,
    amount_1_max: u64,
    tick_lower_index: i32,
    tick_upper_index: i32,
    tick_array_lower_start_index: i32,
    tick_array_upper_start_index: i32,
    with_matedata: bool,
) -> Result<Vec<Instruction>> {
    let payer = config.payer.clone();
    let url = Cluster::Custom(config.http_url.clone(), config.ws_url.clone());
    // Client.
    let client = Client::new(url, Arc::new(payer));
    let program = client.program(config.raydium_v3_program)?;
    let nft_ata_token_account =
        spl_associated_token_account::get_associated_token_address(&program.payer(), &nft_mint_key);
    let (metadata_account_key, _bump) = Pubkey::find_program_address(
        &[
            b"metadata",
            mpl_token_metadata::ID.to_bytes().as_ref(),
            nft_mint_key.to_bytes().as_ref(),
        ],
        &mpl_token_metadata::ID,
    );
    let (protocol_position_key, __bump) = Pubkey::find_program_address(
        &[
            POSITION_SEED.as_bytes(),
            pool_account_key.to_bytes().as_ref(),
            &tick_lower_index.to_be_bytes(),
            &tick_upper_index.to_be_bytes(),
        ],
        &program.id(),
    );
    let (tick_array_lower, __bump) = Pubkey::find_program_address(
        &[
            TICK_ARRAY_SEED.as_bytes(),
            pool_account_key.to_bytes().as_ref(),
            &tick_array_lower_start_index.to_be_bytes(),
        ],
        &program.id(),
    );
    let (tick_array_upper, __bump) = Pubkey::find_program_address(
        &[
            TICK_ARRAY_SEED.as_bytes(),
            pool_account_key.to_bytes().as_ref(),
            &tick_array_upper_start_index.to_be_bytes(),
        ],
        &program.id(),
    );
    let (personal_position_key, __bump) = Pubkey::find_program_address(
        &[POSITION_SEED.as_bytes(), nft_mint_key.to_bytes().as_ref()],
        &program.id(),
    );
    let instructions = program
        .request()
        .accounts(raydium_accounts::OpenPositionV2 {
            payer: program.payer(),
            position_nft_owner: nft_to_owner,
            position_nft_mint: nft_mint_key,
            position_nft_account: nft_ata_token_account,
            metadata_account: metadata_account_key,
            pool_state: pool_account_key,
            protocol_position: protocol_position_key,
            tick_array_lower,
            tick_array_upper,
            personal_position: personal_position_key,
            token_account_0: user_token_account_0,
            token_account_1: user_token_account_1,
            token_vault_0,
            token_vault_1,
            rent: sysvar::rent::id(),
            system_program: system_program::id(),
            token_program: spl_token::id(),
            associated_token_program: spl_associated_token_account::id(),
            metadata_program: mpl_token_metadata::ID,
            token_program_2022: spl_token_2022::id(),
            vault_0_mint: token_mint_0,
            vault_1_mint: token_mint_1,
        })
        .accounts(remaining_accounts)
        .args(raydium_instruction::OpenPositionV2 {
            liquidity,
            amount_0_max,
            amount_1_max,
            tick_lower_index,
            tick_upper_index,
            tick_array_lower_start_index,
            tick_array_upper_start_index,
            with_matedata,
            base_flag: None,
        })
        .instructions()?;
    Ok(instructions)
}

pub fn increase_liquidity_instr(
    config: &ClientConfig,
    pool_account_key: Pubkey,
    token_vault_0: Pubkey,
    token_vault_1: Pubkey,
    token_mint_0: Pubkey,
    token_mint_1: Pubkey,
    nft_mint_key: Pubkey,
    user_token_account_0: Pubkey,
    user_token_account_1: Pubkey,
    remaining_accounts: Vec<AccountMeta>,
    liquidity: u128,
    amount_0_max: u64,
    amount_1_max: u64,
    tick_lower_index: i32,
    tick_upper_index: i32,
    tick_array_lower_start_index: i32,
    tick_array_upper_start_index: i32,
) -> Result<Vec<Instruction>> {
    let payer = config.payer.clone();
    let user: Pubkey = config.user.clone();
    let url = Cluster::Custom(config.http_url.clone(), config.ws_url.clone());
    // Client.
    let client = Client::new(url, Arc::new(payer));
    let program = client.program(config.raydium_v3_program)?;
    let nft_ata_token_account =
        spl_associated_token_account::get_associated_token_address(&program.payer(), &nft_mint_key);
    let (tick_array_lower, __bump) = Pubkey::find_program_address(
        &[
            TICK_ARRAY_SEED.as_bytes(),
            pool_account_key.to_bytes().as_ref(),
            &tick_array_lower_start_index.to_be_bytes(),
        ],
        &config.raydium_v3_program,
    );
    let (tick_array_upper, __bump) = Pubkey::find_program_address(
        &[
            TICK_ARRAY_SEED.as_bytes(),
            pool_account_key.to_bytes().as_ref(),
            &tick_array_upper_start_index.to_be_bytes(),
        ],
        &config.raydium_v3_program,
    );
    let (protocol_position_key, __bump) = Pubkey::find_program_address(
        &[
            POSITION_SEED.as_bytes(),
            pool_account_key.to_bytes().as_ref(),
            &tick_lower_index.to_be_bytes(),
            &tick_upper_index.to_be_bytes(),
        ],
        &config.raydium_v3_program,
    );
    let (personal_position_key, __bump) = Pubkey::find_program_address(
        &[POSITION_SEED.as_bytes(), nft_mint_key.to_bytes().as_ref()],
        &config.raydium_v3_program,
    );

    let instructions = program
        .request()
        .accounts(raydium_accounts::IncreaseLiquidityV2 {
            nft_owner: program.payer(),
            nft_account: nft_ata_token_account,
            pool_state: pool_account_key,
            protocol_position: protocol_position_key,
            personal_position: personal_position_key,
            tick_array_lower,
            tick_array_upper,
            token_account_0: user_token_account_0,
            token_account_1: user_token_account_1,
            token_vault_0,
            token_vault_1,
            token_program: spl_token::id(),
            token_program_2022: spl_token_2022::id(),
            vault_0_mint: token_mint_0,
            vault_1_mint: token_mint_1,
        })
        .accounts(remaining_accounts)
        .args(raydium_instruction::IncreaseLiquidityV2 {
            liquidity,
            amount_0_max,
            amount_1_max,
            base_flag: None,
        })
        .instructions()?;
    Ok(instructions)
}

pub fn decrease_liquidity_instr(
    config: &ClientConfig,
    pool_account_key: Pubkey,
    token_vault_0: Pubkey,
    token_vault_1: Pubkey,
    token_mint_0: Pubkey,
    token_mint_1: Pubkey,
    nft_mint_key: Pubkey,
    user_token_account_0: Pubkey,
    user_token_account_1: Pubkey,
    remaining_accounts: Vec<AccountMeta>,
    liquidity: u128,
    amount_0_min: u64,
    amount_1_min: u64,
    tick_lower_index: i32,
    tick_upper_index: i32,
    tick_array_lower_start_index: i32,
    tick_array_upper_start_index: i32,
) -> Result<Vec<Instruction>> {
    let url = Cluster::Custom(config.http_url.clone(), config.ws_url.clone());
    let payer = config.payer.clone();
    // Client.
    let client = Client::new(url, Arc::new(payer));
    let program = client.program(config.raydium_v3_program)?;
    let nft_ata_token_account =
        spl_associated_token_account::get_associated_token_address(&program.payer(), &nft_mint_key);
    let (personal_position_key, __bump) = Pubkey::find_program_address(
        &[POSITION_SEED.as_bytes(), nft_mint_key.to_bytes().as_ref()],
        &config.raydium_v3_program,
    );
    let (protocol_position_key, __bump) = Pubkey::find_program_address(
        &[
            POSITION_SEED.as_bytes(),
            pool_account_key.to_bytes().as_ref(),
            &tick_lower_index.to_be_bytes(),
            &tick_upper_index.to_be_bytes(),
        ],
        &config.raydium_v3_program,
    );
    let (tick_array_lower, __bump) = Pubkey::find_program_address(
        &[
            TICK_ARRAY_SEED.as_bytes(),
            pool_account_key.to_bytes().as_ref(),
            &tick_array_lower_start_index.to_be_bytes(),
        ],
        &config.raydium_v3_program,
    );
    let (tick_array_upper, __bump) = Pubkey::find_program_address(
        &[
            TICK_ARRAY_SEED.as_bytes(),
            pool_account_key.to_bytes().as_ref(),
            &tick_array_upper_start_index.to_be_bytes(),
        ],
        &config.raydium_v3_program,
    );
    let instructions = program
        .request()
        .accounts(raydium_accounts::DecreaseLiquidityV2 {
            nft_owner: program.payer(),
            nft_account: nft_ata_token_account,
            personal_position: personal_position_key,
            pool_state: pool_account_key,
            protocol_position: protocol_position_key,
            token_vault_0,
            token_vault_1,
            tick_array_lower,
            tick_array_upper,
            recipient_token_account_0: user_token_account_0,
            recipient_token_account_1: user_token_account_1,
            token_program: spl_token::id(),
            token_program_2022: spl_token_2022::id(),
            memo_program: spl_memo::id(),
            vault_0_mint: token_mint_0,
            vault_1_mint: token_mint_1,
        })
        .accounts(remaining_accounts)
        .args(raydium_instruction::DecreaseLiquidityV2 {
            liquidity,
            amount_0_min,
            amount_1_min,
        })
        .instructions()?;
    Ok(instructions)
}

#[derive(Default, Clone, Debug)]
pub struct RebalancingV1TaskInput {
    pub request: Pubkey,
    pub user: Pubkey,
    pub callback: Callback,
}
impl RebalancingV1TaskInput {
    // We should only generate rebalancing once per request. Any retry logic
    // should use the same generated result. No grinding allowed.
    pub fn compile(&self) -> Result<RebalancingV1CompiledTask, SbError> {

        Ok(RebalancingV1CompiledTask {
            request: self.request,
            user: self.user,
            callback: self.callback.clone(),
        })
    }
}
