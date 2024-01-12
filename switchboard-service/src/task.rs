use crate::*;

use futures_util::stream::{FuturesOrdered, FuturesUnordered, StreamExt};
use solana_client::client_error::{ClientError, ClientErrorKind};
use solana_client::rpc_request::{RpcError, RpcResponseErrorData};
use solana_client::rpc_response::RpcSimulateTransactionResult;
use solana_program::instruction::InstructionError;
pub use solana_program::sysvar::instructions::ID as SYSVAR_INSTRUCTIONS_ID;
use solana_sdk::transaction::TransactionError;
use switchboard_solana::rust_decimal::prelude::Zero;

const REQUEST_ACCOUNT_DOESNT_EXIST_ERROR_STR: &str = "Program log: AnchorError caused by account: request. Error Code: AccountOwnedByWrongProgram. Error Number: 3007. Error Message: The given account is owned by a different program than expected.\nProgram log: Left:\nProgram log: 11111111111111111111111111111111";

#[derive(Clone, Debug)]
pub struct CompiledTask {
    pub request: Pubkey,
    pub user: Pubkey,
    pub num_bytes: u8,
    pub callback: Callback,
    pub randomness_bytes: Vec<u8>,
}
impl CompiledTask {
    pub fn build_ixn(
        &self,
        payer: Pubkey,
        switchboard_function: Pubkey,
        switchboard_service: Pubkey,
        enclave_signer: Pubkey,
    ) -> Result<Instruction, SbError> {
        let mut ixn_data = get_ixn_discriminator("settle").to_vec(); // TODO: hardcode this

        // First add the length of the vec
        ixn_data.append(&mut self.num_bytes.to_le_bytes().to_vec());

        // Then add the vec elements
        ixn_data.append(&mut self.randomness_bytes.clone());

        let randomness_state_pubkey =
            Pubkey::find_program_address(&[b"STATE"], &RandomnessServiceID).0;
        let randomness_escrow =
            get_associated_token_address(&randomness_state_pubkey, &NativeMint::ID);

        info!("Program State: {}", randomness_state_pubkey);
        info!("Program Wallet: {}", randomness_escrow);

        let request_escrow = get_associated_token_address(&self.request, &NativeMint::ID);
        info!("Request: {}", self.request);
        info!("User: {}", self.user);
        info!("Escrow: {}", request_escrow);

        let mut ixn = Instruction {
            program_id: RandomnessServiceID,
            data: ixn_data,
            accounts: vec![
                // Payer (mut, signer)
                AccountMeta::new(payer, true),
                // Request (mut)
                AccountMeta::new(self.request, false),
                // User (mut)
                AccountMeta::new(self.user, false),
                // Request Escrow (mut)
                AccountMeta::new(request_escrow, false),
                // State
                AccountMeta::new_readonly(randomness_state_pubkey, false),
                // State Wallet (mut)
                AccountMeta::new(randomness_escrow, false),
                // SWITCHBOARD
                AccountMeta::new_readonly(switchboard_function, false),
                AccountMeta::new_readonly(switchboard_service, false),
                AccountMeta::new_readonly(enclave_signer, true),
                // SystemProgram
                AccountMeta::new_readonly(SystemProgramID, false),
                // TokenProgram
                AccountMeta::new_readonly(TokenProgramID, false),
                // Callback PID
                AccountMeta::new_readonly(self.callback.program_id, false),
                // // Instructions Sysvar
                // AccountMeta::new_readonly(SYSVAR_INSTRUCTIONS_ID, false),
            ],
        };

        // Next, add all of the callback accounts
        for account in self.callback.accounts.iter() {
            // Exclude the randomness_state and randomness_request accounts to reduce number of accounts
            if account.pubkey == payer {
                continue;
            }

            if account.pubkey == self.request {
                continue;
            }

            if account.pubkey == randomness_state_pubkey {
                continue;
            }

            if account.pubkey == enclave_signer {
                continue;
            }

            ixn.accounts.push(account.into());
        }

        Ok(ixn)
    }

