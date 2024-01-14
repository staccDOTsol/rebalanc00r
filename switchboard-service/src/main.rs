#![allow(dead_code, unused)]

pub use switchboard_solana::prelude::*;

pub use log::{debug, error, info, trace};

pub mod env;
pub use env::*;

pub mod tx;
pub use tx::*;

mod service;
pub use service::*;

mod context;
pub use context::*;

mod signer;
pub use signer::*;

pub mod simple_randomness_v1;
pub use simple_randomness_v1::*;

pub mod tasks;
pub use tasks::*;

pub use solana_randomness_service::{Callback, ID as RandomnessServiceID};

// Switchboard deps
pub use switchboard_common::{blocking_retry, retry};
pub use switchboard_common::{IpfsApi, IpfsClient, IpfsManager};
pub use switchboard_node::routine;
pub use switchboard_node::*;
pub use switchboard_solana::get_ixn_discriminator;

// Re-exports
pub use switchboard_solana::anchor_client;
pub use switchboard_solana::solana_client;
pub use switchboard_solana::solana_sdk;

// Solana / Anchor Deps
pub use anchor_client::Client;
pub use anchor_lang::system_program::ID as SystemProgramID;
pub use anchor_lang::Event;
pub use anchor_lang::{Discriminator, Owner};
pub use anchor_spl::associated_token::get_associated_token_address;
pub use anchor_spl::associated_token::ID as AssociatedTokenProgramID;
pub use anchor_spl::token::ID as TokenProgramID;
pub use solana_account_decoder::UiAccountEncoding;
pub use solana_client::nonblocking::pubsub_client::PubsubClient;
pub use solana_client::nonblocking::rpc_client::RpcClient;
pub use solana_client::rpc_config::RpcAccountInfoConfig;
pub use solana_client::rpc_config::RpcProgramAccountsConfig;
pub use solana_client::rpc_config::{
    RpcSendTransactionConfig, RpcTransactionLogsConfig, RpcTransactionLogsFilter,
};
pub use solana_client::rpc_filter::{Memcmp, RpcFilterType};
pub use solana_client::rpc_response::RpcBlockhash;
pub use solana_sdk::account::{Account, WritableAccount};
pub use solana_sdk::commitment_config::CommitmentConfig;
pub use solana_sdk::commitment_config::CommitmentLevel;
pub use solana_sdk::hash::Hash;
pub use solana_sdk::pubkey;
pub use solana_sdk::signature::{self, Signature};
pub use solana_sdk::signer::keypair::read_keypair;
pub use solana_sdk::signer::Signer;

// Tokio Deps
use tokio::runtime::Runtime;
pub use tokio::sync::mpsc::{self, Receiver, Sender, UnboundedReceiver, UnboundedSender};
pub use tokio::sync::Mutex;
pub use tokio::sync::OnceCell;
pub use tokio::sync::RwLock;
pub use tokio::task::JoinHandle;
pub use tokio::time::{interval, Interval};
pub use tokio_graceful_shutdown::{SubsystemBuilder, SubsystemHandle, Toplevel};

// Futures Deps
pub use futures::stream::{FuturesUnordered, StreamExt};
pub use futures::Future;

// Logging Deps
use tracing::level_filters::LevelFilter;
use tracing_subscriber::EnvFilter;

// Std Deps
pub use std::iter;
pub use std::ops::Deref;
pub use std::pin::Pin;
pub use std::str::FromStr;
pub use std::sync::Arc;
pub use std::time::{Duration, SystemTime};

// Misc Deps
pub use anyhow::Result;
pub use arc_swap::ArcSwap;
pub use async_trait::async_trait;
pub use base58::FromBase58;
pub use chrono::{format, NaiveDateTime, Utc};
pub use crossbeam::deque::{Injector, Steal, Stealer, Worker};
pub use dashmap::{DashMap, DashSet};
pub use serde::Deserialize;

// SERVICE WORKER CONSIDERATIONS
// An application running inside of an enclave cannot trust the system clock. A websocket network should be provided that heartbeats the system time every second.
// The service worker will only need a payer balance to approve or deny services. After the service is added, the service controls funds

