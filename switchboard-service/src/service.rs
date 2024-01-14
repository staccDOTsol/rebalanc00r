use crate::*;

use solana_randomness_service::{SimpleRandomnessV1Account, SimpleRandomnessV1RequestedEvent};
use tokio::time::sleep;
use tokio::{join, try_join};

use futures::{Future, StreamExt};
use futures_util::future::{join_all, try_join};

use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle as ThreadJoinHandle;
use std::{collections::HashMap, default::Default};

use base64::{engine::general_purpose, Engine as _};

static DEFAULT_FETCH_INTERVAL: u64 = 5;

#[derive(Default, Clone, Debug)]
pub enum SolanaServiceStatus {
    #[default]
    Initializing,
    Ready,
}

#[derive(Default, Clone, Debug)]
pub enum SignerStatus {
    #[default]
    NotReady,
    Ready,
}

pub struct SolanaService {
    ////////////////////////////////////////
    // Status / Health
    ////////////////////////////////////////
    pub status: SolanaServiceStatus,
    pub health: &'static SwitchboardHealth,

    ////////////////////////////////////////
    // Context
    ////////////////////////////////////////
    pub secure_signer: Arc<SecureSigner>,
    pub pubsub_client: Arc<PubsubClient>,

    ////////////////////////////////////////
    // On-Chain State
    ////////////////////////////////////////
    pub recent_blockhash: Arc<RwLock<(Hash, u64)>>,
    pub service_account: Arc<RwLock<FunctionServiceAccountData>>,
    pub request_accounts: Arc<DashMap<Pubkey, SimpleRandomnessV1Account>>,
    pub func_signer_rotation_interval: Arc<RwLock<i64>>,

    ////////////////////////////////////////
    // Cache
    ////////////////////////////////////////
    // pub in_flight_requests: Arc<DashSet<Pubkey>>,

    ////////////////////////////////////////
    // Worker
    ////////////////////////////////////////
    pub task_queue: Arc<Injector<RebalancingV1TaskInput>>,
}

impl SolanaService {
    /// Initialize a new Solana oracle with a cache.
    pub async fn new(
        task_queue: Arc<Injector<RebalancingV1TaskInput>>,
        secure_signer: Arc<SecureSigner>,
    ) -> Result<Self, SbError> {
        let ctx: &'static ServiceContext = ServiceContext::get_or_init().await;

        let ws_url = ctx
            .rpc
            .url()
            .replace("https://", "wss://")
            .replace("http://", "ws://");
        let pubsub_client = PubsubClient::new(ws_url.as_str()).await.unwrap();

        let worker_status = SolanaServiceStatus::Initializing;
        info!("Status: {:?}", worker_status);

        let service_data = FunctionServiceAccountData::fetch_async(&ctx.rpc, ctx.service).await?;
        let function_data =
            FunctionAccountData::fetch_async(&ctx.rpc, service_data.function).await?;

        let blockhash = retry!(
            5,
            250,
            ctx.rpc
                .get_latest_blockhash_with_commitment(CommitmentConfig::confirmed())
                .await
        )
        .await;

        Ok(Self {
            // Status / Health
            status: worker_status,
            health: SwitchboardHealth::get_or_init().await,

            // Signers
            secure_signer,
            pubsub_client: Arc::new(pubsub_client),

            // On-Chain State
            recent_blockhash: Arc::new(RwLock::new(blockhash.unwrap_or_default())),
            service_account: Arc::new(RwLock::new(service_data)),
            request_accounts: Arc::new(DashMap::new()),
            func_signer_rotation_interval: Arc::new(RwLock::new(
                function_data.services_signer_rotation_interval,
            )),

            // Cache
            // in_flight_requests: Arc::new(DashSet::new()),

            // Worker
            task_queue,
        })
    }

    pub async fn initialize(&mut self) -> Result<(), SbError> {
        // Rotate and set the signer
        match retry!(5, 1000, self.secure_signer.rotate_signer().await).await {
            Ok(_) => {}
            Err(e) => {
                error!("Failed to rotate signer: {:?}", e);
                return Err(e);
            }
        }

        // Log statuses
        self.status = SolanaServiceStatus::Ready;
        info!("Status: {:?}", self.status);

        // start health checker to signal k8s readiness
        self.health.set_is_ready().await;

        println!("Initialization complete");

        Ok(())
    }

