use crate::error::Error;
use std::io::{self, SeekFrom};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::select;
use tokio::time::{sleep_until, Instant};
pub struct TwoSourceRng {
    file1: FileRead,
    file2: FileRead,
}

struct FileRead {
    file: File,
    offset: u64,
}

impl FileRead {
    async fn open(path: impl AsRef<std::path::Path>) -> io::Result<Self> {
        Ok(Self {
            file: File::open(path).await?,
            offset: 0,
        })
    }

    async fn read_bytes(&mut self, num_bytes: usize) -> Result<Vec<u8>, Error> {
        let mut buf = vec![0u8; num_bytes];
        let file = &mut self.file;
        let offset = &mut self.offset;

        // Seek to saved offset
        file.seek(SeekFrom::Start(*offset))
            .await
            .map_err(|e| Error::OsError(e.raw_os_error().unwrap_or(0) as u32))?;

        let mut bytes_read = 0;
        while bytes_read < num_bytes {
            match file.read(&mut buf[bytes_read..]).await {
                Ok(0) => {
                    // Reached EOF, reset to beginning
                    file.seek(SeekFrom::Start(0))
                        .await
                        .map_err(|e| Error::OsError(e.raw_os_error().unwrap_or(0) as u32))?;
                    *offset = 0;
                }
                Ok(n) => {
                    *offset += n as u64;
                    bytes_read += n;
                }
                Err(e) => Err(Error::OsError(e.raw_os_error().unwrap_or(0) as u32))?,
            }
        }
        Ok(buf)
    }

    pub async fn read_bytes_until(
        &mut self,
        num_bytes: usize,
        deadline: Instant,
    ) -> Result<(usize, Vec<u8>), Error> {
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

        Ok((bytes_read, buffer))
    }
}

impl TwoSourceRng {
    pub async fn new(path1: &str, path2: &str) -> io::Result<Self> {
        Ok(Self {
            file1: FileRead::open(path1).await?,
            file2: FileRead::open(path2).await?,
        })
    }

    pub async fn read_bytes(&mut self, num_bytes: usize) -> Result<Vec<u8>, Error> {
        let future1 = self.file1.read_bytes(num_bytes);
        let future2 = self.file2.read_bytes(num_bytes);

        let (res1, res2) = tokio::join!(future1, future2);
        let (mut buf1, buf2) = (res1?, res2?);

        // XOR the read bytes, this should auto-vectorize.
        buf1.iter_mut().zip(buf2).for_each(|(a, b)| *a ^= b);

        Ok(buf1)
    }

    pub async fn read_bytes_until(
        &mut self,
        num_bytes: usize,
        deadline: Instant,
    ) -> Result<(usize, Vec<u8>), Error> {
        let future1 = self.file1.read_bytes_until(num_bytes, deadline);
        let future2 = self.file2.read_bytes_until(num_bytes, deadline);

        let (res1, res2) = tokio::join!(future1, future2);
        let ((n1, mut buf1), (n2, buf2)) = (res1?, res2?);

        let n = n1.min(n2);
        let (slice1, slice2) = (&mut buf1[..n], &buf2[..n]);
        slice1.iter_mut().zip(slice2).for_each(|(a, b)| *a ^= b);

        Ok((n, buf1))
    }
}
