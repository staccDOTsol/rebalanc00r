use crate::*;

use switchboard_solana::solana_sdk::signer::Signer;
use tokio::runtime::Handle;

// TODO
// * We should watch service data and if signer rotated, we should trigger shutdown
// * We should generalize this and add to our SDK for easy use by other services

#[derive(Default, Clone, Debug, PartialEq)]
pub enum SignerStatus {
    #[default]
    None,
    Rotating,
    Ready,
}

pub struct SecureSigner {
    pub signer: ArcSwap<Keypair>,
    pub status: ArcSwap<SignerStatus>,
    pub ipfs: Arc<IpfsManager>,
    pub ctx: Arc<ServiceContext>,
}

impl SecureSigner {
    pub fn new(ctx: Arc<ServiceContext>) -> Result<Self, SbError> {
        let ipfs = Arc::new(IpfsManager::from_env()?);

        Ok(Self {
            signer: ArcSwap::from(Arc::new(Keypair::new())),
            status: ArcSwap::from(Arc::new(SignerStatus::None)),
            ipfs,
            ctx,
        })
    }

    /// Wait up to 2 minutes for the signer to be ready
    pub async fn load_signer(&self, retry_count: Option<u32>) -> Result<Arc<Keypair>, SbError> {
        let mut retry_count = retry_count.unwrap_or(1200); // TODO: re-evaluate this
        loop {
            if self.is_ready() {
                return Ok(self.signer.load_full());
            } else {
                tokio::time::sleep(Duration::from_millis(100)).await;
                retry_count -= 1;
                if retry_count == 0 {
                    panic!("Signer is not ready");
                }
            }
        }
    }

    pub fn is_ready(&self) -> bool {
        matches!(**self.status.load(), SignerStatus::Ready)
    }

    /// Get the signer, returning an error if the signer is not ready
    pub fn get_signer(&self) -> Result<Arc<Keypair>, SbError> {
        if self.is_ready() {
            Ok(self.signer.load_full())
        } else {
            Err(SbError::Message("SignerNotReady"))
        }
    }

    /// Get the signer without checking if it is ready
    pub fn get_signer_unsafe(&self) -> Arc<Keypair> {
        self.signer.load_full()
    }

    async fn get_ipfs_hash(&self, signer_pubkey: Pubkey) -> Result<Vec<u8>, SbError> {
        let quote = blocking_retry!(10, 1000, Gramine::generate_quote(&signer_pubkey.to_bytes()))?;
        // let quote = Gramine::generate_quote(&signer_pubkey.to_bytes())?;
        let cursor = std::io::Cursor::new(quote);

        let ipfs = IpfsManager::from_env()?;

        let handle = Handle::current();
        let (tx, mut rx) = mpsc::channel(1);

        handle
            .spawn_blocking(move || {
                let handle = Handle::current();
                handle.block_on(async move {
                    let add_result = ipfs.client.add(cursor).await.unwrap();
                    tx.send(add_result.hash).await.unwrap();
                });
            })
            .await
            .map_err(|_e| SbError::IpfsNetworkError)?;

        let cid = rx.recv().await.unwrap();
        info!("Uploaded quote to IPFS: {}", cid);

        let bytes = cid.from_base58().unwrap();
        info!("CID: {:?}", bytes);

        Ok(bytes)
    }

    pub async fn rotate_signer(&self) -> Result<(), SbError> {
        let old_signer = self.signer.load_full();
        let old_status = self.status.load_full();

        let mut signer = Keypair::new();

        // Post quote to IPFS with new keypair
        let cid = match self.get_ipfs_hash(signer.pubkey()).await {
            Ok(cid) => cid,
            Err(e) => {
                error!("Failed to post quote to IPFS: {:#?}", e);

                if *old_status != SignerStatus::Ready {
                    // Try to reset the status
                    match self.ctx.fetch_service_data().await {
                        Ok(service_data) => {
                            if service_data.enclave.enclave_signer == old_signer.pubkey() {
                                self.status.store(Arc::new(SignerStatus::Ready));
                            }
                        }
                        Err(e) => {
                            error!("Failed to fetch service data: {:#?}", e);
                        }
                    }
                }

                return Err(e);
            }
        };

        let mut registry_key = [0u8; 64];
        registry_key[0..cid.len()].clone_from_slice(&cid);

        // Send txns on-chain to request a new signer
        let mut service_data = self.ctx.fetch_service_data().await?;
        let function_data =
            FunctionAccountData::fetch_async(&self.ctx.rpc, self.ctx.function).await?;
        let ixn = ServiceRequestQuoteVerify::build_ix(
            &ServiceRequestQuoteVerifyAccounts {
                service: self.ctx.service,
                service_worker: service_data.service_worker,
                function: self.ctx.function,
                attestation_queue: self.ctx.attestation_queue,
                escrow_wallet: service_data.escrow_wallet,
                new_enclave_signer: signer.pubkey(),
                authority: service_data.authority,
            },
            &ServiceRequestQuoteVerifyParams {
                quote_registry: None,
                registry_key: registry_key.to_vec(),
            },
        )?;

        let blockhash = retry!(5, 1000, self.ctx.rpc.get_latest_blockhash().await)
            .await
            .map_err(|_| SbError::SolanaBlockhashError)?;

        let payer = self.ctx.payer.read().await;

        let txn = Transaction::new_signed_with_payer(
            &[ixn],
            Some(&payer.pubkey()),
            &[&payer, &signer],
            blockhash,
        );
        let signature = match self.ctx.rpc.send_and_confirm_transaction(&txn).await {
            Ok(signature) => signature,
            Err(e) => {
                error!("Failed to send ServiceRequestQuoteVerify ixn: {:#?}", e);

                // TODO: handle error better

                if *old_status != SignerStatus::Ready
                    && service_data.enclave.enclave_signer == old_signer.pubkey()
                {
                    self.status.store(Arc::new(SignerStatus::Ready));
                }

                return Err(SbError::CustomError {
                    message: "Failed to send ServiceRequestQuoteVerify ixn".to_string(),
                    source: Arc::new(e),
                });
            }
        };
        info!("Sent txn to request new signer: {:?}", signature);

        // Only change this status when we have requested a new signer
        self.status.store(Arc::new(SignerStatus::Rotating));

        // Wait for QVN to verify the quote
        // TODO: this is wasteful, we should be caching this for the service to also use
        let mut retry_count = 120; // 1 min
        while service_data.enclave.enclave_signer != signer.pubkey() {
            service_data = self.ctx.fetch_service_data().await?;
            tokio::time::sleep(Duration::from_millis(500)).await;
            retry_count -= 1;
            if retry_count == 0 {
                // Reset status if the old signer is still valid
                // TODO: check service config to ensure enclave_signer is still valid
                if service_data.enclave.enclave_signer == old_signer.pubkey() {
                    self.status.store(Arc::new(SignerStatus::Ready));
                }
                return Err(SbError::Message("Failed to rotate quote"));
            }
        }

        // Update signer and status
        self.signer.store(Arc::new(signer));

        // Set the status to ready
        self.status.store(Arc::new(SignerStatus::Ready));

        Ok(())
    }

    pub async fn start(&self, mut rx: mpsc::Receiver<u8>) {
        while rx.recv().await.is_some() {
            info!("Received signal to rotate signer");

            // Rotate the signer, try up to 5 times
            match retry!(5, 1000, self.rotate_signer().await).await {
                Ok(_) => info!("Successfully rotated signer"),
                Err(e) => error!("Failed to rotate signer: {:?}", e),
            }
        }

        panic!("The rotate signer rx channel has closed")
    }
}