    /// Start the Solana oracle and watch the chain for functions to execute.
    pub async fn start(
        &mut self,
        task_queue_rx: UnboundedReceiver<RebalancingV1CompiledTask>,
        rotate_signer_tx: Arc<Sender<u8>>,
    ) {
        println!("Starting routines ...");
        // TODO: these should all be started in a single OS thread
        tokio::select! {
            _ = self.start_workers(task_queue_rx) => {
                panic!("start_workers returned unexpectedly");
            }
            _ = self.rotate_signer_routine(rotate_signer_tx.as_ref()) => {
                panic!("start_rotate_signer_routine returned unexpectedly");
            }
            _ = self.watch_service_account() => {
                panic!("watch_service_account returned unexpectedly");
            }
            _ = self.poll_service_account() => {
                panic!("poll_service_account returned unexpectedly");
            }
            _ = self.watch_function_account() => {
                panic!("watch_function_account returned unexpectedly");
            }
            _ = self.poll_function_account() => {
                panic!("poll_function_account returned unexpectedly");
            }
            _ = self.watch_events() => {
                panic!("watch_events returned unexpectedly");
            }
            _ = self.watch_blockhash() => {
                panic!("watch_blockhash returned unexpectedly");
            }
            _ = self.watch_request_accounts() => {
                panic!("watch_request_accounts returned unexpectedly");
            }
        }

        // TODO: start worker threads in a new OS thread to watch for containers to execute

        panic!("Solana worker crashed");
    }

    // TODO: this function should first check if a signer rotation is needed based on the services config
    // If not then it should schedule the next rotation and return
    /// Periodically rotate the enclave signer to prevent it from being compromised.
    #[routine(interval = 15, skip_first_tick)]
    async fn rotate_signer_routine(&self, rotate_signer_tx: &Sender<u8>) -> Result<(), SbError> {
        let func_signer_rotation_interval = *self.func_signer_rotation_interval.read().await;

        if self
            .service_account
            .read()
            .await
            .ready_for_quote_rotation(func_signer_rotation_interval)
        {
            // Send a message to the mpsc channel
            match rotate_signer_tx.send(1).await {
                Ok(_) => {}
                Err(e) => {
                    error!("Failed to send rotate_signer_tx: {:?}", e);
                    return Err(SbError::CustomError {
                        message: "Failed".to_string(),
                        source: Arc::new(e),
                    });
                }
            }
        }

        Ok(())
    }

    async fn process_ix_batch(
        tasks: Vec<RebalancingV1CompiledTask>,
        secure_signer: Arc<SecureSigner>,
        recent_blockhash: Arc<RwLock<(Hash, u64)>>,
    ) {
        if tasks.is_empty() {
            return;
        }

        let enclave_signer = match secure_signer.load_signer(None).await {
            Err(e) => {
                error!("Failed to load signer: {:?}", e);
                // TODO: re-add these to the queue
                return;
            }
            Ok(enclave_signer) => enclave_signer,
        };

        // Read the recent blockhash
        let recent_blockhash = Arc::new(*recent_blockhash.read().await);

        let batch = RebalancingV1CompiledTaskBatch {
            // ctx: ctx.clone(),
            tasks: tasks.clone(),
            enclave_signer: enclave_signer.clone(),
            recent_blockhash: recent_blockhash.clone(),
        };

        batch.process().await
    }

