mod error;
mod lrng;
mod mock_rng;
mod two_source_xor;

use mock_rng::MockRng;
use std::{error::Error, future::pending, time::Duration};
use tokio::{sync::Mutex, time::Instant};
use two_source_xor::TwoSourceRng;
use zbus::{connection, interface};
// use lrng::os_fill_rand_octets;
use log::{debug, error, info};

struct RemoteQrngXorLinuxRng {
    mock_rng: Mutex<MockRng>,
    two_source_rng: Mutex<TwoSourceRng>,
}

impl RemoteQrngXorLinuxRng {
    async fn new() -> Self {
        Self {
            mock_rng: Mutex::new(
                MockRng::new("/dev/urandom")
                    .await
                    .expect("Failed to initialize mock RNG"),
            ),
            two_source_rng: Mutex::new(
                TwoSourceRng::new("/dev/random", "/dev/urandom")
                    .await
                    .expect("Failed to initialize two source rng"),
            ),
        }
    }
}

static MAX_BYTES: usize = 1024; // Maximum bytes to serve

#[interface(name = "lv.lumii.qrng.Rng")]
impl RemoteQrngXorLinuxRng {
    /// Generates random octets using two sources of RNG.
    ///
    /// # Arguments
    ///
    /// * `num_bytes` - The number of random octets to generate.
    ///
    /// # Returns
    ///
    /// A tuple containing:
    /// - `u32`: Status code (`0` for success, non-zero for errors).
    /// - `(usize, Vec<u8>)`: A tuple containing the number of generated octets and the generated random octets.
    async fn generate_octets(&mut self, num_bytes: usize) -> (u32, Vec<u8>) {
        if num_bytes > MAX_BYTES {
            error!(
                "Requested number of octets ({}) exceeds the maximum allowed ({})",
                num_bytes, MAX_BYTES
            );
            return (4, Vec::new()); // Status code `4` for invalid input
        }

        match self.two_source_rng.lock().await.read_bytes(num_bytes).await {
            Ok(octets) => {
                debug!("Generated {} octets successfully.", num_bytes);
                (0, octets)
            }
            Err(e) => {
                error!("Error reading from mock RNG: {:?}", e);
                (1, Vec::new())
            }
        }
    }

    /// Generates random octets using two sources RNG.
    /// Returns when requested number of bytes are generated or when the timeout is reached.
    ///
    /// # Arguments
    ///
    /// * `num_bytes` - The number of random octets to generate.
    /// * `timeout` - The timeout in miliseconds from when the function is called.
    ///
    /// # Returns
    ///
    /// A tuple containing:
    /// - `u32`: Status code (`0` for success, non-zero for errors).
    /// - `Vec<u8>`: The generated random octets.
    async fn generate_octets_timeout(
        &mut self,
        num_bytes: usize,
        timeout: u64,
    ) -> (u32, (usize, Vec<u8>)) {
        let method_invoked = Instant::now();

        if num_bytes > MAX_BYTES {
            error!(
                "Requested number of octets ({}) exceeds the maximum allowed ({})",
                num_bytes, MAX_BYTES
            );
            return (4, (0, Vec::new())); // Status code `4` for invalid input
        }

        let deadline = Instant::now() + Duration::from_millis(timeout);

        match self
            .two_source_rng
            .lock()
            .await
            .read_bytes_until(num_bytes, deadline)
            .await
        {
            Ok(octets) => {
                debug!("Generated {} octets successfully.", num_bytes);
                debug!(
                    "Generation took {} miliseconds",
                    (Instant::now() - method_invoked).as_millis()
                );
                (0, octets)
            }
            Err(e) => {
                error!("Error reading from mock RNG: {:?}", e);
                (1, (0, Vec::new()))
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Initialize logging
    env_logger::init();

    let rng_service = RemoteQrngXorLinuxRng::new().await;
    let _connection = connection::Builder::session()?
        .name("lv.lumii.qrng")?
        .serve_at("/lv/lumii/qrng/RemoteQrngXorLinuxRng", rng_service)?
        .build()
        .await?;

    info!("D-Bus service 'lv.lumii.qrng' is running.");

    // Keep the application running indefinitely
    pending::<()>().await;

    Ok(())
}
