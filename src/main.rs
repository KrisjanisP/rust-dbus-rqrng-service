mod error;
mod lrng;
mod mock_rng;

use std::{error::Error, future::pending, sync::Mutex};
use mock_rng::MockRng;
use zbus::{connection, interface};
// use lrng::os_fill_rand_octets;
use log::{error, info, debug};
use lazy_static::lazy_static;

lazy_static! {
    static ref MOCK_RNG: Mutex<MockRng> = Mutex::new(
        MockRng::new("./data/mock-dev-random.bin")
            .expect("Failed to initialize mock RNG")
    );
}

struct RemoteQrngXorLinuxRng {
    count: u64,
}

#[interface(name = "lv.lumii.qrng.Rng")]
impl RemoteQrngXorLinuxRng {
    /// Generates random octets using the Linux RNG subsystem.
    ///
    /// # Arguments
    ///
    /// * `num_octets` - The number of random octets to generate.
    ///
    /// # Returns
    ///
    /// A tuple containing:
    /// - `u32`: Status code (`0` for success, non-zero for errors).
    /// - `Vec<u8>`: The generated random octets.
    fn generate_octets(&mut self, num_octets: usize) -> (u32, Vec<u8>) {
        const MAX_OCTETS: usize = 1024; // Define a reasonable maximum

        if num_octets > MAX_OCTETS {
            error!(
                "Requested number of octets ({}) exceeds the maximum allowed ({})",
                num_octets, MAX_OCTETS
            );
            return (4, Vec::new()); // Status code `4` for invalid input
        }

        match MOCK_RNG.lock() {
            Ok(mut rng) => {
                match rng.read_bytes(num_octets) {
                    Ok(octets) => {
                        debug!("Generated {} octets successfully.", num_octets);
                        (0, octets)
                    },
                    Err(e) => {
                        error!("Error reading from mock RNG: {:?}", e);
                        (1, Vec::new())
                    }
                }
            },
            Err(e) => {
                error!("Failed to acquire mock RNG lock: {:?}", e);
                (2, Vec::new())
            }
        }

        // self.count += 1;
        // match os_fill_rand_octets(num_octets) {
        //     Ok(octets) => {
        //         debug!("Generated {} octets successfully.", num_octets);
        //         (0, octets)
        //     }
        //     Err(e) => {
        //         error!("Error generating random octets: {:?}", e);
        //         (e.to_status_code(), Vec::new())
        //     }
        // }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Initialize logging
    env_logger::init();

    let rng_service = RemoteQrngXorLinuxRng { count: 0 };
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