    // Basically just handles batching ixns into groups of 10 so we dont need to keep calling read on RwLocks
    async fn start_workers(&self, mut tx_queue: UnboundedReceiver<RebalancingV1CompiledTask>) {
        let ctx: &'static ServiceContext = ServiceContext::get_or_init().await;

        let rpc = ctx.rpc.clone();
        let secure_signer = self.secure_signer.clone();
        let payer = ctx.payer.clone();
        let recent_blockhash = self.recent_blockhash.clone();

        let mut tasks: Vec<RebalancingV1CompiledTask> = Vec::with_capacity(10);
        let batch_size = 10; // Define your batch size
        let timeout_duration = Duration::from_millis(100); // Adjust as needed

        loop {
            let mut timeout = tokio::time::sleep(timeout_duration);

            tokio::select! {
                Some(task) = tx_queue.recv() => {
                    tasks.push(task);
                    if tasks.len() == batch_size {
                        SolanaService::process_ix_batch(tasks.clone(), secure_signer.clone(), recent_blockhash.clone()).await;
                        tasks = Vec::with_capacity(10);
                    }
                }
                _ = tokio::time::sleep(timeout_duration) => {
                    if !tasks.is_empty() {
                        SolanaService::process_ix_batch(tasks.clone(), secure_signer.clone(), recent_blockhash.clone()).await;
                        tasks = Vec::with_capacity(10);
                    }
                }
                else => {
                    // Channel is closed, process remaining tasks and exit loop
                    if !tasks.is_empty() {
                        SolanaService::process_ix_batch(tasks.clone(), secure_signer.clone(), recent_blockhash.clone()).await;
                        tasks = Vec::with_capacity(10);
                    }
                    break;
                }
            }
        }
    }

    /// Periodically fetch the account data for the service account.
    #[routine(interval = 30)]
    async fn poll_service_account(&self) {
        let ctx: &'static ServiceContext = ServiceContext::get_or_init().await;

        match ctx.fetch_service_data().await {
            Ok(service_account) => {
                self.handle_service_data(service_account).await;
            }
            Err(e) => {
                error!("Failed to fetch service account: {:?}", e);
            }
        }
    }

    async fn watch_service_account(&self) {
        let ctx: &'static ServiceContext = ServiceContext::get_or_init().await;

        let mut retry_count = 0;
        let max_retries = 3;
        let mut delay = Duration::from_millis(500); // start with a 500ms delay

        loop {
            // Attempt to connect
            let connection_result = self
                .pubsub_client
                .account_subscribe(
                    &ctx.service,
                    Some(RpcAccountInfoConfig {
                        encoding: Some(UiAccountEncoding::Base64),
                        ..Default::default()
                    }),
                )
                .await;

            match connection_result {
                Ok((mut stream, _handler)) => {
                    retry_count = 0; // Reset retry count on successful connection
                    delay = Duration::from_millis(500); // Reset delay on successful connection

                    // Process events if connection is successful
                    while let Some(event) = stream.next().await {
                        match event.value.data {
                            solana_account_decoder::UiAccountData::Binary(blob, encoding) => {
                                match encoding {
                                    UiAccountEncoding::Binary => continue,
                                    UiAccountEncoding::Base58 => continue,
                                    UiAccountEncoding::Base64 => {
                                        if let Ok(decoded) = general_purpose::STANDARD.decode(blob)
                                        {
                                            if let Ok(service_account) =
                                                FunctionServiceAccountData::try_deserialize(
                                                    &mut &decoded[..],
                                                )
                                            {
                                                self.handle_service_data(service_account).await;
                                            }
                                        }
                                    }
                                    UiAccountEncoding::Base64Zstd => continue,
                                    _ => continue,
                                }
                            }
                            _ => continue,
                        }
                    }

                    error!("[ACCOUNT][WEBSOCKET] connection closed, attempting to reconnect...");
                }
                Err(e) => {
                    error!("[ACCOUNT][WEBSOCKET] Failed to connect: {:?}", e);
                    if retry_count >= max_retries {
                        error!("[ACCOUNT][WEBSOCKET] Maximum retry attempts reached, aborting...");
                        break;
                    }

                    tokio::time::sleep(delay).await; // wait before retrying
                    retry_count += 1;
                    delay = std::cmp::min(delay * 2, Duration::from_secs(5)); // Double the delay for next retry, up to 5 seconds
                }
            }
        }
    }

    async fn handle_service_data(&self, service_account: FunctionServiceAccountData) {
        // println!("Set service_data: {:?}", service_account);
        *self.service_account.write().await = service_account;
    }

