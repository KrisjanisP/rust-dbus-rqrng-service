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
    async fn return_leftover(&self, leftover: Vec<u8>);
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

    async fn return_leftover(&self, _leftover: Vec<u8>) {
        // LRNG doesn't buffer, so we ignore leftover bytes
    }
}

pub struct FileSource {
    file: tokio::sync::Mutex<File>,
    offset: tokio::sync::Mutex<u64>,
    loop_on_eof: bool,
    buffer: tokio::sync::Mutex<Vec<u8>>,
}

impl FileSource {
    pub async fn new(cfg: FileConfig) -> io::Result<Self> {
        let file = File::open(&cfg.path).await?;
        Ok(Self {
            file: tokio::sync::Mutex::new(file),
            offset: tokio::sync::Mutex::new(0),
            loop_on_eof: cfg.loop_.unwrap_or(false),
            buffer: tokio::sync::Mutex::new(Vec::new()),
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
        let mut buffer = self.buffer.lock().await;
        
        // First, try to satisfy request from buffer
        if buffer.len() >= num_bytes {
            let result = buffer.drain(..num_bytes).collect();
            return Ok((num_bytes, result));
        }
        
        // Take what we have from buffer and read more
        let mut result = buffer.drain(..).collect::<Vec<u8>>();
        let bytes_from_buffer = result.len();
        let remaining = num_bytes - bytes_from_buffer;
        
        drop(buffer); // Release buffer lock while reading from file
        
        let mut file = self.file.lock().await;
        let mut offset = self.offset.lock().await;

        if timeout_ms == 0 {
            // Single attempt, return whatever is available immediately
            let mut buf = vec![0u8; remaining];
            file.seek(tokio::io::SeekFrom::Start(*offset))
                .await
                .map_err(|e| Error::OsError(e.raw_os_error().unwrap_or(0) as u32))?;
            let bytes_read = match file.read(&mut buf).await {
                Ok(0) if self.loop_on_eof => {
                    file.seek(tokio::io::SeekFrom::Start(0)).await
                        .map_err(|e| Error::OsError(e.raw_os_error().unwrap_or(0) as u32))?;
                    *offset = 0;
                    if let Ok(n) = file.read(&mut buf).await { 
                        *offset = n as u64; 
                        n 
                    } else { 0 }
                }
                Ok(n) => {
                    *offset += n as u64;
                    n
                }
                Err(e) => return Err(Error::OsError(e.raw_os_error().unwrap_or(0) as u32)),
            };
            buf.truncate(bytes_read);
            result.extend(buf);
            return Ok((result.len(), result));
        }

        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        let sleep = sleep_until(deadline);
        tokio::pin!(sleep);

        let mut buf = vec![0u8; remaining];
        let mut bytes_read = 0usize;
        loop {
            tokio::select! {
                res = Self::read_inner(&mut *file, &mut *offset, &mut buf[bytes_read..], self.loop_on_eof), if bytes_read < remaining => {
                    let n = res?;
                    bytes_read += n;
                    if bytes_read >= remaining || n == 0 { break; }
                }
                _ = &mut sleep => { break; }
            }
        }
        buf.truncate(bytes_read);
        result.extend(buf);
        Ok((result.len(), result))
    }

    async fn return_leftover(&self, leftover: Vec<u8>) {
        if !leftover.is_empty() {
            let mut buffer = self.buffer.lock().await;
            buffer.extend(leftover);
        }
    }
}

