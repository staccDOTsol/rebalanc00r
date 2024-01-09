use crate::*;

use base64::{engine::general_purpose, Engine as _};
use futures::StreamExt;

// NOTE:
// * This is currently not used but is intended to be a builder to map Anchor events to a handler
// * This should eventually live in the SDK
// * Want to see if there is a good way to use macros for some of this so users can build services quicker

// Stores a function to handle a given event type
pub type AnchorEventHandler<E, T> = Box<dyn Fn(&E) -> Pin<Box<T>> + Send + Sync + 'static>;

// Maps a discriminator to a handler
pub type AnchorEventHandlerMap<E, T> = Arc<DashMap<[u8; 8], Pin<AnchorEventHandler<E, T>>>>;

#[derive(Clone)]
pub struct EventWatcherBuilder<E, T>
where
    E: Event,
    T: Future<Output = ()> + Send + 'static,
{
    url: String,
    addresses: Vec<String>,
    handlers: AnchorEventHandlerMap<E, T>,
    commitment_config: CommitmentConfig,
}

impl<E, T> EventWatcherBuilder<E, T>
where
    E: Event,
    T: Future<Output = ()> + Send + 'static,
{
    fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
            addresses: Vec::new(),
            handlers: Arc::new(DashMap::new()),
            commitment_config: CommitmentConfig::processed(),
        }
    }

    fn add_address(&mut self, address: &str) -> &mut Self {
        self.addresses.push(address.to_string());
        self
    }

    fn add_handler(
        &mut self,
        discriminator: [u8; 8],
        handler: Pin<AnchorEventHandler<E, T>>,
    ) -> &mut Self {
        self.handlers.insert(discriminator, handler);
        self
    }

    async fn build(&self) -> Result<EventWatcher<E, T>, SbError> {
        let pubsub_client = PubsubClient::new(&self.url).await.unwrap();
        Ok(EventWatcher {
            pubsub_client: Arc::new(pubsub_client),
            addresses: self.addresses.clone(),
            handlers: self.handlers.clone(),
            commitment_config: self.commitment_config,
        })
    }
}

#[derive(Clone)]
pub struct EventWatcher<E, T>
where
    E: Event,
    T: Future<Output = ()> + Send + 'static,
{
    pubsub_client: Arc<PubsubClient>,
    addresses: Vec<String>,
    handlers: AnchorEventHandlerMap<E, T>,
    commitment_config: CommitmentConfig,
}

impl<E, T> EventWatcher<E, T>
where
    E: Event + 'static,
    T: Future<Output = ()> + Send + 'static,
{
    pub async fn start(&self) {
        let pubsub_client = self.pubsub_client.clone();
        let handlers = self.handlers.clone();

        let mut retry_count = 0;
        let max_retries = 3;
        let mut delay = Duration::from_millis(500); // start with a 500ms delay

        loop {
            // Attempt to connect
            let connection_result = pubsub_client
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

                            for handler in handlers.iter() {
                                if handler.key() == &discriminator {
                                    log::debug!("Found handler for event: {:?}", handler.key(),);
                                    let handler = handler.value();
                                    let event = E::try_from_slice(&decoded[8..]).unwrap();
                                    let handler = handler(&event);

                                    // TODO: send to channel to handle awaiting the future
                                    // tokio::spawn(handler).await.unwrap();
                                    let r = handler.await;
                                }
                            }
                        }
                    }

                    error!("[EVENT][WEBSOCKET] connection closed, attempting to reconnect...");
                }
                Err(e) => {
                    error!("[EVENT][WEBSOCKET] Failed to connect: {:?}", e);
                    if retry_count >= max_retries {
                        // TODO: graceful shutdown
                        panic!("[EVENT][WEBSOCKET] Maximum retry attempts reached, aborting...");
                    }

                    tokio::time::sleep(delay).await; // wait before retrying
                    retry_count += 1;
                    delay = std::cmp::min(delay * 2, Duration::from_secs(5));
                    // Double the delay for next retry, up to 5 seconds
                }
            }

            if retry_count >= max_retries {
                error!("[EVENT][WEBSOCKET] Maximum retry attempts reached, aborting...");
                break;
            }
        }
    }
}
