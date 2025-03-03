use crate::error::Error;
use std::io::{self, SeekFrom};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::select;
use tokio::time::{sleep_until, Instant};
pub struct MockRng {
    file: File,
    offset: u64,
}

impl MockRng {
    pub async fn new(path: &str) -> io::Result<Self> {
        Ok(Self {
            file: File::open(path).await?,
            offset: 0,
        })
    }

    pub async fn read_bytes(&mut self, num_bytes: usize) -> Result<Vec<u8>, Error> {
        let mut buffer = vec![0u8; num_bytes];
        let file = &mut self.file;
        let offset = &mut self.offset;

        // Seek to saved offset
        file.seek(SeekFrom::Start(*offset))
            .await
            .map_err(|e| Error::OsError(e.raw_os_error().unwrap_or(0) as u32))?;

        let mut bytes_read = 0;
        while bytes_read < num_bytes {
            match file.read(&mut buffer[bytes_read..]).await {
                Ok(0) => {
                    // Reached EOF, reset to beginning
                    file.seek(SeekFrom::Start(0))
                        .await
                        .map_err(|e| Error::OsError(e.raw_os_error().unwrap_or(0) as u32))?;
                    *offset = 0;
                }
                Ok(n) => {
                    bytes_read += n;
                    *offset += n as u64;
                }
                Err(e) => return Err(Error::OsError(e.raw_os_error().unwrap_or(0) as u32)),
            }
        }

        Ok(buffer)
    }

    pub async fn read_bytes_until(
        &mut self,
        num_bytes: usize,
        deadline: Instant,
    ) -> Result<Vec<u8>, Error> {
        let mut buffer = vec![0u8; num_bytes];
        let file = &mut self.file;
        let offset = &mut self.offset;

        // Seek to saved offset
        file.seek(SeekFrom::Start(*offset))
            .await
            .map_err(|e| Error::OsError(e.raw_os_error().unwrap_or(0) as u32))?;

        let mut bytes_read = 0;
        let timeout = sleep_until(deadline);
        tokio::pin!(timeout);

        while bytes_read < num_bytes {
            select! {
                res = file.read(&mut buffer[bytes_read..]) => {
                    match res {
                        Ok(0) => {
                            // Reached EOF, reset to beginning
                            file.seek(SeekFrom::Start(0))
                                .await
                                .map_err(|e| Error::OsError(e.raw_os_error().unwrap_or(0) as u32))?;
                            *offset = 0;
                        }
                        Ok(n) => {
                            bytes_read += n;
                            *offset += n as u64;
                        }
                        Err(e) => return Err(Error::OsError(e.raw_os_error().unwrap_or(0) as u32)),
                    }
                }
                _ = &mut timeout => {
                    break;
                }
            };
        }

        Ok(buffer)
    }
}
