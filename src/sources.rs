use crate::config::{FileConfig, LrngConfig};
use crate::error::Error;
use crate::lrng::os_fill_rand_octets;
use crate::circular_buffer::CircularBuffer;
use async_trait::async_trait;
use std::io;
use std::sync::Arc;
use std::time::Duration;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::time::{sleep_until, Instant, interval};

#[async_trait]
pub trait EntropySource: Send + Sync {
    async fn read_bytes(&self, num_bytes: usize, timeout_ms: u64) -> Result<Vec<u8>, Error>;
    async fn return_leftover(&self, leftover: Vec<u8>);
    async fn get_buffer_status(&self) -> (String, Option<(usize, usize)>); // (id, Some(current_size, max_size)) or None
}

pub struct LrngSource {
    cfg: LrngConfig,
    buffer: Arc<tokio::sync::Mutex<CircularBuffer>>,
    max_buffer_size: Option<usize>,
}

impl LrngSource {
    pub fn new(cfg: LrngConfig) -> Self {
        let max_buffer_size = cfg.buffer_mebibytes.map(|mb| mb as usize * 1024 * 1024);
        let buffer = Arc::new(tokio::sync::Mutex::new(
            CircularBuffer::new(max_buffer_size.unwrap_or(1024))
        ));
        
        // Start background replenishing if buffer is configured
        if let Some(max_size) = max_buffer_size {
            let buffer_clone = buffer.clone();
            let id = cfg.id.clone();
            tokio::spawn(async move {
                Self::background_replenish(buffer_clone, max_size, id).await;
            });
        }
        
        Self { 
            cfg,
            buffer,
            max_buffer_size,
        }
    }
    
    async fn background_replenish(buffer: Arc<tokio::sync::Mutex<CircularBuffer>>, max_size: usize, id: String) {
        let mut interval = interval(Duration::from_millis(10)); // Check more frequently
        loop {
            interval.tick().await;
            let current_size = {
                let buf = buffer.lock().await;
                buf.len()
            };
            
            while current_size < max_size {
                let needed = max_size - current_size;
                // Generate in chunks to avoid blocking too long
                let chunk_size = (needed).min(64 * 1024); // 64KB chunks
                if let Ok(new_bytes) = tokio::task::spawn_blocking(move || os_fill_rand_octets(chunk_size)).await {
                    if let Ok(bytes) = new_bytes {
                        let mut buf = buffer.lock().await;
                        buf.extend_from_vec(bytes);
                        log::debug!("LRNG {} replenished buffer: {} -> {} bytes", id, current_size, buf.len());
                    }
                }
            }
        }
    }
}

#[async_trait]
impl EntropySource for LrngSource {
    async fn read_bytes(&self, num_bytes: usize, timeout_ms: u64) -> Result<Vec<u8>, Error> {
        // Fast path: try to satisfy from buffer first
        {
            let mut buffer = self.buffer.lock().await;
            if buffer.len() >= num_bytes {
                let result = buffer.take(num_bytes);
                return Ok(result);
            }
        }
        
        // For timeout 0, return only what's in buffer (don't generate)
        if timeout_ms == 0 {
            let mut buffer = self.buffer.lock().await;
            let result = buffer.take(num_bytes);
            return Ok(result);
        }
        
        // For non-zero timeout, use buffer
        let mut result = {
            let mut buffer = self.buffer.lock().await;
            let buf_len = buffer.len();
            buffer.take(buf_len) // Take everything from buffer
        };
        let remaining = num_bytes.saturating_sub(result.len());
        
        if remaining > 0 {
            // Buffer already dropped from scope
            let deadline = Instant::now() + Duration::from_millis(timeout_ms);
            let sleep = sleep_until(deadline);
            tokio::pin!(sleep);
            let task = tokio::task::spawn_blocking(move || os_fill_rand_octets(remaining));
            tokio::select! {
                res = task => {
                    let bytes = res.map_err(|_| Error::Unexpected)??;
                    result.extend(bytes);
                }
                _ = &mut sleep => {
                    // Timeout reached, return what we have
                }
            }
        }
        
        Ok(result)
    }

    async fn return_leftover(&self, leftover: Vec<u8>) {
        if !leftover.is_empty() {
            let leftover_len = leftover.len();
            let mut buffer = self.buffer.lock().await;
            buffer.extend_from_vec(leftover);
            log::debug!("LRNG returned {} leftover bytes to buffer (total: {})", leftover_len, buffer.len());
        }
    }
    