// THREADS
// THREAD 1 (Main) - Poll service worker state and determine if any services need to be started or stopped
// THREAD 2 - N (Services Healthcheck) - Healthcheck for services to ensure they are always running. Should handle errors and try to re-pull containers if MrEnclave error is found.

// STATE
// * Service Worker State
// * Service Accounts and Function Accounts for the given service worker
// * Mapping of container id to expected container configuration

// TODO: Maybe this should be fetched by service authority?
pub const SECRETS_AUTHORITY: &str = "FiDmUK83DTc1ijEyVnwMoQwJ6W4gC2S8JhncKsheDQTJ";

#[tokio::main]
async fn main() -> Result<()> {
    // TODO: should remove in prod - no need to read .env files
    // Can be used to run locally if inside an enclave
    dotenvy::dotenv().ok();

    println!("Initializing randomness-service logger");

    // Init logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::DEBUG.into())
                .from_env_lossy(),
        )
        .init();

    let env = SolanaServiceEnvironment::parse()?;

    Toplevel::new(|s| async move {
        s.start(SubsystemBuilder::new(
            "main_thread",
            move |subsys| async move {
                let ctx: &'static ServiceContext = ServiceContext::get_or_init().await;
                let ctx_arc = Arc::new(ctx.clone());

                let secure_signer = Arc::new(SecureSigner::new(ctx_arc.clone())?);

                // Build a task queue to process requests sequentially and across worker pool
                let task_queue: Arc<Injector<SimpleRandomnessV1TaskInput>> =
                    Arc::new(Injector::new());

                // Create an unbounded mpsc channel to signal when an instruction is ready to be sent on-chain and await confirmation. These should be batched
                // to prevent thread exhaustion
                let (task_queue_tx, mut task_queue_rx) =
                    tokio::sync::mpsc::unbounded_channel::<SimpleRandomnessV1CompiledTask>();

                // Create a channel to signal when the signer should be rotated
                let (rotate_signer_tx, mut rotate_signer_rx) = tokio::sync::mpsc::channel::<u8>(1);

                // TODO: we should cascade down and start signer thread, worker thread, and service thread last.
                // Then we can have methods that return the channels needed to communicate to the threads and pass to the service thread.
                // This way all initialization is done before the service thread starts.

                // Start nested subsystems
                // worker_thread
                let mtask_queue = task_queue.clone();
                // let menv = env.clone();
                subsys.start(SubsystemBuilder::new(
                    "worker_thread",
                    move |s| async move {
                        worker_thread(
                            s,
                            mtask_queue.clone(),
                            task_queue_tx,
                        )
                        .await
                    },
                ));

                // signer_thread
                let menv = env.clone();
                let msecure_signer = secure_signer.clone();
                subsys.start(SubsystemBuilder::new(
                    "rotate_signer_thread",
                    move |s| async move {
                        rotate_signer_thread(s, msecure_signer.clone(), rotate_signer_rx).await
                    },
                ));

                // service_thread
                let mtask_queue = task_queue.clone();
                let menv = env.clone();
                let msecure_signer = secure_signer.clone();
                let mctx = ctx_arc.clone();
                subsys.start(SubsystemBuilder::new(
                    "service_thread",
                    move |s| async move {
                        service_thread(
                            s,
                            msecure_signer.clone(),
                            mtask_queue.clone(),
                            task_queue_rx,
                            rotate_signer_tx,
                        )
                        .await
                    },
                ));

                subsys.on_shutdown_requested().await;

                Ok::<(), SbError>(())
            },
        ));
    })
    .catch_signals()
    .handle_shutdown_requests(Duration::from_millis(5000))
    .await
    .map_err(Into::into)
}

