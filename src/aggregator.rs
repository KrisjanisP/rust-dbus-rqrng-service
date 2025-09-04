use crate::config::{CombineMode, FlattenedConfig};
use crate::error::Error;
use crate::sources::{EntropySource, FileSource, LrngSource};
use futures::future::join_all;
use std::sync::Arc;

pub struct Aggregator {
    #[allow(dead_code)]
    combine: CombineMode,
    sources: Vec<Arc<dyn EntropySource>>,
}

impl Aggregator {
    pub async fn from_config(cfg: FlattenedConfig) -> Result<Self, Error> {
        let mut sources: Vec<Arc<dyn EntropySource>> = Vec::new();

        for lrng in cfg.lrng_sources.into_iter() {
            sources.push(Arc::new(LrngSource::new(lrng)));
        }

        for filecfg in cfg.file_sources.into_iter() {
            let src = FileSource::new(filecfg)
                .await
                .map_err(|e| Error::OsError(e.raw_os_error().unwrap_or(0) as u32))?;
            sources.push(Arc::new(src));
        }

        Ok(Self { combine: cfg.combine, sources })
    }

    pub async fn read_bytes(&self, num_bytes: usize, timeout_ms: u64) -> Result<(usize, Vec<u8>), Error> {
        if self.sources.is_empty() {
            return Err(Error::Unexpected);
        }
        let mut futures_vec = Vec::with_capacity(self.sources.len());
        for src in &self.sources {
            futures_vec.push(src.read_bytes(num_bytes, timeout_ms));
        }
        let results = join_all(futures_vec).await;

        let mut min_n = usize::MAX;
        let mut acc: Option<Vec<u8>> = None;
        let mut source_results = Vec::new();
        
        for (i, res) in results.into_iter().enumerate() {
            let (n, buf) = res?;
            min_n = min_n.min(n);
            source_results.push((i, n, buf));
        }
        
        // XOR the common prefix
        for (_, n, buf) in &source_results {
            match &mut acc {
                None => acc = Some(buf.clone()),
                Some(existing) => {
                    let len = existing.len().min(buf.len());
                    for i in 0..len { existing[i] ^= buf[i]; }
                }
            }
        }
        
        if min_n == usize::MAX { min_n = 0; }
        let mut acc = acc.ok_or(Error::Unexpected)?;
        acc.truncate(min_n);
        
        // Return leftover bytes to sources that produced more than min_n
        for (i, n, buf) in source_results {
            if n > min_n && min_n < buf.len() {
                let leftover = buf[min_n..n].to_vec();
                self.sources[i].return_leftover(leftover).await;
            }
        }
        
        Ok((min_n, acc))
    }
}