    pub async fn process(
        &self,
        enclave_signer: Arc<Keypair>,
        recent_blockhash: Arc<(Hash, u64)>,
    ) -> Result<Signature, SbError> {
        let ctx: &'static ServiceContext = ServiceContext::get_or_init().await;
        let payer = Arc::new(ctx.payer.read().await);

        let ix = self.build_ixn(
            payer.pubkey(),
            ctx.function,
            ctx.service,
            enclave_signer.pubkey(),
        )?;

        for (i, account) in ix.accounts.iter().enumerate() {
            info!("Ixn Account #{}: {}", i, account.pubkey);
        }

        let mut tx = Transaction::new_with_payer(&[ix.clone()], Some(&payer.pubkey()));
        let signers = vec![payer.as_ref(), enclave_signer.deref()];

        match tx.try_sign(&signers, recent_blockhash.0) {
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

        match ctx
            .rpc
            .send_and_confirm_transaction_with_spinner_and_config(
                &tx,
                CommitmentConfig {
                    commitment: CommitmentLevel::Processed,
                },
                RpcSendTransactionConfig {
                    preflight_commitment: Some(CommitmentLevel::Processed),
                    min_context_slot: Some(recent_blockhash.1),
                    ..Default::default()
                },
            )
            .await
        {
            Ok(signature) => {
                info!(
                    "[WORKER][SUCCESS] Responded to request ({}): {:#?}",
                    &self.request, signature
                );

                Ok(signature)
            }
            Err(e) => {
                error!(
                    "[WORKER][FAILURE] Failed to broadcast transaction: {:#?}",
                    e
                );

                Err(SbError::CustomError {
                    message: "Failed to broadcast transaction".to_string(),
                    source: Arc::new(e),
                })
            }
        }
    }
}

#[derive(Clone)]
pub struct CompiledTaskBatch {
    pub tasks: Vec<CompiledTask>,
    pub enclave_signer: Arc<Keypair>,
    pub recent_blockhash: Arc<(Hash, u64)>,
}
impl CompiledTaskBatch {
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
                    // TODO: handle this better and retry failures
                    match sb_err {
                        SbError::CustomError { message, source: e } => {
                            if message.starts_with("Failed to broadcast transaction") {
                                if let Some(client_err) = e.downcast_ref::<ClientError>() {
                                    match client_err.kind() {
                                        ClientErrorKind::RpcError(rpc_err) => {
                                            error!("Randomness Rpc Error: {:#?}", rpc_err);

                                            // Extract the rpc response error
                                            if let RpcError::RpcResponseError {
                                                code,
                                                message,
                                                data,
                                            } = rpc_err
                                            {
                                                error!(
                                                    "[{}] {} - Rpc Response Error: {:#?}",
                                                    code, message, data
                                                );
                                                if let RpcResponseErrorData::SendTransactionPreflightFailure(
                                                    RpcSimulateTransactionResult {
                                                        err: Some::<TransactionError>(tx_err),
                                                        logs,
                                                        accounts,
                                                        units_consumed,
                                                        return_data,
                                                    },
                                                ) = data
                                                {
                                                    let logs = logs.clone().unwrap_or_default().join("\n");
                                                    error!(
                                                        "Simulation Error: {:#?}",
                                                        tx_err
                                                    );

                                                    // @DEV - here we can handle the tx_error
                                                    match tx_err {
                                                        TransactionError::BlockhashNotFound => {
                                                            error!("BlockhashNotFound: {:#?}", tx_err);
                                                            // TODO: retry
                                                        },
                                                        TransactionError::InstructionError(idx, ix_error) => {
                                                            // @DEV - here we assume ixn 0 is the randomness ixn. if we add priority fees or compute units this will change.
                                                            if idx.is_zero() {
                                                                error!("Randomness Instruction Error: {:#?}", ix_error);
                                                                match ix_error {
                                                                    InstructionError::Custom(code) => {
                                                                        match code {
                                                                            // AccountOwnedByWrongProgram - no action needed
                                                                            3007 => {
                                                                                if logs.contains(&REQUEST_ACCOUNT_DOESNT_EXIST_ERROR_STR) {
                                                                                    // no action needed
                                                                                    info!("Randomness Request Already Settled");
                                                                                } else {
                                                                                    error!("[3007] Randomness Instruction Error: {:#?}\n{}", ix_error, logs);
                                                                                }
                                                                            }
                                                                            // handle other cases here
                                                                            _ => {
                                                                                error!("[{}] Randomness Instruction Error: {:#?}\n{}", code, ix_error, logs);
                                                                            }
                                                                        }
                                                                        // @DEV - here we assume that the only custom error is that the request has already been settled.
                                                                        // @TODO - we should handle this better
                                                                        error!("Randomness Request Already Settled");
                                                                    }
                                                                    _ => {
                                                                        error!("Randomness Instruction Error: {:#?}\n{}", ix_error, logs);
                                                                    }
                                                                }
                                                            } else {
                                                                error!("Instruction Error: {:#?}\n{}", ix_error, logs);
                                                            }
                                                        },

                                                        _ => {
                                                            error!("Simulation Tx Error: {:#?}\n{}", tx_err, logs);
                                                        }

                                                        // TransactionError::AccountInUse => todo!(),
                                                        // TransactionError::AccountLoadedTwice => todo!(),
                                                        // TransactionError::AccountNotFound => todo!(),
                                                        // TransactionError::ProgramAccountNotFound => todo!(),
                                                        // TransactionError::InsufficientFundsForFee => todo!(),
                                                        // TransactionError::InvalidAccountForFee => todo!(),
                                                        // TransactionError::AlreadyProcessed => todo!(),
                                                        // TransactionError::CallChainTooDeep => todo!(),
                                                        // TransactionError::MissingSignatureForFee => todo!(),
                                                        // TransactionError::InvalidAccountIndex => todo!(),
                                                        // TransactionError::SignatureFailure => todo!(),
                                                        // TransactionError::InvalidProgramForExecution => todo!(),
                                                        // TransactionError::SanitizeFailure => todo!(),
                                                        // TransactionError::ClusterMaintenance => todo!(),
                                                        // TransactionError::AccountBorrowOutstanding => todo!(),
                                                        // TransactionError::WouldExceedMaxBlockCostLimit => todo!(),
                                                        // TransactionError::UnsupportedVersion => todo!(),
                                                        // TransactionError::InvalidWritableAccount => todo!(),
                                                        // TransactionError::WouldExceedMaxAccountCostLimit => todo!(),
                                                        // TransactionError::WouldExceedAccountDataBlockLimit => todo!(),
                                                        // TransactionError::TooManyAccountLocks => todo!(),
                                                        // TransactionError::AddressLookupTableNotFound => todo!(),
                                                        // TransactionError::InvalidAddressLookupTableOwner => todo!(),
                                                        // TransactionError::InvalidAddressLookupTableData => todo!(),
                                                        // TransactionError::InvalidAddressLookupTableIndex => todo!(),
                                                        // TransactionError::InvalidRentPayingAccount => todo!(),
                                                        // TransactionError::WouldExceedMaxVoteCostLimit => todo!(),
                                                        // TransactionError::WouldExceedAccountDataTotalLimit => todo!(),
                                                        // TransactionError::DuplicateInstruction(_) => todo!(),
                                                        // TransactionError::InsufficientFundsForRent { account_index } => todo!(),
                                                        // TransactionError::MaxLoadedAccountsDataSizeExceeded => todo!(),
                                                        // TransactionError::InvalidLoadedAccountsDataSizeLimit => todo!(),
                                                        // TransactionError::ResanitizationNeeded => todo!(),
                                                        // TransactionError::ProgramExecutionTemporarilyRestricted { account_index } => todo!(),
                                                        // TransactionError::UnbalancedTransaction => todo!(),
                                                    }
                                                }
                                            }
                                        }
                                        ClientErrorKind::TransactionError(tx_err) => {
                                            error!("Randomness Tx Error: {:#?}", tx_err);
                                        }
                                        _ => {
                                            error!(
                                                "Failed to broadcast transaction: {:#?}",
                                                client_err
                                            );
                                        }
                                    }
                                } else {
                                    error!("Failed to broadcast transaction: {:#?}", e);
                                }
                            } else {
                                error!("Randomness Txn Error: {:#?}", e);
                            }
                        }
                        _ => {
                            error!("Randomness Txn Error: {:#?}", sb_err);
                            // TODO: should we retry?
                        }
                    }
                }
            }
        }
    }
}

#[derive(Default, Clone, Debug)]
pub struct RandomnessTask {
    pub request: Pubkey,
    pub user: Pubkey,
    pub num_bytes: u8,
    pub callback: Callback,
}
impl RandomnessTask {
    // We should only generate randomness once per request. Any retry logic
    // should use the same generated result. No grinding allowed.
    pub fn compile(&self) -> Result<CompiledTask, SbError> {
        let mut randomness_bytes: Vec<u8> = vec![0u8; self.num_bytes as usize];
        Gramine::read_rand(&mut randomness_bytes)?;

        Ok(CompiledTask {
            request: self.request,
            user: self.user,
            num_bytes: self.num_bytes,
            callback: self.callback.clone(),
            randomness_bytes,
        })
    }
}