async fn worker_thread(
    subsys: SubsystemHandle,
    task_queue: Arc<Injector<SimpleRandomnessV1TaskInput>>,
    task_queue_tx: UnboundedSender<SimpleRandomnessV1CompiledTask>,
) -> Result<(), SbError> {
    // we need a way to receive the shutdown request and start draining the queue
    let mut shutdown_requested = false;

    let mut futures = FuturesUnordered::new();
    for _ in 0..8 {
        let task_queue = task_queue.clone();
        let task_queue_tx = task_queue_tx.clone();

        let handle = tokio::spawn(async move {
            loop {
                match task_queue.steal() {
                    Steal::Success(task) => {
                        // Process the task...
                        let compiled_task: SimpleRandomnessV1CompiledTask =
                            match blocking_retry!(3, 10, task.compile()) {
                                Ok(compiled_task) => compiled_task,
                                Err(e) => {
                                    error!(
                                        "Failed to compile task for request {:?}: {:?}",
                                        task.request, e
                                    );
                                    task_queue.push(task);
                                    continue;
                                }
                            };

                        match task_queue_tx.send(compiled_task) {
                            Ok(_) => {
                                trace!("Sent task to tx_queue for request {:?}", task.request);
                            }
                            Err(e) => {
                                // Failed, try to re-push to task-queue
                                error!(
                                    "Failed to send task to tx_queue for request {:?}: {:?}",
                                    task.request, e
                                );
                                task_queue.push(task);
                            }
                        }
                    }
                    Steal::Retry => {
                        if shutdown_requested {
                            break;
                        }

                        // No task available, sleep for a short duration before retrying
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }
                    Steal::Empty => {
                        if shutdown_requested {
                            break;
                        }

                        // Queue is permanently empty, break the loop
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }
                }
            }
        });
        futures.push(handle);
    }

    // TODO: handle shutdown request and update healthcheck
    tokio::select! {
        _ = subsys.on_shutdown_requested() => {
            shutdown_requested = true;
            info!("Worker threads shutdown requested");

            // wait for shutdown timeout to close threads
        }
        _ = futures.next() => {
            if !shutdown_requested {
                info!("A worker thread crashed");
                subsys.request_shutdown();
            }
        }
    }

    Ok(())
}

async fn service_thread(
    subsys: SubsystemHandle,
    secure_signer: Arc<SecureSigner>,
    task_queue: Arc<Injector<SimpleRandomnessV1TaskInput>>,
    mut task_queue_rx: UnboundedReceiver<SimpleRandomnessV1CompiledTask>,
    rotate_signer_tx: Sender<u8>,
) -> Result<(), SbError> {
    let mut service = SolanaService::new(task_queue.clone(), secure_signer.clone())
        .await
        .unwrap();

    info!("randomness service initializing ...");

    // Initialize the set of services and start the healthcheck
    match service.initialize().await {
        Ok(_) => {
            info!("randomness service initialized");
        }
        Err(e) => {
            error!("Failed to initialize service: {:?}", e);
            subsys.request_shutdown();
            return Err(e);
        }
    }

    let rotate_signer_tx = Arc::new(rotate_signer_tx);

    // TODO: handle shutdown request and update healthcheck
    tokio::select! {
        _ = subsys.on_shutdown_requested() => {
            info!("Service thread shutdown requested");

            info!("Signaling health = NotReady");
            service.health.set_is_not_ready().await;
        }
        _ = service.start(task_queue_rx, rotate_signer_tx.clone()) => {
            info!("The service thread crashed");
        }
    }

    Ok(())
}

async fn rotate_signer_thread(
    subsys: SubsystemHandle,
    secure_signer: Arc<SecureSigner>,
    mut rotate_signer_rx: Receiver<u8>,
) -> Result<(), SbError> {
    let ipfs = match IpfsManager::from_env() {
        Ok(ipfs) => ipfs,
        Err(e) => {
            error!("Failed to initialize IPFS: {:?}", e);
            subsys.request_shutdown();
            return Err(e);
        }
    };

    // TODO: handle shutdown request and update healthcheck
    tokio::select! {
        _ = subsys.on_shutdown_requested() => {
            info!("Rotate signer thread shutdown requested");
        }
        _ = secure_signer.start(rotate_signer_rx) => {
            info!("The rotate signer thread crashed");
        }
    }

    Ok(())
}
