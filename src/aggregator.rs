use crate::config::{CombineMode, FlattenedConfig};
use crate::error::Error;
use crate::sources::{EntropySource, FileSource, LrngSource};
use futures::future::join_all;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::time::{interval, Duration};

pub struct Aggregator {
    #[allow(dead_code)]
    combine: CombineMode,
    sources: Vec<Arc<dyn EntropySource>>,
    bytes_served: Arc<AtomicU64>,
    requests_served: Arc<AtomicU64>,
}

impl Aggregator {
    pub async fn from_config(cfg: FlattenedConfig) -> Result<Self, Error> {
        let mut sources: Vec<Arc<dyn EntropySource>> = Vec::new();

        for lrng in cfg.lrng_sources.into_iter() {
            log::info!("Initializing LRNG source: {}", lrng.id);
            sources.push(Arc::new(LrngSource::new(lrng)));
        }

        for filecfg in cfg.file_sources.into_iter() {
            log::info!("Initializing file source: {} at {}", filecfg.id, filecfg.path);
            let src = FileSource::new(filecfg)
                .await
                .map_err(|e| {
                    log::error!("Failed to open file source: {}", e);
                    Error::OsError(e.raw_os_error().unwrap_or(0) as u32)
                })?;
            sources.push(Arc::new(src));
        }

        log::info!("Aggregator initialized with {} sources", sources.len());
        
        let bytes_served = Arc::new(AtomicU64::new(0));
        let requests_served = Arc::new(AtomicU64::new(0));
        
        // Start periodic logging
        let sources_clone = sources.clone();
        let bytes_served_clone = bytes_served.clone();
        let requests_served_clone = requests_served.clone();
        tokio::spawn(async move {
            Self::periodic_logging(sources_clone, bytes_served_clone, requests_served_clone).await;
        });
        
        Ok(Self { combine: cfg.combine, sources, bytes_served, requests_served })
    }

    pub async fn read_bytes(&self, num_bytes: usize, timeout_ms: u64) -> Result<Vec<u8>, Error> {
        if self.sources.is_empty() {
            log::error!("No enabled entropy sources found in config");
            return Err(Error::Unexpected);
        }
        let mut futures_vec = Vec::with_capacity(self.sources.len());
        for src in &self.sources {
            futures_vec.push(src.read_bytes(num_bytes, timeout_ms));
        }
        let results = join_all(futures_vec).await;

        let mut min_len = usize::MAX;
        let mut acc: Option<Vec<u8>> = None;
        let mut source_results = Vec::new();
        
        for (i, res) in results.into_iter().enumerate() {
            let buf = match res {
                Ok(result) => result,
                Err(e) => {
                    log::error!("Source {} failed: {:?}", i, e);
                    return Err(e);
                }
            };
            // Remove debug logging for performance
            min_len = min_len.min(buf.len());
            source_results.push((i, buf));
        }
        
        // XOR the common prefix
        for (_, buf) in &source_results {
            match &mut acc {
                None => acc = Some(buf.clone()),
                Some(existing) => {
                    let len = existing.len().min(buf.len());
                    for i in 0..len { existing[i] ^= buf[i]; }
                }
            }
        }
        
        if min_len == usize::MAX { min_len = 0; }
        let mut acc = acc.ok_or(Error::Unexpected)?;
        acc.truncate(min_len);
        
        // Return leftover bytes to sources that produced more than min_len
        for (i, buf) in source_results {
            if buf.len() > min_len {
                let leftover = buf[min_len..].to_vec();
                self.sources[i].return_leftover(leftover).await;
            }
        }
        
        // Update statistics
        self.requests_served.fetch_add(1, Ordering::Relaxed);
        self.bytes_served.fetch_add(acc.len() as u64, Ordering::Relaxed);
        
        Ok(acc)
    }
    
    async fn periodic_logging(sources: Vec<Arc<dyn EntropySource>>, bytes_served: Arc<AtomicU64>, requests_served: Arc<AtomicU64>) {
        let mut interval = interval(Duration::from_secs(10));
        loop {
            interval.tick().await;
            
            let total_bytes = bytes_served.load(Ordering::Relaxed);
            let total_requests = requests_served.load(Ordering::Relaxed);
            let total_mb = total_bytes as f64 / (1024.0 * 1024.0);
            
            log::info!("Statistics: {} requests served, {:.2} MB total", total_requests, total_mb);
            
            for source in &sources {
                let (id, buffer_status) = source.get_buffer_status().await;
                match buffer_status {
                    Some((current, max)) => {
                        let current_mb = current as f64 / (1024.0 * 1024.0);
                        let max_mb = max as f64 / (1024.0 * 1024.0);
                        let percentage = if max > 0 { (current as f64 / max as f64) * 100.0 } else { 0.0 };
                        log::info!("Source {}: buffer {:.2}/{:.2} MB ({:.1}%)", id, current_mb, max_mb, percentage);
                    }
                    None => {
                        log::info!("Source {}: no buffer", id);
                    }
                }
            }
        }
    }
}

