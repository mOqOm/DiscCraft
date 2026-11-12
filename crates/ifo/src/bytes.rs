use crate::IfoError;

/// DVD sector size in bytes.
pub const SECTOR: usize = 2048;

/// Convert a `usize` to `u32` with a crate error on overflow.
pub(crate) fn u32_from_usize(v: usize) -> Result<u32, IfoError> {
    u32::try_from(v).map_err(|_| IfoError("value too large for u32".to_string()))
}

/// Big-endian byte writer that grows and zero-fills as needed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    /// Create an empty writer.
    pub(crate) fn new() -> Self {
        Self { buf: Vec::new() }
    }

    /// Current buffer length.
    pub(crate) fn len(&self) -> usize {
        self.buf.len()
    }

    /// Consume the writer and return the bytes.
    pub(crate) fn into_bytes(self) -> Vec<u8> {
        self.buf
    }

    /// Pad the buffer with zeroes to the next sector boundary.
    pub(crate) fn pad_to_sector(&mut self) {
        let rem = self.buf.len() % SECTOR;
        if rem != 0 {
            let add = SECTOR - rem;
            if let Some(end) = self.buf.len().checked_add(add) {
                self.buf.resize(end, 0);
            }
        }
    }

    /// Write one byte at `offset`, growing and zero-filling as needed.
    pub(crate) fn put_u8(&mut self, offset: usize, v: u8) {
        self.put_bytes(offset, &[v]);
    }

    /// Write a big-endian `u16` at `offset`, growing and zero-filling as needed.
    pub(crate) fn put_u16(&mut self, offset: usize, v: u16) {
        self.put_bytes(offset, &v.to_be_bytes());
    }

    /// Write a big-endian `u32` at `offset`, growing and zero-filling as needed.
    pub(crate) fn put_u32(&mut self, offset: usize, v: u32) {
        self.put_bytes(offset, &v.to_be_bytes());
    }

    /// Write a byte slice at `offset`, growing and zero-filling as needed.
    pub(crate) fn put_slice(&mut self, offset: usize, src: &[u8]) {
        self.put_bytes(offset, src);
    }

    fn put_bytes(&mut self, offset: usize, src: &[u8]) {
        if let Some(end) = offset.checked_add(src.len()) {
            if self.buf.len() < end {
                self.buf.resize(end, 0);
            }
            for (i, b) in src.iter().enumerate() {
                if let Some(pos) = offset.checked_add(i)
                    && let Some(slot) = self.buf.get_mut(pos)
                {
                    *slot = *b;
                }
            }
        }
    }
}

impl Default for Writer {
    fn default() -> Self {
        Self::new()
    }
}

/// Bounds-checked big-endian reader over hostile input.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Reader<'a> {
    data: &'a [u8],
}

impl<'a> Reader<'a> {
    /// Create a reader over `data`.
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self { data }
    }

    /// Input length.
    pub(crate) fn len(&self) -> usize {
        self.data.len()
    }

    /// Read one byte.
    pub(crate) fn u8(&self, offset: usize) -> Result<u8, IfoError> {
        self.data
            .get(offset)
            .copied()
            .ok_or_else(|| IfoError("byte out of bounds".to_string()))
    }

    /// Read a big-endian `u16`.
    pub(crate) fn u16(&self, offset: usize) -> Result<u16, IfoError> {
        let s = self.slice(offset, 2)?;
        let mut a = [0u8; 2];
        for (i, slot) in a.iter_mut().enumerate() {
            if let Some(b) = s.get(i) {
                *slot = *b;
            }
        }
        Ok(u16::from_be_bytes(a))
    }

    /// Read a big-endian `u32`.
    pub(crate) fn u32(&self, offset: usize) -> Result<u32, IfoError> {
        let s = self.slice(offset, 4)?;
        let mut a = [0u8; 4];
        for (i, slot) in a.iter_mut().enumerate() {
            if let Some(b) = s.get(i) {
                *slot = *b;
            }
        }
        Ok(u32::from_be_bytes(a))
    }

    /// Read a byte slice.
    pub(crate) fn slice(&self, offset: usize, len: usize) -> Result<&'a [u8], IfoError> {
        let end = offset
            .checked_add(len)
            .ok_or_else(|| IfoError("slice length overflow".to_string()))?;
        self.data
            .get(offset..end)
            .ok_or_else(|| IfoError("slice out of bounds".to_string()))
    }
}
