use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::Mutex;
use crate::error::Error;

pub struct MockRng {
    file: Mutex<File>,
    offset: Mutex<u64>,
}

impl MockRng {
    pub fn new(path: &str) -> io::Result<Self> {
        Ok(Self {
            file: Mutex::new(File::open(path)?),
            offset: Mutex::new(0),
        })
    }

    pub fn read_bytes(&self, num_bytes: usize) -> Result<Vec<u8>, Error> {
        let mut buffer = vec![0u8; num_bytes];
        let mut file = self.file.lock().map_err(|_| Error::Unexpected)?;
        let mut offset = self.offset.lock().map_err(|_| Error::Unexpected)?;
        
        // Seek to saved offset
        file.seek(SeekFrom::Start(*offset))
            .map_err(|e| Error::OsError(e.raw_os_error().unwrap_or(0) as u32))?;
        
        let mut bytes_read = 0;
        while bytes_read < num_bytes {
            match file.read(&mut buffer[bytes_read..]) {
                Ok(0) => {
                    // Reached EOF, reset to beginning
                    file.seek(SeekFrom::Start(0))
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
} 