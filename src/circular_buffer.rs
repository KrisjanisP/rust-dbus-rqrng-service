/// High-performance circular buffer for bytes
pub struct CircularBuffer {
    buffer: Vec<u8>,
    read_pos: usize,
    write_pos: usize,
    len: usize,
    capacity: usize,
}

impl CircularBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: vec![0; capacity],
            read_pos: 0,
            write_pos: 0,
            len: 0,
            capacity,
        }
    }
    
    pub fn len(&self) -> usize {
        self.len
    }
    
    pub fn available_space(&self) -> usize {
        self.capacity - self.len
    }
    
    /// Take up to `count` bytes from the buffer
    pub fn take(&mut self, count: usize) -> Vec<u8> {
        let to_take = count.min(self.len);
        let mut result = Vec::with_capacity(to_take);
        
        if to_take == 0 {
            return result;
        }
        
        // Handle wrap-around case
        if self.read_pos + to_take <= self.capacity {
            // Simple case: no wrap-around
            result.extend_from_slice(&self.buffer[self.read_pos..self.read_pos + to_take]);
        } else {
            // Wrap-around case: take from end, then from beginning
            let first_chunk = self.capacity - self.read_pos;
            let second_chunk = to_take - first_chunk;
            
            result.extend_from_slice(&self.buffer[self.read_pos..]);
            result.extend_from_slice(&self.buffer[..second_chunk]);
        }
        
        self.read_pos = (self.read_pos + to_take) % self.capacity;
        self.len -= to_take;
        
        result
    }
    
    /// Add bytes to the buffer
    pub fn extend(&mut self, data: &[u8]) {
        let to_add = data.len().min(self.available_space());
        
        if to_add == 0 {
            return;
        }
        
        // Handle wrap-around case
        if self.write_pos + to_add <= self.capacity {
            // Simple case: no wrap-around
            self.buffer[self.write_pos..self.write_pos + to_add].copy_from_slice(&data[..to_add]);
        } else {
            // Wrap-around case: write to end, then to beginning
            let first_chunk = self.capacity - self.write_pos;
            let second_chunk = to_add - first_chunk;
            
            self.buffer[self.write_pos..].copy_from_slice(&data[..first_chunk]);
            self.buffer[..second_chunk].copy_from_slice(&data[first_chunk..to_add]);
        }
        
        self.write_pos = (self.write_pos + to_add) % self.capacity;
        self.len += to_add;
    }
    
    /// Add bytes from a Vec (more efficient than extend for Vec<u8>)
    pub fn extend_from_vec(&mut self, mut data: Vec<u8>) {
        let to_add = data.len().min(self.available_space());
        
        if to_add == 0 {
            return;
        }
        
        // Truncate if we can't fit everything
        if to_add < data.len() {
            data.truncate(to_add);
        }
        
        self.extend(&data);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_basic_operations() {
        let mut buf = CircularBuffer::new(10);
        assert_eq!(buf.len(), 0);
        assert_eq!(buf.available_space(), 10);
        
        buf.extend(b"hello");
        assert_eq!(buf.len(), 5);
        assert_eq!(buf.available_space(), 5);
        
        let data = buf.take(3);
        assert_eq!(data, b"hel");
        assert_eq!(buf.len(), 2);
        
        let data = buf.take(5);
        assert_eq!(data, b"lo");
        assert_eq!(buf.len(), 0);
    }
    
    #[test]
    fn test_wraparound() {
        let mut buf = CircularBuffer::new(5);
        
        // Fill buffer
        buf.extend(b"12345");
        assert_eq!(buf.len(), 5);
        
        // Take some
        let data = buf.take(2);
        assert_eq!(data, b"12");
        assert_eq!(buf.len(), 3);
        
        // Add more (should wrap around)
        buf.extend(b"ab");
        assert_eq!(buf.len(), 5);
        
        // Take all
        let data = buf.take(10);
        assert_eq!(data, b"345ab");
        assert_eq!(buf.len(), 0);
    }
}