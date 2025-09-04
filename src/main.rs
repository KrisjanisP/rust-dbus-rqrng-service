mod error;
mod lrng;
mod config;
mod sources;
mod aggregator;

use std::{error::Error, future::pending};
use zbus::{connection, interface};
// use lrng::os_fill_rand_octets;
use log::{error, info};
use aggregator::Aggregator;
use config::load_config;

const DEFAULT_CONFIG_PATH: &str = "/etc/trng-dbus/config.toml";

struct SourceXorAggregator(Aggregator);

impl SourceXorAggregator {
    fn new(aggregator: Aggregator) -> Self {
        Self(aggregator)
    }
}

#[interface(name = "lv.lumii.trng.Rng")]
impl SourceXorAggregator {
    /// ReadBytes returns up to `num_bytes` of data within `timeout_ms`.
    /// Returns (n, bytes) where n <= len(bytes) <= num_bytes.
    async fn read_bytes(&mut self, num_bytes: u64, timeout_ms: u64) -> (u64, Vec<u8>) {
        match self.0.read_bytes(num_bytes as usize, timeout_ms).await {
            Ok((n, mut bytes)) => {
                bytes.truncate(n);
                (n as u64, bytes)
            }
            Err(e) => {
                error!("Error reading random bytes: {:?}", e);
                (0, Vec::new())
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Initialize logging
    env_logger::init();

    let cfg = load_config(DEFAULT_CONFIG_PATH)
        .expect("Failed to load config");
    let aggregator = Aggregator::from_config(cfg)
        .await
        .expect("Failed to initialize aggregator from config");
    let rng_service = SourceXorAggregator::new(aggregator);
    let _connection = connection::Builder::session()?
        .name("lv.lumii.trng")?
        .serve_at("/lv/lumii/trng/SourceXorAggregator", rng_service)?
        .build()
        .await?;

    info!("D-Bus service 'lv.lumii.trng' is running.");

    // Keep the application running indefinitely
    pending::<()>().await;

    Ok(())
}