    async fn get_buffer_status(&self) -> (String, Option<(usize, usize)>) {
        let id = self.cfg.id.clone();
        if let Some(max_size) = self.max_buffer_size {
            let current_size = self.buffer.lock().await.len();
            (id, Some((current_size, max_size)))
        } else {
            (id, None)
        }
    }
}

pub struct FileSource {
    cfg: FileConfig,
    file: tokio::sync::Mutex<File>,
    offset: tokio::sync::Mutex<u64>,
    loop_on_eof: bool,
    buffer: Arc<tokio::sync::Mutex<CircularBuffer>>,
    max_buffer_size: Option<usize>,
}

impl FileSource {
    pub async fn new(cfg: FileConfig) -> io::Result<Self> {
        let file = File::open(&cfg.path).await?;
        let max_buffer_size = cfg.buffer_mebibytes.map(|mb| mb as usize * 1024 * 1024);
        let buffer = Arc::new(tokio::sync::Mutex::new(
            CircularBuffer::new(max_buffer_size.unwrap_or(1024))
        ));
        
        // Start background replenishing if buffer is configured
        if let Some(max_size) = max_buffer_size {
            let buffer_clone = buffer.clone();
            let path = cfg.path.clone();
            let id = cfg.id.clone();
            let loop_on_eof = cfg.loop_.unwrap_or(false);
            tokio::spawn(async move {
                Self::background_replenish(buffer_clone, max_size, path, id, loop_on_eof).await;
            });
        }
        
        Ok(Self {
            cfg: cfg.clone(),
            file: tokio::sync::Mutex::new(file),
            offset: tokio::sync::Mutex::new(0),
            loop_on_eof: cfg.loop_.unwrap_or(false),
            buffer,
            max_buffer_size,
        })
    }
    
    async fn background_replenish(buffer: Arc<tokio::sync::Mutex<CircularBuffer>>, max_size: usize, path: String, id: String, loop_on_eof: bool) {
        let mut interval = interval(Duration::from_secs(1));
        let mut file = match File::open(&path).await {
            Ok(f) => f,
            Err(_) => return,
        };
        let mut offset = 0u64;
        
        loop {
            interval.tick().await;
            let current_size = buffer.lock().await.len();
            if current_size < max_size / 2 { // Replenish when below 50%
                let needed = max_size - current_size;
                let mut buf = vec![0u8; needed];
                let mut bytes_read = 0;
                
                while bytes_read < needed {
                    file.seek(tokio::io::SeekFrom::Start(offset)).await.ok();
                    match file.read(&mut buf[bytes_read..]).await {
                        Ok(0) if loop_on_eof => {
                            file.seek(tokio::io::SeekFrom::Start(0)).await.ok();
                            offset = 0;
                        }
                        Ok(0) => break,
                        Ok(n) => {
                            offset += n as u64;
                            bytes_read += n;
                        }
                        Err(_) => break,
                    }
                }
                
                if bytes_read > 0 {
                    buf.truncate(bytes_read);
                    let mut buffer_guard = buffer.lock().await;
                    buffer_guard.extend_from_vec(buf);
                    log::debug!("File {} replenished buffer: {} -> {} bytes", id, current_size, buffer_guard.len());
                }
            }
        }
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
    async fn read_bytes(&self, num_bytes: usize, timeout_ms: u64) -> Result<Vec<u8>, Error> {
        let mut buffer = self.buffer.lock().await;
        
        // First, try to satisfy request from buffer
        if buffer.len() >= num_bytes {
            let result = buffer.take(num_bytes);
            return Ok(result);
        }
        
        // For timeout 0, return only what's in buffer (don't read file)
        if timeout_ms == 0 {
            let result = buffer.take(num_bytes);
            return Ok(result);
        }
        
        // Take what we have from buffer and read more
        let mut result = {
            let buf_len = buffer.len();
            buffer.take(buf_len)
        };
        let bytes_from_buffer = result.len();
        let remaining = num_bytes - bytes_from_buffer;
        
        drop(buffer); // Release buffer lock while reading from file
        
        let mut file = self.file.lock().await;
        let mut offset = self.offset.lock().await;

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
        Ok(result)
    }

    async fn return_leftover(&self, leftover: Vec<u8>) {
        if !leftover.is_empty() {
            let mut buffer = self.buffer.lock().await;
            buffer.extend_from_vec(leftover);
        }
    }
    
    async fn get_buffer_status(&self) -> (String, Option<(usize, usize)>) {
        let id = self.cfg.id.clone();
        if let Some(max_size) = self.max_buffer_size {
            let current_size = self.buffer.lock().await.len();
            (id, Some((current_size, max_size)))
        } else {
            (id, None)
        }
    }
}

