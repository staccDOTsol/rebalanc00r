use crate::*;

// Summary
// * This stores the context used by the rest of the service to easily share context and rpc
// * Should never change for the lifetime of the service
// * It might make sense to add better parsing to SolanaServicesEnvironment so it can parse strings -> pubkeys, url -> RpcClient so we can have a single env to share across.

// TODO:
// * Fetch telemetry endpoint to send signatures / logs / metrics for customer to monitor themselves

pub static SWITCHBOARD_CONTEXT: OnceCell<ServiceContext> = OnceCell::const_new();

#[derive(Clone)]
pub struct ServiceContext {
    pub rpc: Arc<RpcClient>,
    pub payer: Arc<RwLock<Keypair>>,
    pub service: Pubkey,
    pub service_worker: Pubkey,
    pub attestation_queue: Pubkey,
    pub function: Pubkey,
    pub randomness_service_state: Pubkey,
    pub randomness_service_wallet: Pubkey,
}

impl ServiceContext {
    pub async fn get_or_init() -> &'static ServiceContext {
        SWITCHBOARD_CONTEXT
            .get_or_init(|| async {
                match ServiceContext::initialize().await {
                    Ok(ctx) => ctx,
                    Err(err) => {
                        error!("Failed to initialize service context: {}", err);
                        panic!("Failed to initialize service context: {}", err);
                    }
                }
            })
            .await
    }

    pub async fn initialize() -> Result<Self, SbError> {
        let env = SolanaServiceEnvironment::parse()?;

        // Use the CLUSTER env variable to load our function/service account to yield the authority (needed to fetch our secrets)
        let default_rpc = Arc::new(RpcClient::new_with_commitment(
            env.default_rpc_url(),
            CommitmentConfig {
                commitment: CommitmentLevel::Confirmed,
            },
        ));

        let service_pubkey =
            Pubkey::from_str(&env.service_key).expect("Failed to parse SERVICE_KEY");
        let service_data =
            FunctionServiceAccountData::fetch_async(&default_rpc, service_pubkey).await?;
        println!("Service data: {:#?}", service_data);
        if service_data.service_worker == Pubkey::default() {
            return Err(SbError::Message("Service worker not set"));
        }

        let secrets = switchboard_solana::fetch_secrets(SECRETS_AUTHORITY, None).await?;

        let payer = if let Some(secret) = secrets.keys.get("SERVICE_PAYER_SECRET") {
            info!("[SECRET] Found secret for SERVICE_PAYER_SECRET");
            let kp = read_keypair(&mut std::io::Cursor::new(&secret))
                .expect("Failed to read SERVICE_PAYER_SECRET");
            info!("[ENV] Payer: {}", kp.pubkey());
            Arc::new(RwLock::new(kp))
        } else {
            return Err(SbError::Message("SERVICE_PAYER_SECRET not found"));
        };

        let rpc = if let Some(rpc_url) = secrets.keys.get("SERVICE_RPC_URL") {
            info!("[SECRET] Found secret for SERVICE_RPC_URL");
            Arc::new(RpcClient::new_with_commitment(
                rpc_url.clone(),
                CommitmentConfig {
                    commitment: CommitmentLevel::Confirmed, // TODO: should this be confirmed? Maybe should be controlled by a secret?
                },
            ))
        } else {
            return Err(SbError::Message("SERVICE_RPC_URL not found"));
        };

        // TODO: fetch telemetry endpoint to send signatures / logs / metrics for customer to monitor themselves

        let attestation_queue = service_data.attestation_queue;
        let attestation_queue_data =
            AttestationQueueAccountData::fetch_async(&rpc, attestation_queue).await?;

        // Make sure the function exists
        let function_data = FunctionAccountData::fetch_async(&rpc, service_data.function).await?;

        let randomness_service_state =
            Pubkey::find_program_address(&[b"STATE"], &RandomnessServiceID).0;
        let randomness_service_wallet =
            get_associated_token_address(&randomness_service_state, &NativeMint::ID);

        Ok(Self {
            rpc,
            payer,
            service: service_pubkey,
            service_worker: service_data.service_worker,
            attestation_queue,
            function: service_data.function,
            randomness_service_state,
            randomness_service_wallet,
        })
    }

    pub async fn fetch_service_data(&self) -> Result<FunctionServiceAccountData, SbError> {
        FunctionServiceAccountData::fetch_async(&self.rpc, self.service).await
    }

    pub async fn fetch_function_data(&self) -> Result<FunctionAccountData, SbError> {
        FunctionAccountData::fetch_async(&self.rpc, self.function).await
    }

    // TODO: add method to fetch all accounts with get_multiple_account_infos
}
