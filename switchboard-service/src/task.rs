use crate::*;

use futures_util::stream::{FuturesUnordered, StreamExt};

#[derive(Clone, Debug)]
pub struct CompiledTask {
    pub request: Pubkey,
    pub user: Pubkey,
    pub num_bytes: u32,
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

        let program_state_pubkey =
            Pubkey::find_program_address(&[b"STATE"], &RandomnessServiceID).0;

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
                AccountMeta::new(
                    get_associated_token_address(&self.request, &NativeMint::ID),
                    false,
                ),
                // State
                AccountMeta::new_readonly(program_state_pubkey, false),
                // State Wallet (mut)
                AccountMeta::new(
                    get_associated_token_address(&program_state_pubkey, &NativeMint::ID),
                    false,
                ),
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
            ],
        };

        // Next, add all of the callback accounts
        for account in self.callback.accounts.iter() {
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

        let ix = self.build_ixn(
            ctx.payer.pubkey(),
            ctx.function,
            ctx.service,
            enclave_signer.pubkey(),
        )?;
        let mut tx = Transaction::new_with_payer(&[ix.clone()], Some(&ctx.payer.pubkey()));
        let signers = vec![ctx.payer.as_ref(), enclave_signer.deref()];

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

        let mut futures = FuturesUnordered::new();
        for task in self.tasks.iter() {
            futures.push(task.process(self.enclave_signer.clone(), self.recent_blockhash.clone()));
        }

        while let Some(result) = futures.next().await {
            match result {
                Ok(signature) => {
                    info!("Signature: {:?}", result);
                }
                Err(e) => {
                    error!("Randomness Txn Error: {:?}", e);
                    // TODO: handle this better and retry failures
                }
            }
        }
    }
}

#[derive(Default, Clone, Debug)]
pub struct RandomnessTask {
    pub request: Pubkey,
    pub user: Pubkey,
    pub num_bytes: u32,
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