    /// Periodically fetch the account data for the service account.
    #[routine(interval = 30)]
    async fn poll_function_account(&self) {
        let ctx: &'static ServiceContext = ServiceContext::get_or_init().await;

        match ctx.fetch_function_data().await {
            Ok(function_account) => {
                self.handle_function_data(function_account).await;
            }
            Err(e) => {
                error!("Failed to fetch function account: {:?}", e);
            }
        }
    }

    async fn watch_function_account(&self) {
        let ctx: &'static ServiceContext = ServiceContext::get_or_init().await;

        let mut retry_count = 0;
        let max_retries = 3;
        let mut delay = Duration::from_millis(500); // start with a 500ms delay

        loop {
            // Attempt to connect
            let connection_result = self
                .pubsub_client
                .account_subscribe(
                    &ctx.function,
                    Some(RpcAccountInfoConfig {
                        encoding: Some(UiAccountEncoding::Base64),
                        ..Default::default()
                    }),
                )
                .await;

            match connection_result {
                Ok((mut stream, _handler)) => {
                    retry_count = 0; // Reset retry count on successful connection
                    delay = Duration::from_millis(500); // Reset delay on successful connection

                    // Process events if connection is successful
                    while let Some(event) = stream.next().await {
                        match event.value.data {
                            solana_account_decoder::UiAccountData::Binary(blob, encoding) => {
                                match encoding {
                                    UiAccountEncoding::Binary => continue,
                                    UiAccountEncoding::Base58 => continue,
                                    UiAccountEncoding::Base64 => {
                                        if let Ok(decoded) = general_purpose::STANDARD.decode(blob)
                                        {
                                            if let Ok(function_account) =
                                                FunctionAccountData::try_deserialize(
                                                    &mut &decoded[..],
                                                )
                                            {
                                                self.handle_function_data(function_account).await;
                                            }
                                        }
                                    }
                                    UiAccountEncoding::Base64Zstd => continue,
                                    _ => continue,
                                }
                            }
                            _ => continue,
                        }
                    }

                    error!("[ACCOUNT][WEBSOCKET] connection closed, attempting to reconnect...");
                }
                Err(e) => {
                    error!("[ACCOUNT][WEBSOCKET] Failed to connect: {:?}", e);
                    if retry_count >= max_retries {
                        error!("[ACCOUNT][WEBSOCKET] Maximum retry attempts reached, aborting...");
                        break;
                    }

                    tokio::time::sleep(delay).await; // wait before retrying
                    retry_count += 1;
                    delay = std::cmp::min(delay * 2, Duration::from_secs(5)); // Double the delay for next retry, up to 5 seconds
                }
            }
        }
    }

    async fn handle_function_data(&self, function_account: FunctionAccountData) {
        if *self.func_signer_rotation_interval.read().await
            != function_account.services_signer_rotation_interval
        {
            *self.func_signer_rotation_interval.write().await =
                function_account.services_signer_rotation_interval;
        }
    }

