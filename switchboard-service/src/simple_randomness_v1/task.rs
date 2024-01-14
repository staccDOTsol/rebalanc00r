use crate::tasks::RandomnessTrait;
use crate::*;
use futures_util::stream::FuturesOrdered;
use solana_program::sysvar::instructions::ID as SYSVAR_INSTRUCTIONS_ID;

#[derive(Default, Clone, Debug)]
pub struct SimpleRandomnessV1TaskInput {
    pub request: Pubkey,
    pub user: Pubkey,
    pub num_bytes: u8,
    pub callback: Callback,
}

impl SimpleRandomnessV1TaskInput {
    // We should only generate randomness once per request. Any retry logic
    // should use the same generated result. No grinding allowed.
    pub fn compile(&self) -> Result<SimpleRandomnessV1CompiledTask, SbError> {
        let mut randomness_bytes: Vec<u8> = vec![0u8; self.num_bytes as usize];
        Gramine::read_rand(&mut randomness_bytes)?;

        Ok(SimpleRandomnessV1CompiledTask {
            request: self.request,
            user: self.user,
            callback: self.callback.clone(),
            randomness_bytes,
        })
    }
}

#[derive(Clone, Debug)]
pub struct SimpleRandomnessV1CompiledTask {
    pub request: Pubkey,
    pub user: Pubkey,
    pub callback: Callback,
    pub randomness_bytes: Vec<u8>,
}

impl RandomnessTrait for SimpleRandomnessV1CompiledTask {
    fn id(&self) -> String {
        return self.request.to_string();
    }

    fn build_ixn(
        &self,
        ctx: &'static ServiceContext,
        payer: Pubkey,
        switchboard_function: Pubkey,
        switchboard_service: Pubkey,
        enclave_signer: Pubkey,
    ) -> Result<Instruction, SbError> {
        let mut ixn_data = get_ixn_discriminator("simple_randomness_v1_settle").to_vec(); // TODO: hardcode this

        // First add the length of the vec
        ixn_data.append(&mut self.randomness_bytes.len().to_le_bytes().to_vec());

        // Then add the vec elements
        ixn_data.append(&mut self.randomness_bytes.clone());

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
pub struct SimpleRandomnessV1CompiledTaskBatch {
    pub tasks: Vec<SimpleRandomnessV1CompiledTask>,
    pub enclave_signer: Arc<Keypair>,
    pub recent_blockhash: Arc<(Hash, u64)>,
}
impl SimpleRandomnessV1CompiledTaskBatch {
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
