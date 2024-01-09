use crate::*;

fn default_cluster() -> String {
    "devnet".to_string() // @dev UPDATE THIS IN PROD TO mainnet-beta
}

// @dev WHEN YOU UPDATE THIS, MAKE SURE TO UPDATE THE GRAMINE MANIFEST
#[derive(Deserialize, Debug, Default, PartialEq, Clone)]
pub struct SolanaServiceEnvironment {
    #[serde(default = "default_cluster")]
    pub cluster: String,

    pub service_key: String,
    #[serde(default)]
    pub service_authority_key: String,

    #[serde(default)]
    pub function_key: String,
    #[serde(default)]
    pub function_authority_key: String,

    // @dev We dont use these actually and pull from std::env::var when posting to IPFS. This is here just for verification early on in the startup process.
    // Required to post a quote for verification
    pub ipfs_url: String,
    pub ipfs_username: String,
    pub ipfs_password: String,
}
impl SolanaServiceEnvironment {
    pub fn parse() -> Result<Self, SbError> {
        match envy::from_env::<SolanaServiceEnvironment>() {
            Ok(env) => Ok(env),
            Err(error) => match &error {
                envy::Error::MissingValue(msg) => {
                    println!(
                        ">>>> SERVICE_KEY='{}'  <<<<",
                        std::env::var("SERVICE_KEY").unwrap_or("NOT_SET".to_string())
                    );

                    Err(SbError::EnvVariableMissing(msg.to_string()))
                }
                envy::Error::Custom(msg) => Err(SbError::CustomMessage(format!(
                    "failed to decode environment variables: {}",
                    msg
                ))),
            },
        }
    }

    pub fn default_rpc_url(&self) -> String {
        let cluster: Cluster = match self.cluster.as_str() {
            "devnet" => Cluster::Devnet,
            "mainnet-beta" | "mainnet" => Cluster::Mainnet,
            c => c.parse().unwrap_or_default(),
        };

        match cluster {
            Cluster::Devnet => "https://api.devnet.solana.com".to_string(),
            Cluster::Mainnet => "https://api.mainnet-beta.solana.com".to_string(),
            Cluster::Localnet => "http:://0.0.0.0:8899".to_string(),
            Cluster::Custom(rpc_url, _) => rpc_url.to_string(),
            _ => panic!("Failed to get default RPC_URL"),
        }
    }
}