    /// Stream websocket events for the request trigger event
    async fn watch_events(&self) {
        let mut retry_count = 0;
        let max_retries = 3;
        let mut delay = Duration::from_millis(500); // start with a 500ms delay

        loop {
            // Attempt to connect
            let connection_result = self
                .pubsub_client
                .logs_subscribe(
                    RpcTransactionLogsFilter::Mentions(vec![RandomnessServiceID.to_string()]),
                    RpcTransactionLogsConfig {
                        commitment: Some(CommitmentConfig::processed()),
                    },
                )
                .await;

            match connection_result {
                Ok((mut stream, _handler)) => {
                    retry_count = 0; // Reset retry count on successful connection
                    delay = Duration::from_millis(500); // Reset delay on successful connection

                    // Process events if connection is successful

                    while let Some(event) = stream.next().await {
                        let log: String = event.value.logs.join(" ");
                        for w in log.split(' ') {
                            let decoded = general_purpose::STANDARD.decode(w);
                            if decoded.is_err() {
                                continue;
                            }

                            let decoded = decoded.unwrap();
                            if decoded.len() < 8 {
                                continue;
                            }

                            let mut discriminator: [u8; 8] = [0u8; 8];
                            discriminator.copy_from_slice(&decoded[..8]);

                            match discriminator {
                                // request
                                SimpleRandomnessV1RequestedEvent::DISCRIMINATOR => {
                                    if let Ok(event) =
                                        SimpleRandomnessV1RequestedEvent::try_from_slice(
                                            &decoded[8..],
                                        )
                                    {
                                        self.handle_randomness_requested_event(event).await;
                                    }
                                }

                                _ => {
                                    continue;
                                }
                            }
                        }
                    }

                    error!("[EVENT][WEBSOCKET] connection closed, attempting to reconnect...");
                }
                Err(e) => {
                    error!("[EVENT][WEBSOCKET] Failed to connect: {:?}", e);
                    if retry_count >= max_retries {
                        error!("[EVENT][WEBSOCKET] Maximum retry attempts reached, aborting...");
                        break;
                    }

                    tokio::time::sleep(delay).await; // wait before retrying
                    retry_count += 1;
                    delay = std::cmp::min(delay * 2, Duration::from_secs(5)); // Double the delay for next retry, up to 5 seconds
                }
            }

            if retry_count >= max_retries {
                error!("[EVENT][WEBSOCKET] Maximum retry attempts reached, aborting...");
                break;
            }
        }
    }

    async fn handle_randomness_requested_event(&self, event: SimpleRandomnessV1RequestedEvent) {
        debug!("[EVENT][REQUEST] {:#?}", event);

        // // Check if in_flight_requests contains the request id
        // if self.in_flight_requests.contains(&event.request) {
        //     info!("[EVENT][REQUEST] Request already in flight");
        //     return;
        // }

        // Add to Injector queue
        self.task_queue.push(RebalancingV1TaskInput {
            request: event.request,
            user: event.user,
            callback: event.callback,
        });
    }

    /// Periodically fetch the Solana time from on-chain so we know when to execute functions.
    #[routine(interval = 3)]
    async fn watch_blockhash(&self) {
        let ctx: &'static ServiceContext = ServiceContext::get_or_init().await;

        let blockhash_result = tokio::join!(ctx
            .rpc
            .get_latest_blockhash_with_commitment(CommitmentConfig::confirmed()));

        if let Ok((blockhash, slot)) = blockhash_result.0 {
            let mut recent_blockhash = self.recent_blockhash.write().await;
            *recent_blockhash = (blockhash, slot);
        }
    }

    /// Periodically fetch the request accounts to catch missed events
    #[routine(interval = 10)]
    async fn watch_request_accounts(&self) {
        let ctx: &'static ServiceContext = ServiceContext::get_or_init().await;

        let program_accounts = match ctx
            .rpc
            .get_program_accounts_with_config(
                &RandomnessServiceID,
                RpcProgramAccountsConfig {
                    filters: Some(vec![RpcFilterType::Memcmp(Memcmp::new_raw_bytes(
                        0,
                        SimpleRandomnessV1Account::DISCRIMINATOR.to_vec(),
                    ))]),
                    account_config: RpcAccountInfoConfig {
                        encoding: Some(UiAccountEncoding::Base64), // TODO: Base64Zstd
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .await
        {
            Ok(accounts) => accounts,
            Err(e) => {
                error!("Failed to fetch program accounts: {:?}", e);
                return;
            }
        };

        for (request_pubkey, request_account) in program_accounts {
            // if self.in_flight_requests.contains(&request_pubkey) {
            //     continue;
            // }

            let request_state =
                match SimpleRandomnessV1Account::try_deserialize(&mut &request_account.data[..]) {
                    Ok(state) => state,
                    Err(e) => {
                        error!("Failed to deserialize request account: {:?}", e);
                        continue;
                    }
                };

            let task = RebalancingV1TaskInput {
                request: request_pubkey,
                user: request_state.user,
                callback: request_state.callback.into(),
            };

            self.task_queue.push(task);

            // self.in_flight_requests.insert(request_pubkey);
            debug!("[PROGRAM_ACCOUNTS] Found request: {:?}", request_pubkey);
        }
    }
}
