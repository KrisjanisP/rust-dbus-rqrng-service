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

fn get_config_path() -> String {
    if let Ok(home) = std::env::var("HOME") {
        format!("{}/.config/trng-dbus/config.toml", home)
    } else {
        "/etc/trng-dbus/config.toml".to_string()
    }
}

struct SourceXorAggregator(Aggregator);

impl SourceXorAggregator {
    fn new(aggregator: Aggregator) -> Self {
        Self(aggregator)
    }
}

#[interface(name = "lv.lumii.trng.Rng")]
impl SourceXorAggregator {
    /// ReadBytes returns up to `num_bytes` of data within `timeout_ms`.
    /// Returns bytes vector (length indicates actual bytes produced).
    async fn read_bytes(&mut self, num_bytes: u64, timeout_ms: u64) -> Vec<u8> {
        match self.0.read_bytes(num_bytes as usize, timeout_ms).await {
            Ok(bytes) => bytes,
            Err(e) => {
                error!("Error reading random bytes: {:?}", e);
                Vec::new()
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Initialize logging
    env_logger::init();

    let config_path = get_config_path();
    let cfg = load_config(&config_path)
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
