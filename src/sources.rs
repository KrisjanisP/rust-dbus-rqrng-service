use crate::config::{FileConfig, LrngConfig};
use crate::error::Error;
use crate::lrng::os_fill_rand_octets;
use async_trait::async_trait;
use std::io;
use std::time::Duration;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::time::{sleep_until, Instant};

#[async_trait]
pub trait EntropySource: Send + Sync {
    async fn read_bytes(&self, num_bytes: usize, timeout_ms: u64) -> Result<(usize, Vec<u8>), Error>;
}

pub struct LrngSource {
    #[allow(dead_code)]
    cfg: LrngConfig,
}

impl LrngSource {
    pub fn new(cfg: LrngConfig) -> Self {
        Self { cfg }
    }
}

#[async_trait]
impl EntropySource for LrngSource {
    async fn read_bytes(&self, num_bytes: usize, timeout_ms: u64) -> Result<(usize, Vec<u8>), Error> {
        if timeout_ms == 0 {
            let bytes = tokio::task::spawn_blocking(move || os_fill_rand_octets(num_bytes))
                .await
                .map_err(|_| Error::Unexpected)??;
            return Ok((bytes.len(), bytes));
        }
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        let sleep = sleep_until(deadline);
        tokio::pin!(sleep);
        let task = tokio::task::spawn_blocking(move || os_fill_rand_octets(num_bytes));
        tokio::select! {
            res = task => {
                let bytes = res.map_err(|_| Error::Unexpected)??;
                Ok((bytes.len(), bytes))
            }
            _ = &mut sleep => {
                Ok((0, vec![0u8; num_bytes]))
            }
        }
    }
}

pub struct FileSource {
    file: tokio::sync::Mutex<File>,
    offset: tokio::sync::Mutex<u64>,
    loop_on_eof: bool,
}

impl FileSource {
    pub async fn new(cfg: FileConfig) -> io::Result<Self> {
        let file = File::open(&cfg.path).await?;
        Ok(Self {
            file: tokio::sync::Mutex::new(file),
            offset: tokio::sync::Mutex::new(0),
            loop_on_eof: cfg.loop_.unwrap_or(false),
        })
    }

    async fn read_inner(file: &mut File, offset: &mut u64, buf: &mut [u8], loop_on_eof: bool) -> Result<usize, Error> {
        // Seek to saved offset
        file.seek(tokio::io::SeekFrom::Start(*offset))
            .await
            .map_err(|e| Error::OsError(e.raw_os_error().unwrap_or(0) as u32))?;

        let mut bytes_read = 0usize;
        while bytes_read < buf.len() {
            match file.read(&mut buf[bytes_read..]).await {
                Ok(0) if loop_on_eof => {
                    file.seek(tokio::io::SeekFrom::Start(0))
                        .await
                        .map_err(|e| Error::OsError(e.raw_os_error().unwrap_or(0) as u32))?;
                    *offset = 0;
                }
                Ok(0) => break, // EOF without loop
                Ok(n) => {
                    *offset += n as u64;
                    bytes_read += n;
                }
                Err(e) => return Err(Error::OsError(e.raw_os_error().unwrap_or(0) as u32)),
            }
        }
        Ok(bytes_read)
    }
}

#[async_trait]
impl EntropySource for FileSource {
    async fn read_bytes(&self, num_bytes: usize, timeout_ms: u64) -> Result<(usize, Vec<u8>), Error> {
        let mut buf = vec![0u8; num_bytes];
        let mut file = self.file.lock().await;
        let mut offset = self.offset.lock().await;

        if timeout_ms == 0 {
            // Single attempt, return whatever is available immediately
            // Seek to saved offset
            file.seek(tokio::io::SeekFrom::Start(*offset))
                .await
                .map_err(|e| Error::OsError(e.raw_os_error().unwrap_or(0) as u32))?;
            let mut bytes_read = match file.read(&mut buf).await {
                Ok(0) if self.loop_on_eof => {
                    file.seek(tokio::io::SeekFrom::Start(0)).await
                        .map_err(|e| Error::OsError(e.raw_os_error().unwrap_or(0) as u32))?;
                    0
                }
                Ok(n) => {
                    *offset += n as u64;
                    n
                }
                Err(e) => return Err(Error::OsError(e.raw_os_error().unwrap_or(0) as u32)),
            };
            // If we looped and want one more quick read after EOF reset
            if bytes_read == 0 && self.loop_on_eof && num_bytes > 0 {
                if let Ok(n) = file.read(&mut buf).await { bytes_read = n; *offset = n as u64; }
            }
            return Ok((bytes_read, buf));
        }

        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        let sleep = sleep_until(deadline);
        tokio::pin!(sleep);

        let mut bytes_read = 0usize;
        loop {
            tokio::select! {
                res = Self::read_inner(&mut *file, &mut *offset, &mut buf[bytes_read..], self.loop_on_eof), if bytes_read < num_bytes => {
                    let n = res?;
                    bytes_read += n;
                    if bytes_read >= num_bytes || n == 0 { break; }
                }
                _ = &mut sleep => { break; }
            }
        }
        Ok((bytes_read, buf))
    }
}

