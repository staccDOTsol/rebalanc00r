use crate::*;

use async_trait::async_trait;

// pub use solana_program::sysvar::instructions::ID as SYSVAR_INSTRUCTIONS_ID;

pub enum RandomnessTaskInput {
    SimpleRandomnessV1(SimpleRandomnessV1TaskInput),
}

pub enum RandomnessTaskOutput {
    SimpleRandomnessV1(SimpleRandomnessV1CompiledTask),
}

#[async_trait]
pub trait RandomnessTrait {
    fn build_ixn(
        &self,
        ctx: &'static ServiceContext,
        payer: Pubkey,
        switchboard_function: Pubkey,
        switchboard_service: Pubkey,
        enclave_signer: Pubkey,
    ) -> Result<Instruction, SbError>;

    fn id(&self) -> String;

    async fn process(
        &self,
        enclave_signer: Arc<Keypair>,
        recent_blockhash: Arc<(Hash, u64)>,
    ) -> Result<Signature, SbError> {
        let ctx: &'static ServiceContext = ServiceContext::get_or_init().await;

        let payer = Arc::new(ctx.payer.read().await);

        let ix = self.build_ixn(
            &ctx,
            payer.pubkey(),
            ctx.function,
            ctx.service,
            enclave_signer.pubkey(),
        )?;

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
                    &self.id(),
                    signature
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
